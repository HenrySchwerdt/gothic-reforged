//! Serializable game state: the save-game core for quests, dialogs and the player record.
//!
//! Everything is keyed by lower-cased names (not by database indices) so that a save
//! stays valid when the database is rebuilt or patched. Maps are `BTree*` so
//! serialization is deterministic byte-for-byte.
use crate::model::{NpcStatus, QuestStatus};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Bumped when the serialized shape changes; [`GameState::from_json`] rejects newer saves
/// and migrates older ones (none exist yet).
pub const SAVE_VERSION: u32 = 1;

/// Name of the gold item template; gold is an ordinary inventory stack in Gothic 3
/// (`Give ... It_Gold <amount>` in the info scripts).
pub const GOLD_ITEM: &str = "it_gold";

pub fn norm(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestRuntime {
    pub status: Option<QuestStatus>,
    /// Objective progress per delivery entity (kills, area entered, ...). Items are
    /// counted from the inventory instead.
    pub progress: BTreeMap<String, u32>,
    /// World clock (hours) when the quest last became `Running` (GAME_RULES 2.2).
    pub activation_hours: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NpcRuntime {
    pub dead: bool,
    /// Defeated by the player in a fight (`NPC_PS+0xa4`, GAME_RULES 1.3).
    pub defeated: bool,
    pub talked: bool,
    pub mob_enabled: Option<bool>,
    pub slavery_enabled: Option<bool>,
    pub teach_enabled: Option<bool>,
    pub trade_enabled: Option<bool>,
    pub party_enabled: Option<bool>,
    pub routine: String,
    pub pal: String,
    /// Remaining per-NPC settings forwarded by commands the simulation does not
    /// interpret yet (guard status, slavery flag, sleeping points ...): `command -> value`.
    pub settings: BTreeMap<String, String>,
    pub inventory: BTreeMap<String, u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogKind {
    /// Quest status changed.
    Status(QuestStatus),
    /// `AddQuestLog` text key.
    Text(String),
    /// Dialog excerpt recorded because the info is bound to a quest and not `SuppressLog`.
    Dialog(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEntry {
    pub quest: String,
    pub kind: LogKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GameState {
    pub version: u32,
    pub quests: BTreeMap<String, QuestRuntime>,
    /// Global named flags (`SetGameEvent` / `ClearGameEvent`).
    pub events: BTreeSet<String>,
    /// Infos already given (non-permanent infos disappear afterwards).
    pub told: BTreeSet<String>,
    pub npcs: BTreeMap<String, NpcRuntime>,
    /// Player inventory: item template name -> count (gold is `it_gold`).
    pub inventory: BTreeMap<String, u32>,
    pub xp: u64,
    pub attributes: BTreeMap<String, i32>,
    /// Fame 0..=100 per political alignment (`pol:<tag>`) and enclave (`enc:<name>`),
    /// changed by quest rewards and `ModReputPol` / `ModReputEnc` (GAME_RULES 2.3, 3).
    pub fame: BTreeMap<String, i32>,
    pub level: u32,
    /// Learning points (+10 per level, spent on teachers).
    pub learning_points: u32,
    pub skills: BTreeSet<String>,
    /// World clock in hours (`(year*365+day)*24+hour`), advanced by the host / `Sleep`.
    pub hours: u64,
    /// `QuestRunning` infos: hour of their last run (GAME_RULES 1.2.i).
    pub info_last_run: BTreeMap<String, u64>,
    /// Player guild / political alignment placeholder.
    pub guild: String,
    /// Attitude of NPCs to the player (`gEAttitude` raw value) - placeholder, never read yet.
    pub attitudes: BTreeMap<String, u32>,
    pub party: BTreeSet<String>,
    /// Sector status switched by `SetSectorStatus`.
    pub sectors: BTreeMap<String, bool>,
    pub chapter: u32,
    pub log: Vec<LogEntry>,
    /// Entities the dialog host reports as "owner is near X" (`CondOwnerNearEntity`).
    pub near: BTreeSet<String>,
    pub game_over: bool,
}

impl Default for GameState {
    fn default() -> Self {
        Self::new()
    }
}

impl GameState {
    pub fn new() -> Self {
        Self {
            version: SAVE_VERSION,
            quests: BTreeMap::new(),
            events: BTreeSet::new(),
            told: BTreeSet::new(),
            npcs: BTreeMap::new(),
            inventory: BTreeMap::new(),
            xp: 0,
            attributes: BTreeMap::new(),
            fame: BTreeMap::new(),
            level: 0,
            learning_points: 0,
            skills: BTreeSet::new(),
            hours: 0,
            info_last_run: BTreeMap::new(),
            guild: String::new(),
            attitudes: BTreeMap::new(),
            party: BTreeSet::new(),
            sectors: BTreeMap::new(),
            chapter: 0,
            log: Vec::new(),
            near: BTreeSet::new(),
            game_over: false,
        }
    }

    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    pub fn from_json(text: &str) -> Result<Self> {
        let s: Self = serde_json::from_str(text)?;
        if s.version > SAVE_VERSION {
            bail!(
                "save version {} is newer than supported {SAVE_VERSION}",
                s.version
            );
        }
        // Older versions would be migrated here.
        Ok(s)
    }

    // ---- quests -------------------------------------------------------------------
    pub fn quest_status(&self, quest: &str) -> QuestStatus {
        self.quests
            .get(&norm(quest))
            .and_then(|q| q.status)
            .unwrap_or(QuestStatus::Open)
    }

    /// Set the status, recording a log entry. Returns the previous status when it changed.
    pub fn set_quest_status(&mut self, quest: &str, to: QuestStatus) -> Option<QuestStatus> {
        let from = self.quest_status(quest);
        if from == to {
            return None;
        }
        let key = norm(quest);
        self.quests.entry(key.clone()).or_default().status = Some(to);
        self.log.push(LogEntry {
            quest: key,
            kind: LogKind::Status(to),
        });
        Some(from)
    }

    pub fn quest_progress(&self, quest: &str, entity: &str) -> u32 {
        self.quests
            .get(&norm(quest))
            .and_then(|q| q.progress.get(&norm(entity)))
            .copied()
            .unwrap_or(0)
    }

    /// `IsOvertime` (GAME_RULES 1.4): running longer than the quest's time limit
    /// (`limit_hours`, 0 = unlimited) since activation.
    pub fn quest_overtime(&self, quest: &str, limit_hours: u64) -> bool {
        if limit_hours == 0 {
            return false;
        }
        match self
            .quests
            .get(&norm(quest))
            .and_then(|q| q.activation_hours)
        {
            Some(t) if self.quest_status(quest) != QuestStatus::Open => {
                self.hours.saturating_sub(t) > limit_hours
            }
            _ => false,
        }
    }

    // ---- events -------------------------------------------------------------------
    pub fn knows(&self, event: &str) -> bool {
        self.events.contains(&norm(event))
    }
    pub fn set_event(&mut self, event: &str) -> bool {
        self.events.insert(norm(event))
    }
    pub fn clear_event(&mut self, event: &str) -> bool {
        self.events.remove(&norm(event))
    }

    // ---- inventory ----------------------------------------------------------------
    pub fn item_count(&self, item: &str) -> u32 {
        self.inventory.get(&norm(item)).copied().unwrap_or(0)
    }
    pub fn gold(&self) -> u32 {
        self.item_count(GOLD_ITEM)
    }
    pub fn add_item(&mut self, item: &str, n: u32) {
        if n > 0 {
            *self.inventory.entry(norm(item)).or_insert(0) += n;
        }
    }
    /// Remove `n` items; returns false (and changes nothing) when not enough are held.
    pub fn take_item(&mut self, item: &str, n: u32) -> bool {
        let k = norm(item);
        let have = self.inventory.get(&k).copied().unwrap_or(0);
        if have < n {
            return false;
        }
        if have == n {
            self.inventory.remove(&k);
        } else {
            self.inventory.insert(k, have - n);
        }
        true
    }

    // ---- npcs ---------------------------------------------------------------------
    pub fn npc(&self, name: &str) -> Option<&NpcRuntime> {
        self.npcs.get(&norm(name))
    }
    pub fn npc_mut(&mut self, name: &str) -> &mut NpcRuntime {
        self.npcs.entry(norm(name)).or_default()
    }
    /// Does NPC `name` currently satisfy `gEInfoNPCStatus` `s` (GAME_RULES 1.3 e)?
    pub fn npc_has_status(&self, name: &str, s: NpcStatus) -> bool {
        let n = self.npc(name).cloned().unwrap_or_default();
        match s {
            NpcStatus::Alive => !n.dead,
            NpcStatus::AliveNotDefeated => !n.dead && !n.defeated,
            NpcStatus::Defeated => n.defeated,
            NpcStatus::Dead => n.dead,
            NpcStatus::TalkedToPlayer => n.talked,
            NpcStatus::NotTalkedToPlayer => !n.talked,
        }
    }

    // ---- progression --------------------------------------------------------------
    /// `XPNext(L) = 250*(L+1)*(L+2)` (GAME_RULES 4.1).
    pub fn xp_next(level: u32) -> u64 {
        250 * (u64::from(level) + 1) * (u64::from(level) + 2)
    }

    /// Add raw XP and level up (checked twice per call like the original, so at most two
    /// levels; +10 learning points each, +1 with the `Perk_Learn` skill). Returns
    /// (level before, level after).
    pub fn add_xp(&mut self, amount: u64) -> (u32, u32) {
        let before = self.level;
        self.xp += amount;
        for _ in 0..2 {
            if self.xp >= Self::xp_next(self.level) {
                self.level += 1;
                self.learning_points += 10 + u32::from(self.skills.contains("perk_learn"));
            }
        }
        (before, self.level)
    }

    pub fn add_fame(&mut self, key: &str, delta: i32) {
        let v = self.fame.entry(norm(key)).or_insert(0);
        *v = (*v + delta).clamp(0, 100);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trip_is_deterministic() {
        let mut s = GameState::new();
        s.set_event("Foo_Bar");
        s.add_item("It_Gold", 50);
        s.set_quest_status("Q", QuestStatus::Running);
        s.add_xp(600);
        s.npc_mut("Kerth").talked = true;
        let a = s.to_json().unwrap();
        let back = GameState::from_json(&a).unwrap();
        assert_eq!(back, s);
        assert_eq!(back.to_json().unwrap(), a);
        assert!(back.knows("foo_bar"));
        assert_eq!(back.gold(), 50);
    }

    #[test]
    fn rejects_newer_save_and_garbage() {
        let mut s = GameState::new();
        s.version = SAVE_VERSION + 1;
        assert!(GameState::from_json(&s.to_json().unwrap()).is_err());
        assert!(GameState::from_json("{").is_err());
    }

    #[test]
    fn take_item_is_atomic() {
        let mut s = GameState::new();
        s.add_item("a", 2);
        assert!(!s.take_item("a", 3));
        assert_eq!(s.item_count("a"), 2);
        assert!(s.take_item("A", 2));
        assert_eq!(s.item_count("a"), 0);
    }

    #[test]
    fn level_curve_matches_original_formula() {
        let mut s = GameState::new();
        assert_eq!(GameState::xp_next(0), 500);
        assert_eq!(s.add_xp(499), (0, 0));
        assert_eq!(s.add_xp(1), (0, 1));
        assert_eq!(s.learning_points, 10);
        // 250*2*3 = 1500 needed for level 2; a huge grant gives at most two levels per call.
        s.add_xp(1_000_000);
        assert_eq!(s.level, 3);
    }
}
