//! Static conflict report: which mods write the same record field (or whole record, or
//! string) — no game data needed. The later mod in load order wins.
use gothic_mods::{LoadedMod, RecordKind};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WriteConflict {
    pub kind: RecordKind,
    pub record: String,
    /// Field (`*` for the whole record, the language for strings).
    pub field: String,
    pub earlier: String,
    pub winner: String,
}

impl std::fmt::Display for WriteConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {} {}: {} overrides {}",
            self.kind.name(),
            self.record,
            if self.field == "*" {
                "(whole record)".to_string()
            } else {
                format!("field {}", self.field)
            },
            self.winner,
            self.earlier
        )
    }
}

/// Conflicts between `mods` (in load order).
pub fn static_conflicts(mods: &[LoadedMod]) -> Vec<WriteConflict> {
    // (kind, record lower-case) -> field -> last writer.
    let mut writes: BTreeMap<(RecordKind, String), (String, BTreeMap<String, String>)> =
        BTreeMap::new();
    let mut out = Vec::new();
    for m in mods {
        let id = m.id();
        let mut touch =
            |kind: RecordKind, name: &str, field: &str, out: &mut Vec<WriteConflict>| {
                let key = (kind, name.to_ascii_lowercase());
                let (display, fields) = writes
                    .entry(key)
                    .or_insert_with(|| (name.to_string(), BTreeMap::new()));
                let mut earlier: Vec<String> = if field == "*" {
                    fields.values().cloned().collect()
                } else {
                    fields
                        .get(field)
                        .into_iter()
                        .chain(fields.get("*"))
                        .cloned()
                        .collect()
                };
                earlier.sort();
                earlier.dedup();
                for e in earlier {
                    if e != id {
                        out.push(WriteConflict {
                            kind,
                            record: display.clone(),
                            field: field.to_string(),
                            earlier: e,
                            winner: id.to_string(),
                        });
                    }
                }
                if field == "*" {
                    fields.clear();
                }
                fields.insert(field.to_string(), id.to_string());
            };
        for (_, patch) in &m.patches {
            for kind in [
                RecordKind::Quest,
                RecordKind::Info,
                RecordKind::Item,
                RecordKind::Npc,
            ] {
                let Some(ops) = patch.ops(kind) else { continue };
                for add in &ops.add {
                    if let Some(name) = add.get("name").and_then(|v| v.as_str()) {
                        touch(kind, name, "*", &mut out);
                    }
                }
                for p in &ops.patch {
                    for field in p.set.keys() {
                        touch(kind, &p.name, field, &mut out);
                    }
                }
                for d in &ops.delete {
                    touch(kind, &d.name, "*", &mut out);
                }
            }
            for s in &patch.strings.set {
                for lang in s.texts.keys() {
                    touch(RecordKind::String, &s.key, lang, &mut out);
                }
            }
            for d in &patch.strings.delete {
                touch(RecordKind::String, &d.key, "*", &mut out);
            }
        }
    }
    out.sort_by(|a, b| {
        (a.kind, &a.record, &a.field, &a.winner).cmp(&(b.kind, &b.record, &b.field, &b.winner))
    });
    out.dedup();
    out
}
