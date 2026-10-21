//! One test per VR interaction -> core command mapping.
use glam::{Quat, Vec3};
use gothic_runtime::Command;
use gothic_sim::script::player::evade::Direction;
use gothic_sim::types::ActorId;
use gothic_vr::{ControllerState, Hand, Pose, VrInput, XrFrame};
use gothic_vr_interact::*;

/// `Command` has no `PartialEq`: compare the debug forms.
macro_rules! assert_eq {
    ($a:expr, $b:expr $(, $($m:tt)*)?) => {
        std::assert_eq!(format!("{:?}", $a), format!("{:?}", $b) $(, $($m)*)?)
    };
}

const DT: f32 = 1.0 / 90.0;
const NPC: ActorId = ActorId(4);
const LEFT_REST: Vec3 = Vec3::new(-0.25, 1.3, -0.3);
const RIGHT_REST: Vec3 = Vec3::new(0.25, 1.3, -0.3);

struct Rig {
    input: VrInput,
    frame: XrFrame,
    mapper: InteractMapper,
    hero: HeroState,
    targets: Vec<Target>,
}

impl Rig {
    fn new(targets: Vec<Target>) -> Self {
        let mut frame = XrFrame::default();
        frame.hand_mut(Hand::Left).pose = Some(Pose::new(LEFT_REST, Quat::IDENTITY));
        frame.hand_mut(Hand::Right).pose = Some(Pose::new(RIGHT_REST, Quat::IDENTITY));
        let mut rig = Self {
            input: VrInput::new(),
            frame,
            mapper: InteractMapper::default(),
            hero: HeroState::default(),
            targets,
        };
        rig.idle(3);
        rig
    }

    fn step(&mut self, f: impl FnOnce(&mut XrFrame)) -> Vec<Command> {
        f(&mut self.frame);
        self.input.push(self.frame);
        let tracked = Tracked::from_input(&self.input);
        self.mapper
            .update(&self.input, &tracked, &self.targets, &self.hero, DT)
    }

    fn idle(&mut self, frames: usize) -> Vec<Command> {
        (0..frames).flat_map(|_| self.step(|_| {})).collect()
    }

    fn set_hand(&mut self, h: Hand, pos: Vec3, rot: Quat) -> Vec<Command> {
        self.step(|f| f.hand_mut(h).pose = Some(Pose::new(pos, rot)))
    }

    fn press(&mut self, h: Hand, set: fn(&mut ControllerState, bool), on: bool) -> Vec<Command> {
        self.step(|f| set(f.hand_mut(h), on))
    }
}

fn grip(c: &mut ControllerState, on: bool) {
    c.grip = if on { 1.0 } else { 0.0 };
}

fn trigger(c: &mut ControllerState, on: bool) {
    c.trigger = if on { 1.0 } else { 0.0 };
}

fn secondary(c: &mut ControllerState, on: bool) {
    c.secondary = on;
}

fn npc_ahead() -> Target {
    Target::new(
        TargetKind::Npc { actor: NPC },
        Vec3::new(0.25, 1.3, -3.0),
        0.35,
    )
}

#[test]
fn point_and_trigger_talks() {
    let mut r = Rig::new(vec![npc_ahead()]);
    assert_eq!(
        r.press(Hand::Right, trigger, true),
        [Command::Confirm { focus: 5 }]
    );
    r.press(Hand::Right, trigger, false);
    // Pointing 20 degrees away hits nothing.
    r.set_hand(
        Hand::Right,
        RIGHT_REST,
        Quat::from_rotation_y(20f32.to_radians()),
    );
    assert!(r.press(Hand::Right, trigger, true).is_empty());
}

#[test]
fn point_at_recipe_crafts() {
    let board = Target::new(
        TargetKind::Recipe { recipe: 17 },
        Vec3::new(0.25, 1.3, -0.9),
        0.1,
    );
    let mut r = Rig::new(vec![board]);
    assert_eq!(
        r.press(Hand::Right, trigger, true),
        [Command::Craft { recipe: 17 }]
    );
}

#[test]
fn grabbing_picks_up_loots_and_opens() {
    let near = |x: f32| Vec3::new(x, 1.0, -0.4);
    let mut r = Rig::new(vec![
        Target::new(
            TargetKind::Item {
                entity: 900,
                template: "ItMi_Gold".into(),
            },
            near(0.0),
            0.05,
        ),
        Target::new(TargetKind::Body { entity: 901 }, near(1.0), 0.3),
        Target::new(
            TargetKind::Object {
                entity: 902,
                lock: None,
            },
            near(2.0),
            0.4,
        )
        .with_handle(near(2.0) + Vec3::Y * 0.3),
        Target::new(
            TargetKind::Object {
                entity: 903,
                lock: Some(LockSpec {
                    locked: true,
                    has_key: true,
                    combination: None,
                }),
            },
            near(3.0),
            0.1,
        ),
    ]);
    let grab_at = |r: &mut Rig, p: Vec3| {
        r.set_hand(Hand::Right, p, Quat::IDENTITY);
        let c = r.press(Hand::Right, grip, true);
        r.press(Hand::Right, grip, false);
        c
    };
    assert_eq!(grab_at(&mut r, near(0.05)), [Command::PickUp { item: 900 }]);
    assert_eq!(
        grab_at(&mut r, near(1.0) + Vec3::Y * 0.2),
        [Command::QuickLoot { body: 901 }]
    );
    // The door's handle, not its centre.
    assert!(grab_at(&mut r, near(2.0) - Vec3::Y * 0.6).is_empty());
    assert_eq!(
        grab_at(&mut r, near(2.0) + Vec3::Y * 0.3),
        [Command::UseInteractObject { object: 902 }]
    );
    assert_eq!(
        grab_at(&mut r, near(3.0)),
        [Command::Lockpick {
            object: 903,
            success: None
        }],
        "with the key"
    );
}

fn locked_chest(combination: Option<Vec<Turn>>) -> Target {
    Target::new(
        TargetKind::Object {
            entity: 77,
            lock: Some(LockSpec {
                locked: true,
                has_key: false,
                combination,
            }),
        },
        Vec3::new(0.25, 1.0, -0.4),
        0.05,
    )
}

/// Grip the lock, then roll the wrist through `rolls` (degrees, each held a few frames).
fn pick(r: &mut Rig, rolls: &[f32]) -> Vec<Command> {
    let at = Vec3::new(0.25, 1.0, -0.4);
    r.set_hand(Hand::Right, at, Quat::IDENTITY);
    let mut out = r.press(Hand::Right, grip, true);
    for deg in rolls {
        out.extend(r.set_hand(Hand::Right, at, Quat::from_rotation_z(deg.to_radians())));
        out.extend(r.idle(3));
    }
    out
}

#[test]
fn lockpick_combination_opens() {
    let mut r = Rig::new(vec![locked_chest(Some(vec![
        Turn::Left,
        Turn::Right,
        Turn::Right,
    ]))]);
    r.hero.lockpicks = 2;
    let out = pick(&mut r, &[45.0, 0.0, -45.0, 0.0, -45.0]);
    assert_eq!(
        out,
        [Command::Lockpick {
            object: 77,
            success: Some(true)
        }]
    );
    assert!(r.mapper.lock_progress().is_none());
}

#[test]
fn lockpick_needs_neutral_between_turns_and_breaks_on_a_wrong_turn() {
    let mut r = Rig::new(vec![locked_chest(Some(vec![Turn::Left, Turn::Left]))]);
    r.hero.lockpicks = 1;
    // Holding the first turn is one turn only.
    assert!(pick(&mut r, &[45.0, 50.0, 45.0]).is_empty());
    assert_eq!(r.mapper.lock_progress(), Some((77, 1)));
    let out: Vec<_> = [0.0, -45.0]
        .iter()
        .flat_map(|d| {
            r.set_hand(
                Hand::Right,
                Vec3::new(0.25, 1.0, -0.4),
                Quat::from_rotation_z(f32::to_radians(*d)),
            )
        })
        .collect();
    assert_eq!(
        out,
        [Command::Lockpick {
            object: 77,
            success: Some(false)
        }]
    );
}

#[test]
fn lockpick_without_combination_lets_the_script_roll() {
    let mut r = Rig::new(vec![locked_chest(None)]);
    r.hero.lockpicks = 1;
    assert_eq!(
        pick(&mut r, &[-40.0]),
        [Command::Lockpick {
            object: 77,
            success: None
        }]
    );
}

#[test]
fn lockpick_needs_a_pick_and_aborts_on_letting_go() {
    let mut r = Rig::new(vec![locked_chest(Some(vec![Turn::Left]))]);
    assert!(pick(&mut r, &[45.0]).is_empty(), "no lockpick");
    assert!(r.mapper.lock_progress().is_none());
    r.press(Hand::Right, grip, false);
    r.hero.lockpicks = 1;
    r.set_hand(Hand::Right, Vec3::new(0.25, 1.0, -0.4), Quat::IDENTITY);
    r.press(Hand::Right, grip, true);
    assert!(r.mapper.lock_progress().is_some());
    r.press(Hand::Right, grip, false);
    assert!(r.mapper.lock_progress().is_none());
}

#[test]
fn potion_at_the_mouth_is_used_once() {
    let mut r = Rig::new(vec![]);
    r.hero.held_items[Hand::Right.index()] = Some("ItPo_Health_01".into());
    let mouth = Vec3::new(0.0, 1.6, -0.08);
    r.set_hand(Hand::Right, mouth, Quat::IDENTITY);
    let out = r.idle(90);
    assert_eq!(
        out,
        [Command::UseItem {
            template: "ItPo_Health_01".into()
        }]
    );
    // Held near the chest: nothing.
    r.set_hand(Hand::Right, RIGHT_REST, Quat::IDENTITY);
    assert!(r.idle(90).is_empty());
}

fn spell_rig() -> Rig {
    let mut r = Rig::new(vec![npc_ahead()]);
    r.hero.spells = [
        "It_Spell_Fireball",
        "It_Spell_Heal",
        "It_Spell_Frost",
        "It_Spell_Light",
    ]
    .map(String::from)
    .to_vec();
    r
}

#[test]
fn spell_ring_selects_by_hand_position() {
    let mut r = spell_rig();
    assert!(r.press(Hand::Left, secondary, true).is_empty());
    // Above the off hand: the second of four slots (90 degrees).
    r.set_hand(Hand::Right, LEFT_REST + Vec3::Y * 0.15, Quat::IDENTITY);
    assert_eq!(r.mapper.selecting, Some(1));
    // A trigger pull while choosing does not talk.
    assert!(r.press(Hand::Right, trigger, true).is_empty());
    r.press(Hand::Right, trigger, false);
    assert_eq!(
        r.press(Hand::Left, secondary, false),
        [Command::SelectSpell {
            template: "It_Spell_Heal".into()
        }]
    );
    assert_eq!(r.mapper.spell.as_deref(), Some("It_Spell_Heal"));
}

#[test]
fn casting_aims_from_the_hand_and_power_casts_on_a_long_hold() {
    let mut r = spell_rig();
    r.press(Hand::Left, secondary, true);
    r.set_hand(Hand::Right, LEFT_REST + Vec3::X * 0.15, Quat::IDENTITY);
    r.press(Hand::Left, secondary, false);
    r.set_hand(Hand::Right, RIGHT_REST, Quat::IDENTITY);
    assert_eq!(r.mapper.spell.as_deref(), Some("It_Spell_Fireball"));
    assert!(r.press(Hand::Right, trigger, true).is_empty());
    assert_eq!(
        r.press(Hand::Right, trigger, false),
        [Command::CastSpell {
            target: Some(NPC),
            power: false
        }]
    );
    // Aim away: free cast; hold for a second: power.
    r.set_hand(Hand::Right, RIGHT_REST, Quat::from_rotation_y(1.2));
    r.press(Hand::Right, trigger, true);
    r.idle(90);
    assert_eq!(
        r.press(Hand::Right, trigger, false),
        [Command::CastSpell {
            target: None,
            power: true
        }]
    );
    // Grip during the charge aborts.
    r.press(Hand::Right, trigger, true);
    assert_eq!(r.press(Hand::Right, grip, true), [Command::AbortCast]);
    assert!(r.press(Hand::Right, trigger, false).is_empty());
}

#[test]
fn bow_readies_aims_and_shoots_at_the_aimed_npc() {
    let mut r = Rig::new(vec![Target::new(
        TargetKind::Npc { actor: NPC },
        Vec3::new(-0.25, 1.4, -10.0),
        0.4,
    )]);
    r.hero.bow_equipped = true;
    assert_eq!(r.press(Hand::Left, grip, true), [Command::ReadyRanged]);
    assert_eq!(r.mapper.bow, BowState::Ready);
    // Nock: string hand at the bow hand.
    r.set_hand(
        Hand::Right,
        LEFT_REST + Vec3::new(0.0, 0.0, 0.05),
        Quat::IDENTITY,
    );
    assert!(r.press(Hand::Right, grip, true).is_empty());
    assert_eq!(r.mapper.bow, BowState::Nocked);
    // Draw back towards the face.
    let mut out = Vec::new();
    for k in 1..=10 {
        out.extend(r.set_hand(
            Hand::Right,
            LEFT_REST + Vec3::new(0.0, 0.05, 0.05 * k as f32),
            Quat::IDENTITY,
        ));
    }
    assert_eq!(out, [Command::AimRanged { aim: true }]);
    assert_eq!(
        r.press(Hand::Right, grip, false),
        [Command::ReleaseRanged { target: Some(NPC) }]
    );
    assert_eq!(r.mapper.bow, BowState::Ready);
}

#[test]
fn letting_the_string_down_lowers_the_bow() {
    let mut r = Rig::new(vec![]);
    r.hero.bow_equipped = true;
    r.press(Hand::Left, grip, true);
    r.set_hand(Hand::Right, LEFT_REST, Quat::IDENTITY);
    r.press(Hand::Right, grip, true);
    let drawn = r.set_hand(Hand::Right, LEFT_REST + Vec3::Z * 0.5, Quat::IDENTITY);
    assert_eq!(drawn, [Command::AimRanged { aim: true }]);
    let eased = r.set_hand(Hand::Right, LEFT_REST + Vec3::Z * 0.1, Quat::IDENTITY);
    assert_eq!(eased, [Command::AimRanged { aim: false }]);
    assert!(r.press(Hand::Right, grip, false).is_empty(), "no shot");
}

#[test]
fn real_crouch_toggles_sneak() {
    let mut r = Rig::new(vec![]);
    let mut out = Vec::new();
    // Slow enough not to count as a dodge or jump.
    for i in 0..=60 {
        let y = 1.7 - 0.6 * i as f32 / 60.0;
        out.extend(r.step(|f| f.head.position.y = y));
    }
    assert_eq!(out, [Command::ToggleSneak]);
    assert!(r.mapper.sneaking);
    out.clear();
    for i in 0..=60 {
        let y = 1.1 + 0.6 * i as f32 / 60.0;
        out.extend(r.step(|f| f.head.position.y = y));
    }
    assert_eq!(out, [Command::ToggleSneak]);
    assert!(!r.mapper.sneaking);
}

#[test]
fn quick_steps_dodge_in_the_head_frame() {
    let mut r = Rig::new(vec![]);
    let dodge = |r: &mut Rig, d: Vec3| {
        let mut out = Vec::new();
        for _ in 0..4 {
            out.extend(r.step(|f| f.head.position += d * 0.04));
        }
        out.extend(r.idle(90));
        out
    };
    assert_eq!(
        dodge(&mut r, Vec3::X),
        [Command::Dodge {
            direction: Direction::Right
        }]
    );
    assert_eq!(
        dodge(&mut r, Vec3::Z),
        [Command::Dodge {
            direction: Direction::Backward
        }]
    );
    // Looking right (-90 degrees yaw), a step along +Z is to the player's right.
    r.step(|f| f.head.rotation = Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2));
    assert_eq!(
        dodge(&mut r, Vec3::Z),
        [Command::Dodge {
            direction: Direction::Right
        }]
    );
}

#[test]
fn a_hop_jumps() {
    let mut r = Rig::new(vec![]);
    let mut out = Vec::new();
    for i in 0..12 {
        let dy = if i < 6 { 0.025 } else { -0.025 };
        out.extend(r.step(|f| f.head.position.y += dy));
    }
    assert_eq!(out, [Command::Jump]);
}

#[test]
fn an_open_menu_blocks_world_interaction() {
    let mut r = Rig::new(vec![npc_ahead()]);
    r.hero.in_menu = true;
    assert!(r.press(Hand::Right, trigger, true).is_empty());
}

#[test]
fn stub_core_answers_the_mapped_commands() {
    use gothic_runtime::Event;
    let mut core = StubCore::default();
    core.add(5, core_api::StubEntity::Npc)
        .add(
            900,
            core_api::StubEntity::Item {
                template: "ItMi_Gold".into(),
            },
        )
        .add(
            77,
            core_api::StubEntity::Object {
                name: "chest".into(),
                locked: true,
            },
        );
    let ev = core.tick(
        DT,
        &[
            Command::Confirm { focus: 5 },
            Command::PickUp { item: 900 },
            Command::Lockpick {
                object: 77,
                success: Some(true),
            },
            Command::UseInteractObject { object: 77 },
        ],
    );
    assert!(matches!(ev[0], Event::Confirmed(_)));
    assert!(matches!(&ev[1], Event::ItemMoved { item, gained: true, .. } if item == "ItMi_Gold"));
    assert!(matches!(&ev[2], Event::Notice(n) if n == "chest unlocked"));
    assert!(matches!(&ev[3], Event::Notice(n) if n == "use chest"));
    assert_eq!(core.log.len(), 4);
}
