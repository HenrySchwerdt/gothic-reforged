//! Gothic 3 dialog/info node (`Infos.pak`, `*.info`): plain-text `[Info]` INI record.
//!
//! An info is one dialog line group owned by an NPC (`Owner`) with a condition
//! (`ConditionType` + `Cond*` fields) and a script: parallel arrays
//! `InfoScript_Commands/Entities1/Entities2/IDs1/IDs2/Texts` joined here into
//! [`ScriptStep`]s. `Say` texts are string-table keys (`INFO_*`).
//!
//! Decoded: field names, parallel script arrays, command vocabulary (counted in
//! surveys). Opaque: numeric `ConditionType` / `InfoType` semantics (raw ints), and
//! the exact argument meaning of `IDs1/IDs2` per command (kept as strings).
use crate::gdini::{Anomaly, Record, parse_record, split_list};
use crate::quest::{num, num_list, text, text_list};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ScriptStep {
    pub command: String,
    /// Actor reference: `player`, `npc`, `owner` or a named entity; may be empty.
    pub entity1: String,
    pub entity2: String,
    /// Command arguments: item names, counts, event names, ... (command dependent).
    pub id1: String,
    pub id2: String,
    /// String-table key for `Say`-like commands.
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Info {
    pub name: String,
    pub sort_id: u32,
    pub owner: String,
    pub info_given: bool,
    pub parent: String,
    /// Quest this info belongs to (empty if none).
    pub quest: String,
    pub condition_type: u32,
    pub info_type: u32,
    pub gold_cost: u32,
    pub cond_owner_near_entity: String,
    pub folder: String,
    /// Game events (set by `SetGameEvent`) that must be known / not known.
    pub cond_player_knows: Vec<String>,
    pub cond_player_knows_not: Vec<String>,
    pub cond_item_container: Vec<String>,
    pub cond_items: Vec<String>,
    pub cond_item_amounts: Vec<u32>,
    pub cond_secondary_npc: Vec<String>,
    pub cond_secondary_npc_states: Vec<u32>,
    pub cond_player_party_member: String,
    pub clear_children: bool,
    pub permanent: bool,
    pub suppress_log: bool,
    pub teach_attrib: String,
    pub teach_attrib_value: u32,
    pub teach_skill: String,
    pub script: Vec<ScriptStep>,
    pub extra: BTreeMap<String, String>,
    pub anomalies: Vec<Anomaly>,
}

const KNOWN: &[&str] = &[
    "Name",
    "SortID",
    "Owner",
    "InfoGiven",
    "Parent",
    "Quest",
    "ConditionType",
    "InfoType",
    "GoldCost",
    "CondOwnerNearEntity",
    "Folder",
    "CondPlayerKnows",
    "CondPlayerKnowsNot",
    "CondItemContainer",
    "CondItems",
    "CondItemAmounts",
    "CondSecondaryNPC",
    "CondSecondaryNPCstates",
    "CondPlayerPartyMember",
    "ClearChildren",
    "Permanent",
    "SuppressLog",
    "TeachAttrib",
    "TeachAttribValue",
    "TeachSkill",
    "InfoScript_Commands",
    "InfoScript_Entities1",
    "InfoScript_Entities2",
    "InfoScript_IDs1",
    "InfoScript_IDs2",
    "InfoScript_Texts",
];

fn flag(r: &Record, key: &str) -> Result<bool> {
    match r.get(key) {
        None | Some("") | Some("false") => Ok(false),
        Some("true") => Ok(true),
        Some(other) => anyhow::bail!("field {key} is not a boolean: {other:?}"),
    }
}

impl Info {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let r = parse_record(bytes, "Info")?;
        let name = text(&r, "Name");
        anyhow::ensure!(!name.is_empty(), "info has no Name");
        let commands = split_list(r.get("InfoScript_Commands").unwrap_or(""));
        let col = |key: &str| -> Vec<String> {
            split_list(r.get(key).unwrap_or(""))
                .into_iter()
                .map(str::to_string)
                .collect()
        };
        let (e1, e2, i1, i2, tx) = (
            col("InfoScript_Entities1"),
            col("InfoScript_Entities2"),
            col("InfoScript_IDs1"),
            col("InfoScript_IDs2"),
            col("InfoScript_Texts"),
        );
        let mut anomalies = r.anomalies.clone();
        for (key, v) in [
            ("InfoScript_Entities1", &e1),
            ("InfoScript_Entities2", &e2),
            ("InfoScript_IDs1", &i1),
            ("InfoScript_IDs2", &i2),
            ("InfoScript_Texts", &tx),
        ] {
            if v.len() > commands.len() {
                anomalies.push(Anomaly {
                    kind: "script_array_longer_than_commands",
                    detail: format!("{key}: {} > {}", v.len(), commands.len()),
                });
            }
        }
        let at = |v: &Vec<String>, i: usize| v.get(i).cloned().unwrap_or_default();
        let script = commands
            .iter()
            .enumerate()
            .map(|(i, c)| ScriptStep {
                command: (*c).to_string(),
                entity1: at(&e1, i),
                entity2: at(&e2, i),
                id1: at(&i1, i),
                id2: at(&i2, i),
                text: at(&tx, i),
            })
            .collect();
        let mut extra = BTreeMap::new();
        for (k, v) in &r.fields {
            if !KNOWN.contains(&k.as_str()) {
                extra.entry(k.clone()).or_insert_with(|| v.clone());
            }
        }
        Ok(Self {
            name,
            sort_id: num(&r, "SortID")?,
            owner: text(&r, "Owner"),
            info_given: flag(&r, "InfoGiven")?,
            parent: text(&r, "Parent"),
            quest: text(&r, "Quest"),
            condition_type: num(&r, "ConditionType")?,
            info_type: num(&r, "InfoType")?,
            gold_cost: num(&r, "GoldCost")?,
            cond_owner_near_entity: text(&r, "CondOwnerNearEntity"),
            folder: text(&r, "Folder"),
            cond_player_knows: text_list(&r, "CondPlayerKnows"),
            cond_player_knows_not: text_list(&r, "CondPlayerKnowsNot"),
            cond_item_container: text_list(&r, "CondItemContainer"),
            cond_items: text_list(&r, "CondItems"),
            cond_item_amounts: num_list(&r, "CondItemAmounts")?,
            cond_secondary_npc: text_list(&r, "CondSecondaryNPC"),
            cond_secondary_npc_states: num_list(&r, "CondSecondaryNPCstates")?,
            cond_player_party_member: text(&r, "CondPlayerPartyMember"),
            clear_children: flag(&r, "ClearChildren")?,
            permanent: flag(&r, "Permanent")?,
            suppress_log: flag(&r, "SuppressLog")?,
            teach_attrib: text(&r, "TeachAttrib"),
            teach_attrib_value: num(&r, "TeachAttribValue")?,
            teach_skill: text(&r, "TeachSkill"),
            script,
            extra,
            anomalies,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const SAMPLE: &str = "[Info]\r\nName=T1\r\nSortID=7\r\nOwner=Npc\r\nInfoGiven=false\r\nParent\r\nQuest=Q_A\r\nConditionType=3\r\nInfoType=3\r\nCondPlayerKnows=Ev_A;Ev_B\r\nClearChildren=false\r\nPermanent=true\r\nInfoScript_Commands=Say;Say;SetGameEvent\r\nInfoScript_Entities1=player;npc;\r\nInfoScript_Entities2=;;\r\nInfoScript_IDs1=;;Ev_C\r\nInfoScript_IDs2=;;\r\nInfoScript_Texts=INFO_T1;INFO_T1_0;;\r\nZzz=1\r\n";

    #[test]
    fn joins_parallel_arrays() {
        let i = Info::parse(SAMPLE.as_bytes()).unwrap();
        assert_eq!(i.script.len(), 3);
        assert_eq!(i.script[1].entity1, "npc");
        assert_eq!(i.script[1].text, "INFO_T1_0");
        assert_eq!(i.script[2].command, "SetGameEvent");
        assert_eq!(i.script[2].id1, "Ev_C");
        assert_eq!(i.cond_player_knows, ["Ev_A", "Ev_B"]);
        assert!(i.permanent && !i.clear_children);
        assert!(i.anomalies.is_empty());
        assert_eq!(i.extra.get("Zzz").map(String::as_str), Some("1"));
    }

    #[test]
    fn flags_overlong_arrays_and_bad_bool() {
        let i =
            Info::parse(b"[Info]\r\nName=A\r\nInfoScript_Commands=Say\r\nInfoScript_Texts=a;b\r\n")
                .unwrap();
        assert_eq!(i.anomalies[0].kind, "script_array_longer_than_commands");
        assert!(Info::parse(b"[Info]\r\nName=A\r\nPermanent=maybe\r\n").is_err());
    }

    #[test]
    fn truncation_never_panics() {
        for n in 0..SAMPLE.len() {
            let _ = Info::parse(&SAMPLE.as_bytes()[..n]);
        }
    }
}
