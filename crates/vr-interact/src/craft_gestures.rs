//! Physical gestures that *time* a core command without deciding its result.
//!
//! * [`Hammer`]: smithing at the anvil. Swing the hammer hand down onto the anvil face; each strike
//!   counts, and after `strikes` of them the recipe is made (`Command::Craft`). The core's own
//!   rules (ingredients, skill) decide whether it works; the gesture only replaces the
//!   "Herstellen" button press by the work the smith does.
//! * [`Pickpocket`]: reach into an NPC's pouch from behind while crouched. Hold the grip with the
//!   hand in the pouch zone and keep it still; a trembling hand or a turning NPC makes the
//!   attempt fail. The outcome is the *attempt* (the core's `PickPocket` info decides what
//!   the NPC does, see `GAME_RULES.md` 1.3).
use glam::{Quat, Vec3};
use gothic_runtime::Command;

/// Hammering at an anvil.
#[derive(Debug, Clone, PartialEq)]
pub struct Hammer {
    /// Top of the anvil face, world metres, and its half extents on X/Z.
    pub anvil_top: Vec3,
    pub half: [f32; 2],
    /// Strikes needed (the recipe's work; the host chooses, e.g. 5).
    pub strikes: u32,
    pub recipe: i32,
    done: u32,
    prev: Option<Vec3>,
    /// The hand must lift above this before the next strike counts.
    armed: bool,
}

/// Downward speed that makes a blow, m/s.
pub const STRIKE_SPEED: f32 = 1.2;
/// Lift needed between blows, m.
pub const REARM_HEIGHT: f32 = 0.10;

#[derive(Debug, Clone, PartialEq)]
pub enum HammerStep {
    Idle,
    /// A strike landed (play the clang, the count so far).
    Strike(u32),
    /// The work is done: send the command.
    Finished(i32),
}

impl Hammer {
    pub fn new(anvil_top: Vec3, half: [f32; 2], strikes: u32, recipe: i32) -> Self {
        Self {
            anvil_top,
            half,
            strikes,
            recipe,
            done: 0,
            prev: None,
            armed: true,
        }
    }

    pub fn done(&self) -> u32 {
        self.done
    }

    /// `head` = the hammer head's world position this frame.
    pub fn step(&mut self, head: Vec3, dt: f32) -> HammerStep {
        let prev = self.prev.replace(head);
        let rel = head - self.anvil_top;
        if rel.y > REARM_HEIGHT {
            self.armed = true;
        }
        let Some(prev) = prev else {
            return HammerStep::Idle;
        };
        let vy = (head.y - prev.y) / dt.max(1e-4);
        let over = rel.x.abs() <= self.half[0] && rel.z.abs() <= self.half[1];
        let crossed = prev.y > self.anvil_top.y && head.y <= self.anvil_top.y + 0.02;
        if self.armed && over && crossed && -vy >= STRIKE_SPEED {
            self.armed = false;
            self.done += 1;
            if self.done >= self.strikes {
                return HammerStep::Finished(self.recipe);
            }
            return HammerStep::Strike(self.done);
        }
        HammerStep::Idle
    }

    pub fn command(recipe: i32) -> Command {
        Command::Craft { recipe }
    }
}

/// Reaching into a pouch.
#[derive(Debug, Clone, PartialEq)]
pub struct Pickpocket {
    /// The NPC's pouch, world metres, and the NPC's facing (`yaw`, radians; forward = -Z at 0).
    pub pouch: Vec3,
    pub npc_yaw: f32,
    /// How long the hand must stay in the pouch, s.
    pub hold: f32,
    t: f32,
    prev: Option<Vec3>,
    /// Accumulated tremble (m of unwanted motion); above `tolerance` the NPC notices.
    shake: f32,
}

/// Distance of the hand from the pouch that counts as "in", m.
pub const POUCH_RADIUS: f32 = 0.09;
/// Total sideways hand motion tolerated during the hold, m.
pub const TOLERANCE: f32 = 0.12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PocketStep {
    /// Not in the pouch (or the grip is open): nothing happens.
    Waiting,
    /// In the pouch, hold still; `0..=1` progress.
    Reaching(u8),
    /// The hand stole: the host starts the core's pickpocket info.
    Success,
    /// The NPC noticed (shaky hand, grip dropped inside, or the NPC turned round).
    Caught,
}

impl Pickpocket {
    pub fn new(pouch: Vec3, npc_yaw: f32, hold: f32) -> Self {
        Self {
            pouch,
            npc_yaw,
            hold,
            t: 0.0,
            prev: None,
            shake: 0.0,
        }
    }

    /// Whether the hero stands behind the NPC (within 70 degrees of straight behind).
    pub fn behind(&self, hero: Vec3, npc_pos: Vec3) -> bool {
        let fwd = Quat::from_rotation_y(self.npc_yaw) * Vec3::NEG_Z;
        let to_hero = Vec3::new(hero.x - npc_pos.x, 0.0, hero.z - npc_pos.z).normalize_or(Vec3::Z);
        fwd.dot(to_hero) < -(70f32.to_radians().cos())
    }

    pub fn step(&mut self, grip: bool, hand: Vec3, npc_yaw_now: f32, dt: f32) -> PocketStep {
        let inside = hand.distance(self.pouch) <= POUCH_RADIUS;
        if !inside {
            let was_in = self.t > 0.0;
            self.reset();
            return if was_in && grip { PocketStep::Caught } else { PocketStep::Waiting };
        }
        if !grip {
            let was_in = self.t > 0.0;
            self.reset();
            return if was_in { PocketStep::Caught } else { PocketStep::Waiting };
        }
        // The NPC turning round ends it.
        if (npc_yaw_now - self.npc_yaw).abs() > 35f32.to_radians() {
            self.reset();
            return PocketStep::Caught;
        }
        if let Some(p) = self.prev.replace(hand) {
            self.shake += (hand - p).length();
        }
        self.t += dt;
        if self.shake > TOLERANCE + self.hold * 0.05 {
            self.reset();
            return PocketStep::Caught;
        }
        if self.t >= self.hold {
            self.reset();
            return PocketStep::Success;
        }
        PocketStep::Reaching(((self.t / self.hold) * 100.0) as u8)
    }

    /// 0..1 how close the NPC is to noticing (shown on the watch).
    pub fn noise(&self) -> f32 {
        (self.shake / (TOLERANCE + self.hold * 0.05)).clamp(0.0, 1.0)
    }

    fn reset(&mut self) {
        self.t = 0.0;
        self.prev = None;
        self.shake = 0.0;
    }
}
