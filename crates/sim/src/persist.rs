//! What a save game keeps of a running simulation, and how a load puts it back.
//!
//! The original persists a *delta* against the authored world: `gCSession` owns the world time
//! and the game events, `gCSectorPersistence_PS` remembers what happened to the entities of a
//! sector (killed, removed, moved), `eCEntityDynamicContext` the dynamic state of an entity,
//! `eCTrigger_PS` / interact objects their switch state, and `gCInventory_PS`, `gCStats*` and
//! `gCNavigation_PS` the per-character streams. Here the authored world is whatever the host
//! registered at start-up, so the snapshot stores the mutable half of every record and
//! [`Sim::apply_snapshot`] writes it over the freshly registered world.
//!
//! The snapshot is plain serde data; the file container (versions, migrations, compression,
//! slots) lives in `gothic-runtime`.

use crate::actor::Actor;
use crate::brain::{Brain, Task};
use crate::item::{Equipment, ItemDef};
use crate::rng::Rng;
use crate::routine::GameClock;
use crate::script_player::ObjectData;
use crate::sim::Sim;
use crate::types::{ActorId, AiMode, Alignment, Attitude, NpcType};
use crate::world::{Pos, WorldQuery};
use gothic_gameplay::GameState;
use gothic_gameplay::inventory::Inventory;
use gothic_script::player::api::{Lock, LockStatus};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Mutable half of one character (`gCNavigation_PS`, `gCInventory_PS`, `gCStats` streams and
/// the entity's dynamic context).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActorRecord {
    pub id: u32,
    pub name: String,
    pub pos: [f32; 3],
    pub yaw: f32,
    pub hp: i32,
    pub stamina: i32,
    pub mana: i32,
    pub ai_mode: AiMode,
    pub task: Task,
    pub since_ms: u64,
    pub unconscious_secs: u32,
    pub alignment: Alignment,
    pub npc_type: NpcType,
    pub enclave: Option<String>,
    pub attitude: Attitude,
    pub level: i32,
    pub equipment: EquipmentRecord,
    pub inventory: Inventory,
    pub perks: BTreeSet<String>,
    pub immortal: bool,
    pub boss: bool,
    pub summoned: bool,
    pub transformed: bool,
    pub defeated_by_player: bool,
    pub party_leader: Option<u32>,
    pub drawn: bool,
    pub routine_name: String,
    pub status: u32,
}

/// Worn items (the original links them to inventory slots; the simulation keeps the defs).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct EquipmentRecord {
    pub left: Option<ItemDef>,
    pub right: Option<ItemDef>,
    pub body: Option<ItemDef>,
    pub accessories: Vec<ItemDef>,
}

/// State of an interact object (chest, door, ore vein, bed) - the sector-persistence entry of
/// opened / unlocked / used-up objects and their remaining contents.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObjectRecord {
    pub open: bool,
    pub used: bool,
    pub lock: Option<LockRecord>,
    pub loot: Vec<(String, u32)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockRecord {
    pub locked: bool,
    pub key: String,
    pub key_amount: u32,
    pub difficulty: u32,
}

/// A loose item: picked up (`dropped`), or dropped by somebody and lying elsewhere.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemRecord {
    pub name: String,
    pub dropped: bool,
    pub pos: [f32; 3],
}

/// Everything the simulation contributes to a save game.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimSnapshot {
    pub tick: u64,
    pub rng: u64,
    /// World time in game milliseconds (`gCSession` clock).
    pub game_ms: u64,
    pub chapter: u32,
    pub player_alignment: Alignment,
    /// Quests, infos, game events, hero progression, NPC runtime flags.
    pub state: GameState,
    pub actors: Vec<ActorRecord>,
    pub objects: BTreeMap<u32, ObjectRecord>,
    pub items: Vec<ItemRecord>,
    pub counters: [u32; 2],
    pub books: [i32; 3],
}

/// What [`Sim::apply_snapshot`] could and could not put back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RestoreReport {
    pub actors: u32,
    /// Records whose actor is missing or has another name (the world was registered differently).
    pub actors_skipped: u32,
    pub objects: u32,
    pub objects_skipped: u32,
    pub items: u32,
    /// Loose items created after start-up (dropped by characters) that were re-created.
    pub items_added: u32,
}

fn arr(p: Pos) -> [f32; 3] {
    [p.x, p.y, p.z]
}

fn pos(a: [f32; 3]) -> Pos {
    Pos::new(a[0], a[1], a[2])
}

impl<W: WorldQuery> Sim<W> {
    fn actor_record(&self, a: &Actor) -> ActorRecord {
        let p = self.world.position(a.id).unwrap_or_default();
        let e = &a.equipment;
        ActorRecord {
            id: a.id.0,
            name: a.name.clone(),
            pos: arr(p),
            yaw: self.world.facing(a.id),
            hp: a.hp,
            stamina: a.stamina,
            mana: a.mana,
            ai_mode: a.ai_mode,
            task: a.brain.task,
            since_ms: a.brain.since_ms,
            unconscious_secs: a.brain.unconscious_secs,
            alignment: a.alignment,
            npc_type: a.npc_type,
            enclave: a.enclave.clone(),
            attitude: a.base_attitude_to_player,
            level: a.level,
            equipment: EquipmentRecord {
                left: e.left.clone(),
                right: e.right.clone(),
                body: e.body.clone(),
                accessories: e.accessories.clone(),
            },
            inventory: a.inventory.clone(),
            perks: a.perks.clone(),
            immortal: a.immortal,
            boss: a.boss,
            summoned: a.summoned,
            transformed: a.transformed,
            defeated_by_player: a.defeated_by_player,
            party_leader: a.party_leader.map(|l| l.0),
            drawn: a.drawn,
            routine_name: a.routine_name.clone(),
            status: a.status,
        }
    }

    /// Port of the save half of `gCSession` / `gCSectorPersistence_PS` / the per-character
    /// streams: capture the persistent state of the simulation.
    pub fn snapshot(&self) -> SimSnapshot {
        let player = &self.script.player;
        SimSnapshot {
            tick: self.tick,
            rng: self.rng.state(),
            game_ms: self.clock.game_ms,
            chapter: self.chapter,
            player_alignment: self.player_alignment,
            state: self.state.clone(),
            actors: self.actors.iter().map(|a| self.actor_record(a)).collect(),
            objects: player
                .objects
                .iter()
                .map(|(id, o)| {
                    let rec = ObjectRecord {
                        open: o.open,
                        used: o.used,
                        lock: o.lock.as_ref().map(|l| LockRecord {
                            locked: l.status == LockStatus::Locked,
                            key: l.key.clone(),
                            key_amount: l.key_amount,
                            difficulty: l.difficulty,
                        }),
                        loot: o.loot.clone(),
                    };
                    (*id, rec)
                })
                .collect(),
            items: self
                .script
                .items
                .iter()
                .map(|i| ItemRecord {
                    name: i.name.clone(),
                    dropped: i.dropped,
                    pos: arr(i.pos),
                })
                .collect(),
            counters: player.counters,
            books: player.books,
        }
    }

    /// Port of the load half (`gCWorld::LoadGameWorld` -> sector persistence -> per-entity
    /// `ReadSaveGame`): write a snapshot over the registered world. Transient AI state
    /// (targets, strikes, paths, queued commands) is reset; characters resume their routine
    /// or the persistent tasks (dead, unconscious, following, waiting).
    pub fn apply_snapshot(&mut self, snap: &SimSnapshot) -> RestoreReport {
        let mut rep = RestoreReport::default();
        self.tick = snap.tick;
        self.rng = Rng::new(snap.rng);
        self.clock = GameClock {
            game_ms: snap.game_ms,
        };
        self.chapter = snap.chapter;
        self.player_alignment = snap.player_alignment;
        self.state = snap.state.clone();
        self.dialog_partner = None;
        self.gui_open = false;
        self.events.clear();
        self.script.commands.clear();

        for r in &snap.actors {
            let ok = self
                .actors
                .get(r.id as usize)
                .is_some_and(|a| a.name.eq_ignore_ascii_case(&r.name));
            if !ok {
                rep.actors_skipped += 1;
                continue;
            }
            rep.actors += 1;
            let id = ActorId(r.id);
            self.world.teleport(id, pos(r.pos));
            let look = Pos::new(
                r.pos[0] + r.yaw.cos() * 100.0,
                r.pos[1],
                r.pos[2] + r.yaw.sin() * 100.0,
            );
            self.world.face_towards(id, look);
            let items = &self.items;
            let a = &mut self.actors[r.id as usize];
            a.hp = r.hp;
            a.stamina = r.stamina;
            a.mana = r.mana;
            a.ai_mode = r.ai_mode;
            a.alignment = r.alignment;
            a.npc_type = r.npc_type;
            a.enclave = r.enclave.clone();
            a.base_attitude_to_player = r.attitude;
            a.level = r.level;
            a.equipment = Equipment {
                left: r.equipment.left.clone(),
                right: r.equipment.right.clone(),
                body: r.equipment.body.clone(),
                accessories: r.equipment.accessories.clone(),
            };
            a.inventory = r.inventory.clone();
            a.inventory.rebind(items);
            a.perks = r.perks.clone();
            a.immortal = r.immortal;
            a.boss = r.boss;
            a.summoned = r.summoned;
            a.transformed = r.transformed;
            a.defeated_by_player = r.defeated_by_player;
            a.party_leader = r.party_leader.map(ActorId);
            a.drawn = r.drawn;
            a.routine_name = r.routine_name.clone();
            a.status = r.status;
            a.target = None;
            a.current_attacker = None;
            a.last_attacker = None;
            a.combat_state_active = false;
            // Only tasks that outlive a load come back; fights restart from the routine.
            let task = match r.task {
                Task::Dead | Task::Unconscious | Task::FollowPlayer | Task::WaitForPlayer => r.task,
                _ => Task::Routine,
            };
            a.brain = Brain {
                task,
                since_ms: r.since_ms,
                unconscious_secs: r.unconscious_secs,
                ..Brain::default()
            };
            if a.ai_mode != AiMode::Dead && a.ai_mode != AiMode::Down {
                a.ai_mode = AiMode::Routine;
            }
            if task == Task::Dead {
                a.ai_mode = AiMode::Dead;
            }
        }

        let player = &mut self.script.player;
        for (id, rec) in &snap.objects {
            let Some(o) = player.objects.get_mut(id) else {
                rep.objects_skipped += 1;
                continue;
            };
            apply_object(o, rec);
            rep.objects += 1;
        }
        player.counters = snap.counters;
        player.books = snap.books;

        for (i, rec) in snap.items.iter().enumerate() {
            match self.script.items.get_mut(i) {
                Some(it) if it.name.eq_ignore_ascii_case(&rec.name) => {
                    it.dropped = rec.dropped;
                    it.pos = pos(rec.pos);
                    rep.items += 1;
                }
                Some(_) => {}
                None => {
                    self.script.add_item(&rec.name, pos(rec.pos));
                    if let Some(it) = self.script.items.last_mut() {
                        it.dropped = rec.dropped;
                    }
                    rep.items_added += 1;
                }
            }
        }
        rep
    }
}

impl crate::script_host::ScriptTables {
    /// Whether the loose item entity `entity` (raw script id) was picked up; `None` for ids that
    /// are no loose item. The host uses it to show or hide item meshes after a load.
    pub fn loose_item_taken(&self, entity: u32) -> Option<bool> {
        let i = crate::script_host::table_index(
            gothic_script::host::EntityId(entity),
            crate::script_host::KIND_ITEM,
        )?;
        self.items.get(i).map(|it| it.dropped)
    }
}

fn apply_object(o: &mut ObjectData, rec: &ObjectRecord) {
    o.open = rec.open;
    o.used = rec.used;
    o.loot = rec.loot.clone();
    o.lock = rec.lock.as_ref().map(|l| Lock {
        status: if l.locked {
            LockStatus::Locked
        } else {
            LockStatus::Unlocked
        },
        key: l.key.clone(),
        key_amount: l.key_amount,
        difficulty: l.difficulty,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SimConfig;
    use crate::item::ItemDb;
    use crate::types::Species;
    use crate::world::Grid2dWorld;
    use gothic_script::enums::UseType;

    /// The world a host registers at start-up (identical for the saving and the loading run).
    fn world() -> Sim<Grid2dWorld> {
        let mut sim = Sim::new(SimConfig::default(), Grid2dWorld::new(), ItemDb::default());
        let hero = sim.spawn_hero(Pos::flat(0.0, 0.0));
        assert_eq!(hero, ActorId(0));
        let orc = Actor::npc(ActorId(0), "Orc_A", Species::Orc, 10, 10);
        sim.spawn(orc, Pos::flat(500.0, 0.0));
        let guard = Actor::npc(ActorId(0), "Guard_A", Species::Human, 5, 5);
        sim.spawn(guard, Pos::flat(-300.0, 40.0));
        sim.script.add_object(
            UseType::Chest,
            Pos::flat(100.0, 100.0),
            ObjectData {
                name: "Chest".into(),
                lock: Some(Lock {
                    status: LockStatus::Locked,
                    key: "It_Key".into(),
                    key_amount: 1,
                    difficulty: 2,
                }),
                loot: vec![("It_Gold".into(), 50)],
                ..Default::default()
            },
        );
        sim.script.add_item("It_Apple", Pos::flat(10.0, 10.0));
        sim
    }

    #[test]
    fn scripted_scenario_round_trips() {
        let mut a = world();
        // Pick up the apple, open the chest and take its gold, kill the orc, advance a quest,
        // move the guard, pass some time.
        a.script.items[0].dropped = true;
        a.actors[0].inventory.insert("It_Apple", 1);
        a.actors[0].inventory.insert("It_Gold", 50);
        let chest = a.script.player.objects.values_mut().next().unwrap();
        chest.open = true;
        chest.lock.as_mut().unwrap().status = LockStatus::Unlocked;
        chest.loot.clear();
        a.actors[1].hp = 0;
        a.actors[1].ai_mode = AiMode::Dead;
        a.actors[1].brain.task = Task::Dead;
        a.world.teleport(ActorId(2), Pos::flat(-120.0, 75.0));
        a.state.events.insert("Orc_Killed".into());
        a.state.told.insert("Info_Guard_Hello".into());
        a.clock.game_ms += 7 * 3_600_000 + 1234;
        a.tick = 4242;
        a.rng.next_u64();
        let saved = a.snapshot();

        let mut b = world();
        let rep = b.apply_snapshot(&saved);
        assert_eq!((rep.actors, rep.actors_skipped), (3, 0));
        assert_eq!((rep.objects, rep.items), (1, 1));
        assert_eq!(b.snapshot(), saved);
        assert!(b.script.items[0].dropped);
        assert_eq!(b.actors[1].ai_mode, AiMode::Dead);
        assert_eq!(b.rng.state(), a.rng.state());
        assert_eq!(b.world.position(ActorId(2)).unwrap().x, -120.0);
    }

    #[test]
    fn mismatching_world_is_reported_not_applied() {
        let mut a = world();
        let mut snap = a.snapshot();
        snap.actors[2].name = "Somebody_Else".into();
        snap.objects
            .insert(99, snap.objects.values().next().unwrap().clone());
        let rep = a.apply_snapshot(&snap);
        assert_eq!(rep.actors_skipped, 1);
        assert_eq!(rep.objects_skipped, 1);
    }
}
