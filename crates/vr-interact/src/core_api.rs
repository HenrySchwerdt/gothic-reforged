//! The Command/Event surface VR talks to, implemented by the real core and by a stub.
use gothic_runtime::{Command, Core, Event};
use gothic_sim::script::host::EntityId;
use gothic_sim::script::player::normal::Confirm;
use gothic_sim::types::ActorId;
use std::collections::BTreeMap;

/// Commands in, events out (`Core::tick`).
pub trait CoreApi {
    fn tick(&mut self, dt: f32, commands: &[Command]) -> Vec<Event>;
}

impl CoreApi for Core {
    fn tick(&mut self, dt: f32, commands: &[Command]) -> Vec<Event> {
        Core::tick(self, dt, commands)
    }
}

/// What a stub entity is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StubEntity {
    Npc,
    Body { loot: Vec<String> },
    Item { template: String },
    Object { name: String, locked: bool },
}

/// A tiny stand-in for the core: answers commands with plausible events so demos and tests
/// can run without game data. It does not model the original rules (reach, skills, mana).
#[derive(Debug, Default, Clone)]
pub struct StubCore {
    /// Raw script entity id -> entity (actors: `actor + 1`).
    pub entities: BTreeMap<u32, StubEntity>,
    pub inventory: Vec<String>,
    pub spell: Option<String>,
    pub sneaking: bool,
    /// Every command received, in order.
    pub log: Vec<Command>,
}

impl StubCore {
    pub fn add(&mut self, id: u32, e: StubEntity) -> &mut Self {
        self.entities.insert(id, e);
        self
    }

    fn moved(&mut self, item: &str, gained: bool) -> Event {
        if gained {
            self.inventory.push(item.to_string());
        } else if let Some(i) = self.inventory.iter().position(|x| x == item) {
            self.inventory.remove(i);
        }
        Event::ItemMoved {
            item: item.to_string(),
            amount: 1,
            gained,
        }
    }

    fn apply(&mut self, c: &Command, out: &mut Vec<Event>) {
        self.log.push(c.clone());
        let note = |s: String| Event::Notice(s);
        match c {
            Command::Confirm { focus } => {
                let e = EntityId(*focus);
                out.push(Event::Confirmed(match self.entities.get(focus) {
                    Some(StubEntity::Npc) => Confirm::Talk(e),
                    Some(StubEntity::Body { .. }) => Confirm::Loot(e),
                    Some(StubEntity::Item { .. }) => Confirm::PickUp(e),
                    Some(StubEntity::Object { .. }) => Confirm::Interact(e),
                    None => Confirm::Nothing,
                }));
            }
            Command::PickUp { item } => {
                if let Some(StubEntity::Item { template }) = self.entities.remove(item) {
                    out.push(self.moved(&template, true));
                }
            }
            Command::QuickLoot { body } => {
                if let Some(StubEntity::Body { loot }) = self.entities.get_mut(body) {
                    let loot = std::mem::take(loot);
                    for l in loot {
                        out.push(self.moved(&l, true));
                    }
                }
            }
            Command::UseInteractObject { object } => match self.entities.get(object) {
                Some(StubEntity::Object { name, locked: true }) => {
                    out.push(note(format!("{name} is locked")))
                }
                Some(StubEntity::Object { name, .. }) => out.push(note(format!("use {name}"))),
                _ => {}
            },
            Command::Lockpick { object, success } => {
                if let Some(StubEntity::Object { name, locked }) = self.entities.get_mut(object) {
                    // `None`: the script would roll on the skill; the stub lets it succeed.
                    if success.unwrap_or(true) {
                        *locked = false;
                        out.push(note(format!("{name} unlocked")));
                    } else {
                        out.push(note(format!("lockpick broke on {name}")));
                    }
                }
            }
            Command::Craft { recipe } => out.push(note(format!("crafted recipe {recipe}"))),
            Command::UseItem { template } => {
                if self.inventory.contains(template) {
                    out.push(self.moved(template, false));
                }
            }
            Command::SelectSpell { template } => {
                self.spell = Some(template.clone());
                out.push(note(format!("{template} in hand")));
            }
            Command::CastSpell { target, power } => {
                if let Some(s) = &self.spell {
                    out.push(Event::Projectile {
                        owner: ActorId(0),
                        target: *target,
                        template: if *power {
                            format!("{s} (power)")
                        } else {
                            s.clone()
                        },
                        spell: true,
                    });
                }
            }
            Command::AbortCast => out.push(note("cast aborted".into())),
            Command::ReadyRanged => out.push(note("bow up".into())),
            Command::AimRanged { aim } => {
                out.push(note(if *aim { "aiming" } else { "bow lowered" }.into()))
            }
            Command::ReleaseRanged { target } => out.push(Event::Projectile {
                owner: ActorId(0),
                target: *target,
                template: "ItAm_Arrow".into(),
                spell: false,
            }),
            Command::ToggleSneak => {
                self.sneaking = !self.sneaking;
                out.push(note(format!("sneaking {}", self.sneaking)));
            }
            Command::Jump => out.push(note("jump".into())),
            Command::Dodge { direction } => out.push(note(format!("dodge {direction:?}"))),
            _ => {}
        }
    }
}

impl CoreApi for StubCore {
    fn tick(&mut self, _dt: f32, commands: &[Command]) -> Vec<Event> {
        let mut out = Vec::new();
        for c in commands {
            self.apply(c, &mut out);
        }
        out
    }
}
