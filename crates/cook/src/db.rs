//! Cooked [`GameDatabase`]: parsed quests, infos and string table as one bincode blob.
//! Name indexes are rebuilt on load (a few ms) instead of re-parsing ~tens of thousands of
//! INI/Genome files.
use anyhow::Result;
use gothic_formats::{info::Info, quest::Quest, stringtable::StringTable};
use gothic_gameplay::db::GameDatabase;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Payload {
    language: String,
    quests: Vec<Quest>,
    infos: Vec<Info>,
    strings: StringTable,
    known_items: Option<Vec<String>>,
}

pub fn encode(db: &GameDatabase) -> Result<Vec<u8>> {
    let known_items = db.known_items.as_ref().map(|s| {
        let mut v: Vec<String> = s.iter().cloned().collect();
        v.sort();
        v
    });
    Ok(bincode::serialize(&Payload {
        language: db.language.clone(),
        quests: db.quests.clone(),
        infos: db.infos.clone(),
        strings: db.strings.clone(),
        known_items,
    })?)
}

pub fn decode(bytes: &[u8]) -> Result<GameDatabase> {
    let mut p: Payload = bincode::deserialize(bytes)?;
    p.strings.rebuild_index();
    let mut db = GameDatabase::from_parts(p.quests, p.infos, p.strings, &p.language)?;
    db.known_items = p.known_items.map(|v| v.into_iter().collect());
    Ok(db)
}
