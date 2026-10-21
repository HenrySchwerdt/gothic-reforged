//! Applying mods in load order, with conflict detection.
//!
//! Every write is attributed to (record kind, record name, field) and the mod that made it.
//! When a later mod writes something an earlier mod already wrote, that is a **conflict**: the
//! later mod wins (load order) and the report says so. Operations that cannot apply (patch or
//! delete of a record that does not exist, adding one that does, unknown fields, values of the
//! wrong type) are **errors**: the operation is skipped, everything else still applies.
use crate::data::{ModData, empty_record, validate};
use crate::loader::{LoadOrder, OrderProblem};
use crate::patch::{PatchFile, RecordKind};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

/// Two mods wrote the same thing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Conflict {
    pub kind: RecordKind,
    pub record: String,
    /// The field, or `*` for the whole record (add / delete).
    pub field: String,
    /// Mod that wrote first.
    pub first: String,
    /// Mod that wrote later and wins.
    pub winner: String,
}

/// An operation that was skipped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OpError {
    #[serde(rename = "mod")]
    pub mod_id: String,
    pub file: String,
    pub kind: RecordKind,
    pub record: String,
    pub message: String,
}

/// Result of applying mods.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ModReport {
    /// Mods applied, in order (`id@version`).
    pub applied: Vec<String>,
    pub order_problems: Vec<OrderProblem>,
    pub conflicts: Vec<Conflict>,
    pub errors: Vec<OpError>,
    /// Operations that took effect.
    pub operations: usize,
}

impl ModReport {
    pub fn is_clean(&self) -> bool {
        self.order_problems.is_empty() && self.conflicts.is_empty() && self.errors.is_empty()
    }

    /// Human-readable report.
    pub fn to_text(&self) -> String {
        use std::fmt::Write as _;
        let mut o = String::new();
        let _ = writeln!(
            o,
            "mods: {} applied ({}), {} operations, {} conflicts, {} errors, {} load-order problems",
            self.applied.len(),
            self.applied.join(", "),
            self.operations,
            self.conflicts.len(),
            self.errors.len(),
            self.order_problems.len()
        );
        for p in &self.order_problems {
            let _ = writeln!(o, "LOAD {p:?}");
        }
        for c in &self.conflicts {
            let _ = writeln!(
                o,
                "CONFLICT {} {} field {}: {} overrides {}",
                c.kind.name(),
                c.record,
                c.field,
                c.winner,
                c.first
            );
        }
        for e in &self.errors {
            let _ = writeln!(
                o,
                "ERROR {} ({}) {} {}: {}",
                e.mod_id,
                e.file,
                e.kind.name(),
                e.record,
                e.message
            );
        }
        o
    }
}

/// Applies patches and tracks who wrote what.
pub struct Applier {
    pub data: ModData,
    pub report: ModReport,
    writes: BTreeMap<(RecordKind, String, String), String>,
}

impl Applier {
    pub fn new(base: ModData) -> Self {
        Self {
            data: base,
            report: ModReport::default(),
            writes: BTreeMap::new(),
        }
    }

    fn write(&mut self, mod_id: &str, kind: RecordKind, record: &str, field: &str) {
        let key = (kind, record.to_ascii_lowercase(), field.to_string());
        // A whole-record write also touches every field written before.
        let earlier: Vec<String> = self
            .writes
            .iter()
            .filter(|((k, r, f), m)| {
                *k == kind
                    && *r == key.1
                    && (f == field || field == "*" || f == "*")
                    && m.as_str() != mod_id
            })
            .map(|(_, m)| m.clone())
            .collect();
        for first in earlier {
            let c = Conflict {
                kind,
                record: record.to_string(),
                field: field.to_string(),
                first,
                winner: mod_id.to_string(),
            };
            if !self.report.conflicts.contains(&c) {
                self.report.conflicts.push(c);
            }
        }
        self.writes.insert(key, mod_id.to_string());
    }

    fn error(&mut self, mod_id: &str, file: &str, kind: RecordKind, record: &str, msg: String) {
        self.report.errors.push(OpError {
            mod_id: mod_id.to_string(),
            file: file.to_string(),
            kind,
            record: record.to_string(),
            message: msg,
        });
    }

    /// Apply one patch file on behalf of `mod_id`.
    pub fn apply_file(&mut self, mod_id: &str, file: &str, patch: &PatchFile) {
        for kind in [
            RecordKind::Quest,
            RecordKind::Info,
            RecordKind::Item,
            RecordKind::Npc,
        ] {
            let ops = patch.ops(kind).expect("record kind").clone();
            for add in ops.add {
                let Some(name) = add.get("name").and_then(Value::as_str).map(str::to_string) else {
                    self.error(mod_id, file, kind, "?", "add without a name".into());
                    continue;
                };
                if self.data.find(kind, &name).is_some() {
                    self.error(
                        mod_id,
                        file,
                        kind,
                        &name,
                        "already exists (use patch)".into(),
                    );
                    continue;
                }
                let mut rec = empty_record(kind, &name);
                if let Err(msg) = merge(kind, &mut rec, &add, true) {
                    self.error(mod_id, file, kind, &name, msg);
                    continue;
                }
                if let Err(e) = validate(kind, &rec) {
                    self.error(mod_id, file, kind, &name, format!("{e:#}"));
                    continue;
                }
                self.data.records_mut(kind).push(rec);
                self.write(mod_id, kind, &name, "*");
                self.report.operations += 1;
            }
            for p in ops.patch {
                let Some(i) = self.data.find(kind, &p.name) else {
                    self.error(mod_id, file, kind, &p.name, "no such record".into());
                    continue;
                };
                let mut rec = self.data.records(kind)[i].clone();
                if let Err(msg) = merge(kind, &mut rec, &p.set, false) {
                    self.error(mod_id, file, kind, &p.name, msg);
                    continue;
                }
                if let Err(e) = validate(kind, &rec) {
                    self.error(mod_id, file, kind, &p.name, format!("{e:#}"));
                    continue;
                }
                self.data.records_mut(kind)[i] = rec;
                for f in p.set.keys() {
                    self.write(mod_id, kind, &p.name, f);
                }
                self.report.operations += 1;
            }
            for d in ops.delete {
                let Some(i) = self.data.find(kind, &d.name) else {
                    self.error(mod_id, file, kind, &d.name, "no such record".into());
                    continue;
                };
                self.data.records_mut(kind).remove(i);
                self.write(mod_id, kind, &d.name, "*");
                self.report.operations += 1;
            }
        }
        for s in &patch.strings.set {
            let unknown: Vec<&String> = s
                .texts
                .keys()
                .filter(|l| {
                    !self
                        .data
                        .strings
                        .languages
                        .iter()
                        .any(|x| x.eq_ignore_ascii_case(l))
                })
                .collect();
            if !unknown.is_empty() {
                let msg = format!("unknown languages {unknown:?}");
                self.error(mod_id, file, RecordKind::String, &s.key, msg);
                continue;
            }
            self.data.set_string(&s.key, &s.texts);
            for lang in s.texts.keys() {
                self.write(mod_id, RecordKind::String, &s.key, lang);
            }
            self.report.operations += 1;
        }
        for d in &patch.strings.delete {
            if self.data.delete_string(&d.key) {
                self.write(mod_id, RecordKind::String, &d.key, "*");
                self.report.operations += 1;
            } else {
                self.error(
                    mod_id,
                    file,
                    RecordKind::String,
                    &d.key,
                    "no such string".into(),
                );
            }
        }
    }
}

/// Set top-level fields of `rec` from `set`. Unknown fields are errors for kinds whose JSON
/// form lists every field (all but NPC records, which keep only the fields they were given).
/// `allow_name`: adds carry their own name; patches may not rename (identity).
fn merge(
    kind: RecordKind,
    rec: &mut Value,
    set: &serde_json::Map<String, Value>,
    allow_name: bool,
) -> Result<(), String> {
    let obj = rec.as_object_mut().ok_or("record is not an object")?;
    for (k, v) in set {
        if k == "name" && !allow_name {
            return Err("a patch cannot rename a record (delete and add instead)".into());
        }
        if kind != RecordKind::Npc && !obj.contains_key(k) {
            return Err(format!("unknown field {k:?}"));
        }
        obj.insert(k.clone(), v.clone());
    }
    Ok(())
}

/// Apply resolved mods to `base`.
pub fn apply(base: ModData, order: &LoadOrder) -> (ModData, ModReport) {
    let mut a = Applier::new(base);
    a.report.order_problems = order.problems.clone();
    for m in &order.order {
        for (file, patch) in &m.patches {
            a.apply_file(m.id(), file, patch);
        }
        a.report
            .applied
            .push(format!("{}@{}", m.id(), m.manifest.info.version));
    }
    (a.data, a.report)
}
