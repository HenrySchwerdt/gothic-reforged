//! Indexed, immutable game database: quests, infos (dialog nodes) and localized strings.
//!
//! All name lookups are ASCII-case-insensitive (the shipped data mixes case between the
//! definitions and their references, e.g. event names). Names are interned to small
//! integer ids ([`Sym`]) for the indexes; the raw records stay available unchanged.
use anyhow::{Context, Result};
use gothic_formats::{
    PakArchive,
    info::Info,
    quest::Quest,
    stringtable::{StringTable, unescape},
};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Interned (lower-cased) name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Sym(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct InfoId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct QuestId(pub u32);

#[derive(Debug, Default, Clone)]
pub struct Interner {
    map: HashMap<String, Sym>,
    names: Vec<String>,
}

impl Interner {
    pub fn intern(&mut self, name: &str) -> Sym {
        let key = name.to_ascii_lowercase();
        if let Some(&s) = self.map.get(&key) {
            return s;
        }
        let s = Sym(self.names.len() as u32);
        self.names.push(key.clone());
        self.map.insert(key, s);
        s
    }
    pub fn get(&self, name: &str) -> Option<Sym> {
        self.map.get(&name.to_ascii_lowercase()).copied()
    }
    pub fn name(&self, s: Sym) -> &str {
        &self.names[s.0 as usize]
    }
    pub fn len(&self) -> usize {
        self.names.len()
    }
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct GameDatabase {
    pub language: String,
    pub names: Interner,
    pub quests: Vec<Quest>,
    pub infos: Vec<Info>,
    pub strings: StringTable,
    quest_by_name: HashMap<Sym, QuestId>,
    info_by_name: HashMap<Sym, InfoId>,
    by_owner: HashMap<Sym, Vec<InfoId>>,
    by_quest: HashMap<Sym, Vec<InfoId>>,
    children: HashMap<Sym, Vec<InfoId>>,
    /// Interned owner per info (parallel to `infos`).
    owner_sym: Vec<Sym>,
    /// Item template names known to exist (lower-cased), if the loader had them.
    pub known_items: Option<HashSet<String>>,
}

impl GameDatabase {
    /// Build from already parsed records. `language` is a string-table language name
    /// (`German`, `English`, ...).
    pub fn from_parts(
        quests: Vec<Quest>,
        infos: Vec<Info>,
        strings: StringTable,
        language: &str,
    ) -> Result<Self> {
        let li = strings
            .language_index(language)
            .with_context(|| format!("language {language:?} not in string table"))?;
        let language = strings.languages[li].clone();
        let mut names = Interner::default();
        let mut quest_by_name = HashMap::new();
        for (i, q) in quests.iter().enumerate() {
            let s = names.intern(&q.name);
            quest_by_name.entry(s).or_insert(QuestId(i as u32));
        }
        let mut info_by_name = HashMap::new();
        let mut by_owner: HashMap<Sym, Vec<InfoId>> = HashMap::new();
        let mut by_quest: HashMap<Sym, Vec<InfoId>> = HashMap::new();
        let mut children: HashMap<Sym, Vec<InfoId>> = HashMap::new();
        let mut owner_sym = Vec::with_capacity(infos.len());
        for (i, inf) in infos.iter().enumerate() {
            let id = InfoId(i as u32);
            let s = names.intern(&inf.name);
            info_by_name.entry(s).or_insert(id);
            let o = names.intern(&inf.owner);
            owner_sym.push(o);
            by_owner.entry(o).or_default().push(id);
            if !inf.quest.is_empty() {
                let q = names.intern(&inf.quest);
                by_quest.entry(q).or_default().push(id);
            }
            if !inf.parent.is_empty() {
                let p = names.intern(&inf.parent);
                children.entry(p).or_default().push(id);
            }
        }
        // Stable presentation order: SortID, then name.
        let key = |id: &InfoId| {
            let i = &infos[id.0 as usize];
            (i.sort_id, i.name.to_ascii_lowercase())
        };
        for v in by_owner
            .values_mut()
            .chain(children.values_mut())
            .chain(by_quest.values_mut())
        {
            v.sort_by_key(key);
        }
        Ok(Self {
            language,
            names,
            quests,
            infos,
            strings,
            quest_by_name,
            info_by_name,
            by_owner,
            by_quest,
            children,
            owner_sym,
            known_items: None,
        })
    }

    /// Load `Quests.pak`, `Infos.pak`, `Strings.pak` from a game `Data` directory.
    pub fn load_install(data_dir: &Path, language: &str) -> Result<Self> {
        let strings = {
            let mut pak = PakArchive::open(data_dir.join("Strings.pak"))?;
            let i = pak
                .entries
                .iter()
                .position(|e| e.path.to_ascii_lowercase().ends_with("stringtable.ini"))
                .context("stringtable.ini not in Strings.pak")?;
            StringTable::parse(&pak.read_payload(i)?)?
        };
        let quests = read_all(&data_dir.join("Quests.pak"), ".quest", Quest::parse)?;
        let infos = read_all(&data_dir.join("Infos.pak"), ".info", Info::parse)?;
        Self::from_parts(quests, infos, strings, language)
    }

    pub fn sym(&self, name: &str) -> Option<Sym> {
        self.names.get(name)
    }
    pub fn quest_id(&self, name: &str) -> Option<QuestId> {
        self.sym(name)
            .and_then(|s| self.quest_by_name.get(&s))
            .copied()
    }
    pub fn quest(&self, id: QuestId) -> &Quest {
        &self.quests[id.0 as usize]
    }
    pub fn quest_by_name(&self, name: &str) -> Option<&Quest> {
        self.quest_id(name).map(|q| self.quest(q))
    }
    pub fn info_id(&self, name: &str) -> Option<InfoId> {
        self.sym(name)
            .and_then(|s| self.info_by_name.get(&s))
            .copied()
    }
    pub fn info(&self, id: InfoId) -> &Info {
        &self.infos[id.0 as usize]
    }
    /// All infos owned by an NPC, in presentation order.
    pub fn infos_of(&self, owner: &str) -> &[InfoId] {
        self.sym(owner)
            .and_then(|s| self.by_owner.get(&s))
            .map_or(&[], Vec::as_slice)
    }
    pub fn infos_of_quest(&self, quest: &str) -> &[InfoId] {
        self.sym(quest)
            .and_then(|s| self.by_quest.get(&s))
            .map_or(&[], Vec::as_slice)
    }
    /// Child infos (player dialog choices) of a parent info, in presentation order.
    pub fn children_of(&self, parent: &str) -> &[InfoId] {
        self.sym(parent)
            .and_then(|s| self.children.get(&s))
            .map_or(&[], Vec::as_slice)
    }
    pub fn owners(&self) -> impl Iterator<Item = &str> {
        self.by_owner.keys().map(|&s| self.names.name(s))
    }
    pub fn owner_of(&self, id: InfoId) -> Sym {
        self.owner_sym[id.0 as usize]
    }

    /// Localized, unescaped text for a string-table key in the selected language.
    pub fn text(&self, key: &str) -> Option<String> {
        if key.is_empty() {
            return None;
        }
        let li = self.strings.language_index(&self.language)?;
        let e = self.strings.get_loose(key)?;
        let t = e.texts.get(li)?;
        Some(unescape(t))
    }
}

fn read_all<T>(path: &Path, ext: &str, parse: impl Fn(&[u8]) -> Result<T>) -> Result<Vec<T>> {
    let mut pak = PakArchive::open(path)?;
    let mut out = Vec::new();
    for i in 0..pak.entries.len() {
        let e = &pak.entries[i];
        if !e.path.to_ascii_lowercase().ends_with(ext) || e.payloadless() {
            continue;
        }
        let p = e.path.clone();
        let bytes = pak.read_payload(i)?;
        out.push(parse(&bytes).with_context(|| format!("parsing {p}"))?);
    }
    Ok(out)
}
