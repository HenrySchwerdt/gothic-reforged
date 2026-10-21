//! Bevy-free mapping of tracked poses and buttons onto core commands.
use crate::lockpick::{LockSession, LockStep};
use crate::targets::{Target, TargetKind, aimed_actor, grabbable, pointed};
use glam::Vec3;
use gothic_runtime::Command;
use gothic_sim::script::player::evade::Direction;
use gothic_vr::{Hand, Pose, VrButton, VrInput};
use serde::{Deserialize, Serialize};

/// Head and hands in world metres (the plugin applies the locomotion offset; tests and the
/// demo use tracking space directly).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tracked {
    pub head: Pose,
    pub hands: [Option<Pose>; 2],
}

impl Tracked {
    /// Tracking space as world space.
    pub fn from_input(input: &VrInput) -> Self {
        Self {
            head: input.head(),
            hands: Hand::BOTH.map(|h| input.hand_pose(h)),
        }
    }

    pub fn hand(&self, h: Hand) -> Option<Pose> {
        self.hands[h.index()]
    }
}

/// What the host tells the mapper about the hero (inventory and equipment).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct HeroState {
    pub lockpicks: u32,
    /// Spells on the quick slots (item templates), in ring order.
    pub spells: Vec<String>,
    /// A bow or crossbow is equipped.
    pub bow_equipped: bool,
    /// Consumables the physics hands hold (from a grab in `gothic-vr-physics`).
    pub held_items: [Option<String>; 2],
    /// A full-screen window is open: no world interaction.
    pub in_menu: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct InteractSettings {
    /// Hand that points, casts and draws the string.
    pub main_hand: Hand,
    /// Talk / use range of the pointer, m (the core checks its own reach too).
    pub point_range: f32,
    /// Aim tolerance of the pointer, radians.
    pub point_slack: f32,
    /// Grab reach beyond a target's radius, m.
    pub grab_reach: f32,
    pub aim_range: f32,
    pub aim_cone: f32,
    /// Crouch below / stand up above this fraction of the standing head height.
    pub crouch_enter: f32,
    pub crouch_exit: f32,
    /// Horizontal head speed that counts as a dodge step, m/s.
    pub dodge_speed: f32,
    pub dodge_cooldown: f32,
    /// Vertical head speed that counts as a jump, m/s (`None` = button only).
    pub jump_speed: Option<f32>,
    pub jump_cooldown: f32,
    /// Trigger held at least this long casts with power.
    pub power_cast_hold: f32,
    /// The string hand must be this close to the bow hand to nock, m.
    pub nock_reach: f32,
    /// Draw length (hand distance) that aims / lowers the bow again, m.
    pub aim_draw: f32,
    pub lower_draw: f32,
    /// Item within this distance of the mouth for `eat_time` seconds is used.
    pub eat_reach: f32,
    pub eat_time: f32,
    /// Lockpick: wrist roll for a turn and back to neutral, radians; hand leash, m.
    pub lock_turn: f32,
    pub lock_neutral: f32,
    pub lock_leash: f32,
}

impl Default for InteractSettings {
    fn default() -> Self {
        Self {
            main_hand: Hand::Right,
            point_range: 6.0,
            point_slack: 4f32.to_radians(),
            grab_reach: 0.12,
            aim_range: 40.0,
            aim_cone: 6f32.to_radians(),
            crouch_enter: 0.72,
            crouch_exit: 0.82,
            dodge_speed: 2.0,
            dodge_cooldown: 0.6,
            jump_speed: Some(1.6),
            jump_cooldown: 0.9,
            power_cast_hold: 0.8,
            nock_reach: 0.15,
            aim_draw: 0.35,
            lower_draw: 0.25,
            eat_reach: 0.14,
            eat_time: 0.4,
            lock_turn: 35f32.to_radians(),
            lock_neutral: 12f32.to_radians(),
            lock_leash: 0.25,
        }
    }
}

/// The bow's state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BowState {
    #[default]
    Idle,
    /// Held up (`ReadyRanged` sent).
    Ready,
    /// The string hand holds the string.
    Nocked,
    /// Drawn past `aim_draw` (`AimRanged { aim: true }` sent).
    Aiming,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InteractMapper {
    pub settings: InteractSettings,
    /// What the mapper believes the sneak toggle is (the host corrects it from events).
    pub sneaking: bool,
    /// Calibrated standing head height, m (raised by the highest head seen).
    pub standing_height: Option<f32>,
    pub bow: BowState,
    /// Spell in the casting hand (after `SelectSpell`).
    pub spell: Option<String>,
    /// Ring slot under the hand while the off hand's secondary button is held.
    pub selecting: Option<usize>,
    lock: Option<(Hand, LockSession)>,
    cast_held: Option<f32>,
    eat: [f32; 2],
    last_head: Option<Vec3>,
    dodge_cd: f32,
    jump_cd: f32,
}

impl Default for InteractMapper {
    fn default() -> Self {
        Self::new(InteractSettings::default())
    }
}

impl InteractMapper {
    pub fn new(settings: InteractSettings) -> Self {
        Self {
            settings,
            sneaking: false,
            standing_height: None,
            bow: BowState::Idle,
            spell: None,
            selecting: None,
            lock: None,
            cast_held: None,
            eat: [0.0; 2],
            last_head: None,
            dodge_cd: 0.0,
            jump_cd: 0.0,
        }
    }

    /// The lock being played and the correct turns so far.
    pub fn lock_progress(&self) -> Option<(u32, usize)> {
        self.lock.as_ref().map(|(_, s)| (s.object, s.progress()))
    }

    /// The target under the pointer (for highlighting), if the hand is free to point.
    pub fn focus<'a>(&self, tracked: &Tracked, targets: &'a [Target]) -> Option<&'a Target> {
        let hand = tracked.hand(self.settings.main_hand)?;
        pointed(
            targets,
            &hand,
            self.settings.point_range,
            self.settings.point_slack,
            |t| matches!(t.kind, TargetKind::Npc { .. } | TargetKind::Recipe { .. }),
        )
        .or_else(|| {
            pointed(
                targets,
                &hand,
                self.settings.point_range,
                self.settings.point_slack,
                |_| true,
            )
        })
    }

    /// One frame. `dt` is the frame time, s.
    pub fn update(
        &mut self,
        input: &VrInput,
        tracked: &Tracked,
        targets: &[Target],
        hero: &HeroState,
        dt: f32,
    ) -> Vec<Command> {
        let dt = if dt.is_finite() {
            dt.clamp(0.0, 0.1)
        } else {
            0.0
        };
        let mut out = Vec::new();
        self.dodge_cd = (self.dodge_cd - dt).max(0.0);
        self.jump_cd = (self.jump_cd - dt).max(0.0);
        self.body(tracked, dt, &mut out);
        if hero.in_menu {
            self.lock = None;
            self.cast_held = None;
            self.selecting = None;
            return out;
        }
        let busy = self.lockpick(input, tracked, &mut out);
        let bow_busy = self.ranged(input, tracked, targets, hero, &mut out);
        let spell_busy = self.spells(input, tracked, targets, hero, dt, &mut out);
        self.eat(tracked, hero, dt, &mut out);
        for hand in Hand::BOTH {
            if busy == Some(hand) || bow_busy.contains(&hand) {
                continue;
            }
            if input.just_pressed(hand, VrButton::Grip)
                && let Some(p) = tracked.hand(hand)
            {
                self.grab(hand, &p, targets, hero, &mut out);
            }
        }
        let main = self.settings.main_hand;
        if !spell_busy
            && busy != Some(main)
            && !bow_busy.contains(&main)
            && input.just_pressed(main, VrButton::Trigger)
            && let Some(t) = self.focus(tracked, targets)
        {
            match &t.kind {
                TargetKind::Recipe { recipe } => out.push(Command::Craft { recipe: *recipe }),
                _ => {
                    if let Some(focus) = t.focus_id() {
                        out.push(Command::Confirm { focus });
                    }
                }
            }
        }
        out
    }

    /// Real crouch -> sneak, a quick step -> dodge, a hop -> jump.
    fn body(&mut self, tracked: &Tracked, dt: f32, out: &mut Vec<Command>) {
        let s = self.settings;
        let head = tracked.head.position;
        if !head.is_finite() {
            return;
        }
        let standing = self.standing_height.get_or_insert(head.y);
        *standing = standing.max(head.y);
        let standing = *standing;
        if standing > 0.5 {
            let k = head.y / standing;
            if (!self.sneaking && k < s.crouch_enter) || (self.sneaking && k > s.crouch_exit) {
                self.sneaking = !self.sneaking;
                out.push(Command::ToggleSneak);
            }
        }
        if let Some(last) = self.last_head.replace(head)
            && dt > 0.0
        {
            let v = (head - last) / dt;
            let flat = Vec3::new(v.x, 0.0, v.z);
            if self.dodge_cd <= 0.0 && flat.length() > s.dodge_speed {
                let yaw = tracked.head.yaw();
                let fwd = Vec3::new(-yaw.sin(), 0.0, -yaw.cos());
                let right = Vec3::new(-fwd.z, 0.0, fwd.x);
                let (f, r) = (flat.dot(fwd), flat.dot(right));
                let direction = if r.abs() >= f.abs() {
                    if r > 0.0 {
                        Direction::Right
                    } else {
                        Direction::Left
                    }
                } else if f > 0.0 {
                    Direction::Forward
                } else {
                    Direction::Backward
                };
                self.dodge_cd = s.dodge_cooldown;
                out.push(Command::Dodge { direction });
            }
            if let Some(j) = s.jump_speed
                && self.jump_cd <= 0.0
                && !self.sneaking
                && v.y > j
            {
                self.jump_cd = s.jump_cooldown;
                out.push(Command::Jump);
            }
        }
    }

    fn grab(
        &mut self,
        hand: Hand,
        p: &Pose,
        targets: &[Target],
        hero: &HeroState,
        out: &mut Vec<Command>,
    ) {
        let Some(t) = grabbable(targets, p.position, self.settings.grab_reach) else {
            return;
        };
        match &t.kind {
            TargetKind::Item { entity, .. } => out.push(Command::PickUp { item: *entity }),
            TargetKind::Body { entity } => out.push(Command::QuickLoot { body: *entity }),
            TargetKind::Object { entity, lock } => match lock {
                Some(l) if l.locked && l.has_key => out.push(Command::Lockpick {
                    object: *entity,
                    success: None,
                }),
                Some(l) if l.locked => {
                    if hero.lockpicks > 0 {
                        self.lock = Some((
                            hand,
                            LockSession::new(*entity, l.clone(), p.rotation, p.position),
                        ));
                    }
                }
                _ => out.push(Command::UseInteractObject { object: *entity }),
            },
            TargetKind::Npc { .. } | TargetKind::Recipe { .. } => {}
        }
    }

    /// Returns the hand busy with a lock.
    fn lockpick(
        &mut self,
        input: &VrInput,
        tracked: &Tracked,
        out: &mut Vec<Command>,
    ) -> Option<Hand> {
        let s = self.settings;
        let (hand, session) = self.lock.as_mut()?;
        let hand = *hand;
        let Some(p) = tracked.hand(hand) else {
            self.lock = None;
            return Some(hand);
        };
        match session.step(
            input.held(hand, VrButton::Grip),
            p.rotation,
            p.position,
            s.lock_turn,
            s.lock_neutral,
            s.lock_leash,
        ) {
            LockStep::Continue | LockStep::Turned(_) => {}
            LockStep::Done(success) => {
                out.push(Command::Lockpick {
                    object: session.object,
                    success,
                });
                self.lock = None;
            }
            LockStep::Aborted => self.lock = None,
        }
        Some(hand)
    }

    /// Returns the hands the bow uses.
    fn ranged(
        &mut self,
        input: &VrInput,
        tracked: &Tracked,
        targets: &[Target],
        hero: &HeroState,
        out: &mut Vec<Command>,
    ) -> Vec<Hand> {
        let s = self.settings;
        let (string, bow) = (s.main_hand, s.main_hand.other());
        if !hero.bow_equipped {
            self.bow = BowState::Idle;
            return Vec::new();
        }
        let (Some(bp), Some(sp)) = (tracked.hand(bow), tracked.hand(string)) else {
            return Vec::new();
        };
        let draw = bp.position.distance(sp.position);
        let bow_held = input.held(bow, VrButton::Grip);
        let string_held = input.held(string, VrButton::Grip);
        if !bow_held {
            if self.bow == BowState::Aiming {
                out.push(Command::AimRanged { aim: false });
            }
            self.bow = BowState::Idle;
            return Vec::new();
        }
        self.bow = match self.bow {
            BowState::Idle => {
                // Grabbing near a target grabs the target instead of raising the bow.
                if input.just_pressed(bow, VrButton::Grip)
                    && grabbable(targets, bp.position, s.grab_reach).is_none()
                {
                    out.push(Command::ReadyRanged);
                    BowState::Ready
                } else {
                    BowState::Idle
                }
            }
            BowState::Ready => {
                if input.just_pressed(string, VrButton::Grip) && draw <= s.nock_reach {
                    BowState::Nocked
                } else {
                    BowState::Ready
                }
            }
            BowState::Nocked if !string_held => BowState::Ready,
            BowState::Nocked if draw >= s.aim_draw => {
                out.push(Command::AimRanged { aim: true });
                BowState::Aiming
            }
            BowState::Nocked => BowState::Nocked,
            BowState::Aiming if !string_held => {
                let dir = bp.position - sp.position;
                out.push(Command::ReleaseRanged {
                    target: aimed_actor(targets, bp.position, dir, s.aim_range, s.aim_cone),
                });
                BowState::Ready
            }
            BowState::Aiming if draw < s.lower_draw => {
                out.push(Command::AimRanged { aim: false });
                BowState::Nocked
            }
            BowState::Aiming => BowState::Aiming,
        };
        match self.bow {
            BowState::Idle => Vec::new(),
            BowState::Ready => vec![bow],
            BowState::Nocked | BowState::Aiming => vec![bow, string],
        }
    }

    /// Off hand's secondary held: a ring of quick-slot spells around the off hand, the main
    /// hand picks one, release selects it. With a spell in hand the trigger casts at what the
    /// hand aims at (held long = power cast); the grip during a charge aborts it.
    /// Returns whether the main hand's trigger belongs to the spell.
    fn spells(
        &mut self,
        input: &VrInput,
        tracked: &Tracked,
        targets: &[Target],
        hero: &HeroState,
        dt: f32,
        out: &mut Vec<Command>,
    ) -> bool {
        let s = self.settings;
        let (main, off) = (s.main_hand, s.main_hand.other());
        if input.held(off, VrButton::Secondary) && !hero.spells.is_empty() {
            if let (Some(op), Some(mp)) = (tracked.hand(off), tracked.hand(main)) {
                // Angle of the main hand around the off hand, in the off hand's frame.
                let local = op.inverse().transform_point(mp.position);
                let n = hero.spells.len();
                let a = local.y.atan2(local.x).rem_euclid(std::f32::consts::TAU);
                let slot = ((a / std::f32::consts::TAU * n as f32).round() as usize) % n;
                self.selecting = (local.length() > 0.08).then_some(slot);
            }
            return true;
        }
        if input.just_released(off, VrButton::Secondary)
            && let Some(slot) = self.selecting.take()
            && let Some(template) = hero.spells.get(slot)
        {
            self.spell = Some(template.clone());
            out.push(Command::SelectSpell {
                template: template.clone(),
            });
            return true;
        }
        self.selecting = None;
        if self.spell.is_none() || self.bow != BowState::Idle {
            self.cast_held = None;
            return false;
        }
        if input.just_pressed(main, VrButton::Trigger) {
            self.cast_held = Some(0.0);
        }
        if let Some(t) = self.cast_held.as_mut() {
            if input.just_pressed(main, VrButton::Grip) {
                self.cast_held = None;
                out.push(Command::AbortCast);
            } else if input.held(main, VrButton::Trigger) {
                *t += dt;
            } else {
                let power = *t >= s.power_cast_hold;
                self.cast_held = None;
                let target = tracked.hand(main).and_then(|p| {
                    aimed_actor(targets, p.position, p.forward(), s.aim_range, s.aim_cone)
                });
                out.push(Command::CastSpell { target, power });
            }
        }
        true
    }

    /// A held consumable at the mouth for a moment is used.
    fn eat(&mut self, tracked: &Tracked, hero: &HeroState, dt: f32, out: &mut Vec<Command>) {
        let h = tracked.head;
        let mouth = h.transform_point(Vec3::new(0.0, -0.1, -0.08));
        for hand in Hand::BOTH {
            let i = hand.index();
            let near = tracked
                .hand(hand)
                .is_some_and(|p| p.position.distance(mouth) <= self.settings.eat_reach);
            match (&hero.held_items[i], near) {
                (Some(template), true) => {
                    let before = self.eat[i];
                    self.eat[i] += dt;
                    if before < self.settings.eat_time && self.eat[i] >= self.settings.eat_time {
                        out.push(Command::UseItem {
                            template: template.clone(),
                        });
                    }
                }
                _ => self.eat[i] = 0.0,
            }
        }
    }
}
