use gothic_gameplay::db::GameDatabase;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let db = GameDatabase::load_install(
        std::path::Path::new("D:/SteamLibrary/steamapps/common/Gothic 3/Data"),
        "German",
    )
    .unwrap();
    if a[1] == "owners" {
        let mut v: Vec<_> = db.owners().map(|o| (db.infos_of(o).len(), o.to_string())).collect();
        v.sort();
        for (n, o) in v.iter().rev().take(60) { println!("{n} {o}"); }
        return;
    }
    let ids: Vec<_> = if a[1].starts_with("q:") {
        db.infos_of_quest(&a[1][2..]).to_vec()
    } else {
        db.infos_of(&a[1]).to_vec()
    };
    for id in ids {
        let i = db.info(id);
        println!(
            "== {} owner={} sort={} par={:?} q={:?} ct={} it={} perm={} clr={} gold={} knows={:?} not={:?} items={:?}/{:?} sec={:?}/{:?}",
            i.name, i.owner, i.sort_id, i.parent, i.quest, i.condition_type, i.info_type, i.permanent,
            i.clear_children, i.gold_cost, i.cond_player_knows, i.cond_player_knows_not, i.cond_items,
            i.cond_item_amounts, i.cond_secondary_npc, i.cond_secondary_npc_states
        );
        if std::env::var("SHORT").is_ok() {
            let c: Vec<String> = i.script.iter().filter(|s| s.command != "Say").map(|s| format!("{}({},{},{})", s.command, s.entity1, s.id1, s.id2)).collect();
            println!("   -> {}", c.join(" "));
            continue;
        }
        for s in &i.script {
            let t = db.text(&s.text).unwrap_or_default();
            println!(
                "   {} e1={} e2={} i1={} i2={} {}",
                s.command, s.entity1, s.entity2, s.id1, s.id2,
                t.chars().take(70).collect::<String>()
            );
        }
    }
}
