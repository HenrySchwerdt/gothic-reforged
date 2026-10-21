//! The vocabulary scripts see: a stable, serializable mirror of the core's `Event`s and the
//! subset of `Command`s mods may issue. The host adapts the real core types to these (see the
//! `runtime` feature), so scripts do not break when the core's internals change.
use rhai::{Dynamic, Map};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Position in world centimetres.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    pub(crate) fn to_dynamic(self) -> Dynamic {
        let mut m = Map::new();
        m.insert("x".into(), (self.x as f64).into());
        m.insert("y".into(), (self.y as f64).into());
        m.insert("z".into(), (self.z as f64).into());
        m.into()
    }

    pub(crate) fn from_map(m: &Map) -> Option<Self> {
        let f = |k: &str| {
            let v = m.get(k)?;
            v.as_float()
                .ok()
                .or_else(|| v.as_int().ok().map(|i| i as f64))
        };
        Some(Self::new(f("x")? as f32, f("y")? as f32, f("z")? as f32))
    }
}

/// What a script may know about an actor.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ActorInfo {
    pub id: u32,
    /// Template / entity name (`Orc_Warrior`).
    pub name: String,
    /// Species (`Human`, `Orc`, `Chicken`, ...).
    pub species: String,
    pub guild: String,
    pub pos: Vec3,
    pub alive: bool,
}

impl ActorInfo {
    pub(crate) fn to_dynamic(&self) -> Dynamic {
        let mut m = Map::new();
        m.insert("id".into(), (self.id as i64).into());
        m.insert("name".into(), self.name.clone().into());
        m.insert("species".into(), self.species.clone().into());
        m.insert("guild".into(), self.guild.clone().into());
        m.insert("pos".into(), self.pos.to_dynamic());
        m.insert("alive".into(), self.alive.into());
        m.into()
    }
}

/// Read-only snapshot of the world the host keeps up to date between ticks. Scripts read it
/// through the `world` capability.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WorldView {
    pub hero: Vec3,
    pub actors: BTreeMap<u32, ActorInfo>,
    /// Quest name -> status (`Running`, `Success`, ...).
    pub quests: BTreeMap<String, String>,
    /// Game time in hours since the start.
    pub hours: f64,
}

/// A spoken line (subtitle) as the rewrite hook sees it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Line {
    /// The speaking actor (`None` for the hero).
    pub actor: Option<u32>,
    pub speaker: String,
    pub key: String,
    pub text: String,
    pub player: bool,
}

impl Line {
    pub(crate) fn to_dynamic(&self, world: &WorldView) -> Dynamic {
        let mut m = Map::new();
        m.insert(
            "actor".into(),
            self.actor.map_or(Dynamic::UNIT, |a| (a as i64).into()),
        );
        m.insert("speaker".into(), self.speaker.clone().into());
        m.insert("key".into(), self.key.clone().into());
        m.insert("text".into(), self.text.clone().into());
        m.insert("player".into(), self.player.into());
        let info = self.actor.and_then(|a| world.actors.get(&a));
        m.insert(
            "species".into(),
            info.map_or(String::new(), |i| i.species.clone()).into(),
        );
        m.insert(
            "guild".into(),
            info.map_or(String::new(), |i| i.guild.clone()).into(),
        );
        m.into()
    }
}

/// Events delivered to scripts. A mod handles `X` by defining `fn on_<x>(e)`, e.g.
/// `fn on_quest_changed(e) { if e.status == "Success" { ... } }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum ScriptEvent {
    /// Once per core tick, before the tick's other events.
    Tick {
        dt: f32,
    },
    QuestChanged {
        quest: String,
        status: String,
    },
    ActorSpawned {
        actor: u32,
        name: String,
    },
    ActorKilled {
        victim: u32,
        killer: Option<u32>,
    },
    LineSpoken(Line),
    DialogStarted {
        npc: u32,
    },
    DialogEnded {
        npc: Option<u32>,
    },
    ItemMoved {
        item: String,
        amount: u32,
        gained: bool,
    },
    XpGained {
        amount: u32,
        level_after: u32,
    },
}

impl ScriptEvent {
    /// Name of the handler function.
    pub fn handler(&self) -> &'static str {
        match self {
            Self::Tick { .. } => "on_tick",
            Self::QuestChanged { .. } => "on_quest_changed",
            Self::ActorSpawned { .. } => "on_actor_spawned",
            Self::ActorKilled { .. } => "on_actor_killed",
            Self::LineSpoken(_) => "on_line_spoken",
            Self::DialogStarted { .. } => "on_dialog_started",
            Self::DialogEnded { .. } => "on_dialog_ended",
            Self::ItemMoved { .. } => "on_item_moved",
            Self::XpGained { .. } => "on_xp_gained",
        }
    }

    pub(crate) fn to_dynamic(&self, world: &WorldView) -> Dynamic {
        fn id(v: Option<u32>) -> Dynamic {
            v.map_or(Dynamic::UNIT, |a| (a as i64).into())
        }
        let mut m = Map::new();
        match self {
            Self::Tick { dt } => {
                m.insert("dt".into(), (*dt as f64).into());
            }
            Self::QuestChanged { quest, status } => {
                m.insert("quest".into(), quest.clone().into());
                m.insert("status".into(), status.clone().into());
            }
            Self::ActorSpawned { actor, name } => {
                m.insert("actor".into(), (*actor as i64).into());
                m.insert("name".into(), name.clone().into());
            }
            Self::ActorKilled { victim, killer } => {
                m.insert("victim".into(), (*victim as i64).into());
                m.insert("killer".into(), id(*killer));
            }
            Self::LineSpoken(line) => return line.to_dynamic(world),
            Self::DialogStarted { npc } => {
                m.insert("npc".into(), (*npc as i64).into());
            }
            Self::DialogEnded { npc } => {
                m.insert("npc".into(), id(*npc));
            }
            Self::ItemMoved {
                item,
                amount,
                gained,
            } => {
                m.insert("item".into(), item.clone().into());
                m.insert("amount".into(), (*amount as i64).into());
                m.insert("gained".into(), (*gained).into());
            }
            Self::XpGained {
                amount,
                level_after,
            } => {
                m.insert("amount".into(), (*amount as i64).into());
                m.insert("level_after".into(), (*level_after as i64).into());
            }
        }
        m.into()
    }
}

/// What scripts may ask of the game. Everything else is out of reach.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum ScriptCommand {
    /// Spawn an NPC from a template (capability `spawn`).
    SpawnNpc {
        template: String,
        at: Vec3,
        yaw: f32,
    },
    /// Give the hero items (capability `items`).
    GiveHero { template: String, amount: u32 },
    /// Give an NPC items (capability `items`).
    GiveNpc {
        actor: u32,
        template: String,
        amount: u32,
    },
    /// Show a message on screen (capability `ui`).
    Notice { text: String },
}

impl ScriptCommand {
    /// The command with the issuing mod.
    pub fn from_mod(self, id: &str) -> Issued {
        Issued {
            by: id.to_string(),
            command: self,
        }
    }
}

/// A command and the mod that issued it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Issued {
    pub by: String,
    pub command: ScriptCommand,
}
