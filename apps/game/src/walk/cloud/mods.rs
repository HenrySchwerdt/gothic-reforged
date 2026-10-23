//! Data overlay mods (`gothic-mods`) on the play mode's database: quests, dialog infos and
//! strings ([`GameDatabase`]) and item templates ([`ItemDb`]). The enabled mods and their
//! order come from the mod list (`<dir>/modlist.toml`, as `modtool enable/disable/move` keep
//! it; mods not in the list are enabled).
use anyhow::{Context, Result};
use gothic_gameplay::GameDatabase;
use gothic_mods::{LoadOrder, ModData, ModReport, apply};
use gothic_modtool::ModList;
use gothic_sim::item::ItemDb;
use std::path::Path;

/// The enabled, loadable mods of `dir` in load order (problems kept for the report).
pub(crate) fn load_order(dir: &Path) -> Result<LoadOrder> {
    let list = ModList::load(dir).with_context(|| format!("mod list of {}", dir.display()))?;
    Ok(LoadOrder {
        order: list.active_mods(),
        problems: list.load_order().1,
    })
}

/// Apply `order` on top of the database and the item templates. `npcs` are the NPC template
/// records in their `npcs.json` form (only validated: the play mode has no template table
/// that would use them).
pub(crate) fn overlay(
    db: GameDatabase,
    items: ItemDb,
    npcs: Vec<serde_json::Value>,
    order: &LoadOrder,
) -> Result<(GameDatabase, ItemDb, ModReport)> {
    let base_items: Vec<_> = items.iter().cloned().collect();
    let base = ModData::new(&db.quests, &db.infos, db.strings.clone(), &base_items, npcs);
    let (data, report) = apply(base, order);
    let modded_db = data
        .to_database(&db.language)
        .context("building the modded database")?;
    let mut modded_items = ItemDb::default();
    for item in data.items_typed().context("modded item templates")? {
        modded_items.insert(item);
    }
    data.npcs_typed().context("modded NPC records")?;
    Ok((modded_db, modded_items, report))
}

/// NPC templates of `.local/npcs.json` as JSON records (empty when the file is missing).
fn npc_templates(path: &Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("templates")?.as_array().cloned())
        .unwrap_or_default()
}

/// The play-mode hook: unchanged data unless `--mods` / `GR_MODS` is given. A mods folder that
/// cannot be read or a modded database that does not build is reported and the original data
/// is kept.
pub(crate) fn apply_opt_in(
    args: &[String],
    db: GameDatabase,
    items: ItemDb,
) -> (GameDatabase, ItemDb) {
    let Some(dir) = super::Options::from_process(args).mods else {
        return (db, items);
    };
    let order = match load_order(&dir) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("mods: {e:#}; playing without mods");
            return (db, items);
        }
    };
    let started = std::time::Instant::now();
    let npcs = npc_templates(Path::new(".local/npcs.json"));
    // `overlay` consumes its inputs; keep the originals for the fallback.
    match overlay(db.clone(), items.clone(), npcs, &order) {
        Ok((db, items, report)) => {
            eprint!("{}", report.to_text());
            eprintln!(
                "mods: data overlay from {} in {} ms",
                dir.display(),
                started.elapsed().as_millis()
            );
            (db, items)
        }
        Err(e) => {
            eprintln!("mods: {e:#}; playing without data mods");
            (db, items)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::strings;
    use super::*;
    use gothic_formats::{info::Info, quest::Quest};
    use gothic_sim::item::ItemDef;
    use std::path::PathBuf;

    fn synthetic() -> (GameDatabase, ItemDb) {
        let quests = vec![Quest::parse(b"[Quest]\r\nName=Q_Letter\r\nType=1\r\nDeliveryEntities=Bob\r\nDeliveryAmounts=1\r\nExperiencePoints=50\r\n").unwrap()];
        let infos = vec![Info::parse(b"[Info]\r\nName=A_Intro\r\nSortID=1\r\nOwner=Alice\r\nConditionType=3\r\nInfoType=3\r\nInfoScript_Commands=Say\r\nInfoScript_Entities1=npc;\r\nInfoScript_Entities2=;\r\nInfoScript_IDs1=;\r\nInfoScript_IDs2=;\r\nInfoScript_Texts=T_HELLO;\r\n").unwrap()];
        let db = GameDatabase::from_parts(quests, infos, strings(), "German").unwrap();
        let mut items = ItemDb::default();
        items.insert(ItemDef {
            name: "It_Herb".into(),
            category: "Plant".into(),
            value: 5,
            ..ItemDef::default()
        });
        (db, items)
    }

    fn examples() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mods/examples")
    }

    #[test]
    fn cloud_mods_overlay_applies_example_mod() {
        let (db, items) = synthetic();
        let order = load_order(&examples()).unwrap();
        let ids: Vec<&str> = order.order.iter().map(|m| m.id()).collect();
        // Script-only mods are in the list too; they carry no patches.
        assert!(ids.contains(&"forest_hermit"), "{ids:?}");
        let npcs = vec![serde_json::json!({"name": "Alice", "species": "Human", "level": 3})];
        let (db, items, report) = overlay(db, items, npcs, &order).unwrap();
        assert!(report.errors.is_empty(), "{}", report.to_text());
        assert!(report.applied.iter().any(|a| a == "forest_hermit@1.0.0"));
        assert!(db.quests.iter().any(|q| q.name == "Q_HermitTea"));
        assert!(db.infos.iter().any(|i| i.name == "H_Quest"));
        assert!(db.quests.iter().any(|q| q.name == "Q_Letter"), "base kept");
        assert!(items.get("It_HermitTea").is_some());
        assert!(items.get("it_herb").is_some(), "base kept");
        assert_eq!(db.language, "German");
        assert!(db.strings.language_index("German").is_some());
    }

    #[test]
    fn cloud_mods_missing_dir_is_empty_order() {
        let dir = std::env::temp_dir().join(format!("gr-cloud-no-mods-{}", std::process::id()));
        let order = load_order(&dir).unwrap();
        assert!(order.order.is_empty());
        let (db, items) = synthetic();
        let (db2, items2, report) = overlay(db.clone(), items, Vec::new(), &order).unwrap();
        assert!(report.applied.is_empty());
        assert_eq!(db2.quests.len(), db.quests.len());
        assert_eq!(items2.len(), 1);
    }
}
