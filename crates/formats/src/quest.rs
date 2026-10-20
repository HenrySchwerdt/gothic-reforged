//! Gothic 3 quest definition (`Quests.pak`, `*.quest`): plain-text `[Quest]` INI record.
//!
//! Decoded: all field names/types observed in the 570 installed files.
//! Opaque: the numeric meaning of `Type`, `PoliticalSuccess`, `JobSuccess` (kept
//! as raw integers; nothing here maps them to named enums). Quest *state
//! transitions* are not stored here — they are driven by `.info` scripts
//! (`RunQuest`/`SucceedQuest`/`CloseQuest`/`AddQuestLog`, see `info.rs`).
use crate::gdini::{Anomaly, Record, parse_record, split_list};
use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize)]
pub struct Quest {
    pub name: String,
    /// Raw `Type` integer (0..=13 observed); semantics undecoded.
    pub quest_type: u32,
    /// Quests that must be finished before this one (`FinishedQuests`).
    pub finished_quests: Vec<String>,
    /// String-table key of the log topic title (e.g. `QUEST_FILLER60`).
    pub log_topic: String,
    /// Optional log-text key; empty in all but unusual files.
    pub log_text: String,
    pub experience_points: u32,
    pub political_success: u32,
    pub political_success_amount: u32,
    pub enclave_success: String,
    pub enclave_success_amount: u32,
    pub job_success: u32,
    pub job_success_amount: u32,
    /// Slash-terminated folder (e.g. `Varant/Braga/`).
    pub folder: String,
    pub delivery_entities: Vec<String>,
    pub delivery_amounts: Vec<u32>,
    pub delivery_counter: Vec<u32>,
    pub destination_entity: String,
    pub running_time_years: u32,
    pub running_time_days: u32,
    pub running_time_hours: u32,
    pub attrib_success: String,
    pub attrib_success_amount: u32,
    pub spell_name: String,
    pub quest_description: String,
    /// Fields not listed above, preserved verbatim.
    pub extra: BTreeMap<String, String>,
    pub anomalies: Vec<Anomaly>,
}

const KNOWN: &[&str] = &[
    "Name",
    "Type",
    "FinishedQuests",
    "LogTopic",
    "LogText",
    "ExperiencePoints",
    "PoliticalSuccess",
    "PoliticalSuccessAmount",
    "EnclaveSuccess",
    "EnclaveSuccessAmount",
    "JobSuccess",
    "JobSuccessAmount",
    "Folder",
    "DeliveryEntities",
    "DeliveryAmounts",
    "DeliveryCounter",
    "DestinationEntity",
    "RunningTimeYears",
    "RunningTimeDays",
    "RunningTimeHours",
    "AttribSuccess",
    "AttribSuccessAmount",
    "SpellName",
    "QuestDescription",
];

pub(crate) fn num(r: &Record, key: &str) -> Result<u32> {
    match r.get(key) {
        None | Some("") => Ok(0),
        Some(v) => v
            .trim()
            .parse()
            .with_context(|| format!("field {key} is not an unsigned integer: {v:?}")),
    }
}

pub(crate) fn num_list(r: &Record, key: &str) -> Result<Vec<u32>> {
    split_list(r.get(key).unwrap_or(""))
        .into_iter()
        .map(|v| {
            v.trim()
                .parse()
                .with_context(|| format!("field {key} list item {v:?}"))
        })
        .collect()
}

pub(crate) fn text(r: &Record, key: &str) -> String {
    r.get(key).unwrap_or("").to_string()
}

pub(crate) fn text_list(r: &Record, key: &str) -> Vec<String> {
    split_list(r.get(key).unwrap_or(""))
        .into_iter()
        .map(str::to_string)
        .collect()
}

impl Quest {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let r = parse_record(bytes, "Quest")?;
        let name = text(&r, "Name");
        anyhow::ensure!(!name.is_empty(), "quest has no Name");
        let mut extra = BTreeMap::new();
        for (k, v) in &r.fields {
            if !KNOWN.contains(&k.as_str()) {
                extra.entry(k.clone()).or_insert_with(|| v.clone());
            }
        }
        Ok(Self {
            name,
            quest_type: num(&r, "Type")?,
            finished_quests: text_list(&r, "FinishedQuests"),
            log_topic: text(&r, "LogTopic"),
            log_text: text(&r, "LogText"),
            experience_points: num(&r, "ExperiencePoints")?,
            political_success: num(&r, "PoliticalSuccess")?,
            political_success_amount: num(&r, "PoliticalSuccessAmount")?,
            enclave_success: text(&r, "EnclaveSuccess"),
            enclave_success_amount: num(&r, "EnclaveSuccessAmount")?,
            job_success: num(&r, "JobSuccess")?,
            job_success_amount: num(&r, "JobSuccessAmount")?,
            folder: text(&r, "Folder"),
            delivery_entities: text_list(&r, "DeliveryEntities"),
            delivery_amounts: num_list(&r, "DeliveryAmounts")?,
            delivery_counter: num_list(&r, "DeliveryCounter")?,
            destination_entity: text(&r, "DestinationEntity"),
            running_time_years: num(&r, "RunningTimeYears")?,
            running_time_days: num(&r, "RunningTimeDays")?,
            running_time_hours: num(&r, "RunningTimeHours")?,
            attrib_success: text(&r, "AttribSuccess"),
            attrib_success_amount: num(&r, "AttribSuccessAmount")?,
            spell_name: text(&r, "SpellName"),
            quest_description: text(&r, "QuestDescription"),
            extra,
            anomalies: r.anomalies,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const SAMPLE: &str = "[Quest]\r\nName=Test_Quest\r\nType=2\r\nFinishedQuests\r\nLogTopic=QUEST_TEST\r\nLogText\r\nExperiencePoints=300\r\nDeliveryEntities=It_A;It_B\r\nDeliveryAmounts=1;5\r\nDeliveryCounter=0;0\r\nFolder=Varant/X/\r\nNewKey=1\r\n";

    #[test]
    fn parses_synthetic_quest() {
        let q = Quest::parse(SAMPLE.as_bytes()).unwrap();
        assert_eq!(q.name, "Test_Quest");
        assert_eq!(q.quest_type, 2);
        assert!(q.finished_quests.is_empty());
        assert_eq!(q.delivery_entities, ["It_A", "It_B"]);
        assert_eq!(q.delivery_amounts, [1, 5]);
        assert_eq!(q.extra.get("NewKey").map(String::as_str), Some("1"));
    }

    #[test]
    fn rejects_bad_numbers_and_missing_name() {
        assert!(Quest::parse(b"[Quest]\r\nName=A\r\nType=x\r\n").is_err());
        assert!(Quest::parse(b"[Quest]\r\nType=1\r\n").is_err());
        assert!(Quest::parse(b"[Info]\r\nName=A\r\n").is_err());
    }

    #[test]
    fn truncation_never_panics() {
        for n in 0..SAMPLE.len() {
            let _ = Quest::parse(&SAMPLE.as_bytes()[..n]);
        }
    }
}
