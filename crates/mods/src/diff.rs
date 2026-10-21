//! Diff two data sets into a patch: `apply(base, diff(base, modded)) == modded`.
//!
//! Used for round-trip checks and to turn an edited export back into a mod.
use crate::data::{ModData, record_name};
use crate::patch::{Delete, FieldPatch, PatchFile, RecordKind, StringDelete, StringSet};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// Drop `null`s (unset optional fields): an add starts from the record's defaults anyway,
/// and TOML has no null.
fn strip_nulls(v: &Value) -> Option<Value> {
    match v {
        Value::Null => None,
        Value::Object(m) => Some(Value::Object(
            m.iter()
                .filter_map(|(k, v)| strip_nulls(v).map(|v| (k.clone(), v)))
                .collect(),
        )),
        Value::Array(a) => Some(Value::Array(a.iter().filter_map(strip_nulls).collect())),
        other => Some(other.clone()),
    }
}

/// The patch that turns `base` into `modded`. Changing an optional field back to unset
/// (`null`) can only be written as RON, not TOML.
pub fn diff(base: &ModData, modded: &ModData) -> PatchFile {
    let mut p = PatchFile::default();
    for kind in [
        RecordKind::Quest,
        RecordKind::Info,
        RecordKind::Item,
        RecordKind::Npc,
    ] {
        let ops = p.ops_mut(kind).expect("record kind");
        let b = base.records(kind);
        let m = modded.records(kind);
        let find = |list: &[Value], name: &str| {
            list.iter()
                .find(|r| record_name(r).is_some_and(|n| n.eq_ignore_ascii_case(name)))
                .cloned()
        };
        for rec in b {
            let Some(name) = record_name(rec) else {
                continue;
            };
            match find(m, name) {
                None => ops.delete.push(Delete {
                    name: name.to_string(),
                }),
                Some(new) if &new != rec => {
                    let (Some(old), Some(new)) = (rec.as_object(), new.as_object()) else {
                        continue;
                    };
                    let set: Map<String, Value> = new
                        .iter()
                        .filter(|(k, v)| old.get(*k) != Some(*v))
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    if !set.is_empty() {
                        ops.patch.push(FieldPatch {
                            name: name.to_string(),
                            set,
                        });
                    }
                }
                Some(_) => {}
            }
        }
        for rec in m {
            let Some(name) = record_name(rec) else {
                continue;
            };
            if find(b, name).is_none()
                && let Some(Value::Object(obj)) = strip_nulls(rec)
            {
                ops.add.push(obj);
            }
        }
    }
    let langs = &modded.strings.languages;
    for e in &modded.strings.entries {
        let old = base
            .strings
            .entries
            .iter()
            .find(|x| x.key.eq_ignore_ascii_case(&e.key));
        let texts: BTreeMap<String, String> = langs
            .iter()
            .enumerate()
            .filter(|(i, _)| old.is_none_or(|o| o.texts.get(*i) != e.texts.get(*i)))
            .map(|(i, l)| (l.clone(), e.texts.get(i).cloned().unwrap_or_default()))
            .collect();
        if !texts.is_empty() {
            p.strings.set.push(StringSet {
                key: e.key.clone(),
                texts,
            });
        }
    }
    for e in &base.strings.entries {
        if !modded
            .strings
            .entries
            .iter()
            .any(|x| x.key.eq_ignore_ascii_case(&e.key))
        {
            p.strings.delete.push(StringDelete { key: e.key.clone() });
        }
    }
    p
}
