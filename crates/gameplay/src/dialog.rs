//! Dialog flow: list building, ordering and tree navigation, following
//! `docs/research/GAME_RULES.md` section 1.5 (`[R 1.5]`).
//!
//! Known deviations / inferences (`[I]`): the original has no built-in "leave" entry (the
//! data ships `End` infos), the CLI adds one at the root so a conversation can never trap
//! the player; auto-run chains are capped to avoid looping on permanent single entries.
use crate::conditions::{self, Assumptions, Unmet, owner_ok};
use crate::db::{GameDatabase, InfoId};
use crate::model::{Cmd, InfoCond, InfoType};
use crate::script::{self, ExecResult};
use crate::state::{GameState, norm};
use serde::Serialize;

/// Owner name whose infos are appended to every NPC's list [R 1.5].
pub const PLAYER_OWNER: &str = "PC_Hero";

/// An entry of the dialog menu.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Choice {
    /// `None` is the built-in "leave" entry.
    pub info: Option<InfoId>,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DialogSession {
    pub npc: String,
    /// Parent chain of the child menu being shown; empty = root menu.
    pub stack: Vec<InfoId>,
    pub ended: bool,
    list: Vec<InfoId>,
}

pub struct Engine<'a> {
    pub db: &'a GameDatabase,
    pub assume: Assumptions,
}

const AUTO_CHAIN_LIMIT: u8 = 6;

impl<'a> Engine<'a> {
    pub fn new(db: &'a GameDatabase) -> Self {
        Self {
            db,
            assume: Assumptions::PERMISSIVE,
        }
    }
    pub fn with(db: &'a GameDatabase, assume: Assumptions) -> Self {
        Self { db, assume }
    }

    /// Why `id` is not available for a dialog with `npc` (or `Ok`).
    pub fn check(&self, st: &GameState, id: InfoId, npc: &str) -> Result<(), Unmet> {
        conditions::evaluate(self.db, st, self.assume, id, npc)
    }

    pub fn is_available(&self, st: &GameState, id: InfoId, npc: &str) -> bool {
        self.check(st, id, npc).is_ok()
    }

    /// Apply an info's effects (no availability check).
    pub fn execute(&self, st: &mut GameState, id: InfoId, npc: &str) -> ExecResult {
        script::execute_info(self.db, st, id, npc)
    }

    fn sort_key(&self, id: InfoId) -> (u32, bool, u32) {
        let i = self.db.info(id);
        (
            i.condition_type,
            i.owner.eq_ignore_ascii_case(PLAYER_OWNER),
            i.sort_id,
        )
    }

    fn ty(&self, id: InfoId) -> InfoType {
        InfoType::from_raw(self.db.info(id).info_type)
    }

    fn cond(&self, id: InfoId) -> InfoCond {
        InfoCond::from_raw(self.db.info(id).condition_type)
    }

    /// The NPC's own infos plus the player's, restricted to roots (no `Parent`).
    fn root_candidates(&self, npc: &str) -> impl Iterator<Item = InfoId> + '_ {
        self.db
            .infos_of(npc)
            .iter()
            .chain(self.db.infos_of(PLAYER_OWNER))
            .copied()
            .filter(|&i| self.db.info(i).parent.is_empty())
    }

    /// `GetInfoList` for the dialog start [R 1.5]: Refuse (first), Important (first), all
    /// QuestRunning, all News, then Info/Parent sorted by (condition, NPC-owned first, SortID).
    pub fn root_list(&self, st: &GameState, npc: &str) -> Vec<InfoId> {
        let avail: Vec<InfoId> = self
            .root_candidates(npc)
            .filter(|&i| self.is_available(st, i, npc))
            .collect();
        let of = |t: InfoType| avail.iter().copied().filter(move |&i| self.ty(i) == t);
        let mut list: Vec<InfoId> = Vec::new();
        list.extend(of(InfoType::Refuse).next());
        list.extend(of(InfoType::Important).next());
        list.extend(of(InfoType::QuestRunning));
        list.extend(of(InfoType::News));
        let mut infos: Vec<InfoId> = avail
            .iter()
            .copied()
            .filter(|&i| matches!(self.ty(i), InfoType::Info | InfoType::Parent))
            .collect();
        infos.sort_by_key(|&i| self.sort_key(i));
        list.extend(infos);
        self.dedupe_special(list)
    }

    /// Keep only the first End/Trade/MasterThief/NothingToSay; drop NothingToSay unless the
    /// list holds nothing else but End/NothingToSay [R 1.5].
    fn dedupe_special(&self, list: Vec<InfoId>) -> Vec<InfoId> {
        let mut seen: Vec<InfoCond> = Vec::new();
        let mut out = Vec::new();
        for id in list {
            let c = self.cond(id);
            if matches!(
                c,
                InfoCond::End | InfoCond::Trade | InfoCond::MasterThief | InfoCond::NothingToSay
            ) {
                if seen.contains(&c) {
                    continue;
                }
                seen.push(c);
            }
            out.push(id);
        }
        let only_trivial = out
            .iter()
            .all(|&i| matches!(self.cond(i), InfoCond::End | InfoCond::NothingToSay));
        if !only_trivial {
            out.retain(|&i| self.cond(i) != InfoCond::NothingToSay);
        }
        out
    }

    /// `GetChildInfoList` [R 1.5].
    pub fn child_list(&self, st: &GameState, parent: InfoId, npc: &str) -> Vec<InfoId> {
        let mut v: Vec<InfoId> = self
            .db
            .children_of(&self.db.info(parent).name)
            .iter()
            .copied()
            .filter(|&c| owner_ok(self.db, c, npc))
            .filter(|&c| matches!(self.ty(c), InfoType::Info | InfoType::Parent))
            .filter(|&c| self.is_available(st, c, npc))
            .collect();
        v.sort_by_key(|&i| self.sort_key(i));
        v
    }

    /// Menu label: the `Description` text, else the player's first spoken line.
    pub fn label(&self, id: InfoId) -> String {
        let info = self.db.info(id);
        let pick = |cmd: Cmd, player_only: bool| {
            info.script
                .iter()
                .filter(|s| Cmd::parse(&s.command) == Some(cmd))
                .filter(|s| {
                    !player_only
                        || s.entity1.eq_ignore_ascii_case("player")
                        || s.entity1.eq_ignore_ascii_case("pc_hero")
                })
                .find_map(|s| self.db.text(&s.text))
        };
        pick(Cmd::Description, false)
            .or_else(|| pick(Cmd::Say, true))
            .or_else(|| pick(Cmd::Say, false))
            .unwrap_or_else(|| format!("<{}>", info.name))
    }

    /// Begin a conversation (`StartInfoManager` [R 1.5]).
    pub fn start(&self, st: &mut GameState, npc: &str) -> (DialogSession, ExecResult) {
        let mut s = DialogSession {
            npc: npc.to_string(),
            stack: Vec::new(),
            ended: false,
            list: Vec::new(),
        };
        st.npc_mut(npc).talked = true;
        let mut out = ExecResult::default();
        s.list = self.root_list(st, npc);
        self.decide(st, &mut s, &mut out, true, 0);
        (s, out)
    }

    /// Choose between ending, a menu, and auto-running the first entry.
    fn decide(
        &self,
        st: &mut GameState,
        s: &mut DialogSession,
        out: &mut ExecResult,
        first_open: bool,
        depth: u8,
    ) {
        let list = s.list.clone();
        let Some(&first) = list.first() else {
            s.ended = true;
            return;
        };
        if list.iter().all(|&i| self.cond(i) == InfoCond::End) && first_open {
            s.ended = true;
            return;
        }
        let first_is_info = matches!(self.ty(first), InfoType::Info | InfoType::Parent);
        if first_is_info && list.len() > 1 {
            return; // menu
        }
        if depth >= AUTO_CHAIN_LIMIT {
            return; // present as menu instead of looping
        }
        let r = self.execute(st, first, &s.npc.clone());
        self.after_info(st, s, first, r, out, depth + 1);
    }

    /// `OnInfoScriptFinished` [R 1.5].
    fn after_info(
        &self,
        st: &mut GameState,
        s: &mut DialogSession,
        id: InfoId,
        r: ExecResult,
        out: &mut ExecResult,
        depth: u8,
    ) {
        let ended = r.ended;
        let refused = r.refused.is_some();
        merge(out, r);
        let cond = self.cond(id);
        if refused {
            self.refresh(st, s);
            return;
        }
        if ended
            || matches!(
                cond,
                InfoCond::End | InfoCond::NothingToSay | InfoCond::Trade | InfoCond::MasterThief
            )
            || self.ty(id) == InfoType::Refuse
        {
            s.ended = true;
            return;
        }
        let npc = s.npc.clone();
        // ClearChildren: all sibling children are consumed.
        if self.db.info(id).clear_children && !self.db.info(id).parent.is_empty() {
            for &sib in self.db.children_of(&self.db.info(id).parent) {
                st.told.insert(norm(&self.db.info(sib).name));
            }
        }
        let children = self.child_list(st, id, &npc);
        if !children.is_empty() {
            s.stack.push(id);
            s.list = children;
            return;
        }
        if !s.stack.is_empty() && cond != InfoCond::Back {
            if cond == InfoCond::StepBack {
                s.stack.pop();
            }
            if let Some(&p) = s.stack.last() {
                let again = self.child_list(st, p, &npc);
                if !again.is_empty() {
                    s.list = again;
                    return;
                }
            }
        }
        // Back to the root list: only Info/Parent entries.
        s.stack.clear();
        s.list = self
            .root_list(st, &npc)
            .into_iter()
            .filter(|&i| matches!(self.ty(i), InfoType::Info | InfoType::Parent))
            .collect();
        match s.list.len() {
            0 => s.ended = true,
            1 if depth < AUTO_CHAIN_LIMIT => {
                let only = s.list[0];
                let res = self.execute(st, only, &npc);
                self.after_info(st, s, only, res, out, depth + 1);
            }
            _ => {}
        }
    }

    /// Recompute the current level's list from the live state.
    fn refresh(&self, st: &GameState, s: &mut DialogSession) {
        s.list = match s.stack.last() {
            Some(&p) => self.child_list(st, p, &s.npc),
            None => self
                .root_list(st, &s.npc)
                .into_iter()
                .filter(|&i| matches!(self.ty(i), InfoType::Info | InfoType::Parent))
                .collect(),
        };
    }

    /// Current menu. At the root a built-in leave entry is appended [I].
    pub fn menu(&self, st: &GameState, s: &DialogSession) -> Vec<Choice> {
        if s.ended {
            return Vec::new();
        }
        let mut out: Vec<Choice> = s
            .list
            .iter()
            .copied()
            .filter(|&i| self.is_available(st, i, &s.npc))
            .filter(|&i| matches!(self.ty(i), InfoType::Info | InfoType::Parent))
            .map(|i| Choice {
                info: Some(i),
                label: self.label(i),
            })
            .collect();
        if s.stack.is_empty() {
            out.push(Choice {
                info: None,
                label: "(Gespraech beenden)".into(),
            });
        }
        out
    }

    /// Pick a menu entry.
    pub fn choose(&self, st: &mut GameState, s: &mut DialogSession, c: &Choice) -> ExecResult {
        let mut out = ExecResult::default();
        let Some(id) = c.info else {
            s.ended = true;
            out.ended = true;
            return out;
        };
        let npc = s.npc.clone();
        let r = self.execute(st, id, &npc);
        self.after_info(st, s, id, r, &mut out, 0);
        out
    }
}

pub fn merge(into: &mut ExecResult, mut r: ExecResult) {
    into.outputs.append(&mut r.outputs);
    into.diagnostics.append(&mut r.diagnostics);
    into.ended |= r.ended;
    if r.refused.is_some() {
        into.refused = r.refused.take();
    }
}
