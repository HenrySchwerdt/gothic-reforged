//! A headless stand-in for the game core: enough world (hero, actors, quests, inventory) to
//! drive script mods in tests and tools without Bevy or game data.
use crate::vocab::{ActorInfo, Issued, Line, ScriptCommand, ScriptEvent, Vec3, WorldView};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default)]
pub struct StubCore {
    pub world: WorldView,
    /// Template -> (species, guild) for spawned NPCs.
    pub templates: BTreeMap<String, (String, String)>,
    pub inventory: BTreeMap<String, u32>,
    pub notices: Vec<String>,
    /// Every command applied, in order.
    pub applied: Vec<Issued>,
    next_id: u32,
    pending: Vec<ScriptEvent>,
}

impl StubCore {
    pub const DT: f32 = 1.0 / 30.0;

    pub fn new() -> Self {
        Self {
            next_id: 1,
            ..Self::default()
        }
    }

    pub fn template(&mut self, name: &str, species: &str, guild: &str) -> &mut Self {
        self.templates
            .insert(name.into(), (species.into(), guild.into()));
        self
    }

    /// Place an actor; `ActorSpawned` follows with the next tick.
    pub fn add_actor(&mut self, name: &str, species: &str, guild: &str, pos: Vec3) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.world.actors.insert(
            id,
            ActorInfo {
                id,
                name: name.into(),
                species: species.into(),
                guild: guild.into(),
                pos,
                alive: true,
            },
        );
        self.pending.push(ScriptEvent::ActorSpawned {
            actor: id,
            name: name.into(),
        });
        id
    }

    pub fn set_quest(&mut self, quest: &str, status: &str) {
        self.world.quests.insert(quest.into(), status.into());
        self.pending.push(ScriptEvent::QuestChanged {
            quest: quest.into(),
            status: status.into(),
        });
    }

    /// An actor (or the hero, `None`) says a line; returns it for the subtitle hook.
    pub fn say(&mut self, actor: Option<u32>, key: &str, text: &str) -> Line {
        let speaker = actor
            .and_then(|a| self.world.actors.get(&a))
            .map_or_else(|| "Du".to_string(), |a| a.name.clone());
        let line = Line {
            actor,
            speaker,
            key: key.into(),
            text: text.into(),
            player: actor.is_none(),
        };
        self.pending.push(ScriptEvent::LineSpoken(line.clone()));
        line
    }

    pub fn kill(&mut self, victim: u32, killer: Option<u32>) {
        if let Some(a) = self.world.actors.get_mut(&victim) {
            a.alive = false;
        }
        self.pending
            .push(ScriptEvent::ActorKilled { victim, killer });
    }

    /// Apply the commands, advance time by one tick and return the tick's events (`Tick`
    /// first).
    pub fn tick(&mut self, commands: &[Issued]) -> Vec<ScriptEvent> {
        for c in commands {
            self.apply(c);
        }
        self.world.hours += f64::from(Self::DT) / 3600.0;
        let mut out = vec![ScriptEvent::Tick { dt: Self::DT }];
        out.append(&mut self.pending);
        out
    }

    fn apply(&mut self, c: &Issued) {
        self.applied.push(c.clone());
        match &c.command {
            ScriptCommand::SpawnNpc { template, at, .. } => {
                let (species, guild) = self
                    .templates
                    .get(template)
                    .cloned()
                    .unwrap_or_else(|| ("Unknown".into(), String::new()));
                self.add_actor(template, &species, &guild, *at);
            }
            ScriptCommand::GiveHero { template, amount } => {
                *self.inventory.entry(template.clone()).or_default() += amount;
                self.pending.push(ScriptEvent::ItemMoved {
                    item: template.clone(),
                    amount: *amount,
                    gained: true,
                });
            }
            ScriptCommand::GiveNpc { .. } => {}
            ScriptCommand::Notice { text } => self.notices.push(text.clone()),
        }
    }

    /// Actors of a species.
    pub fn count_species(&self, species: &str) -> usize {
        self.world
            .actors
            .values()
            .filter(|a| a.species == species)
            .count()
    }
}
