//! Headless greybox demo for VR-11: a scripted player in a room with a character, a table
//! with gold, a body, a door, a locked chest, a work bench's recipe board and a target
//! dummy far away. Every interaction goes through [`InteractMapper`] into a [`StubCore`] that
//! answers with core events.
//!
//! `cargo run -p gothic-vr-interact --example interact_greybox` prints the command and event
//! log and fails if any mapping did not fire.
use glam::{Quat, Vec3};
use gothic_runtime::{Command, Event};
use gothic_sim::types::ActorId;
use gothic_vr::{ControllerState, Hand, Pose, VrInput, XrFrame};
use gothic_vr_interact::core_api::StubEntity;
use gothic_vr_interact::*;

const DT: f32 = 1.0 / 90.0;
const LEFT: Vec3 = Vec3::new(-0.25, 1.3, -0.3);
const RIGHT: Vec3 = Vec3::new(0.25, 1.3, -0.3);

struct Player {
    input: VrInput,
    frame: XrFrame,
    mapper: InteractMapper,
    hero: HeroState,
    targets: Vec<Target>,
    core: StubCore,
    sent: Vec<Command>,
}

impl Player {
    fn frame(&mut self, f: impl FnOnce(&mut XrFrame)) {
        f(&mut self.frame);
        self.input.push(self.frame);
        let tracked = Tracked::from_input(&self.input);
        let cmds = self
            .mapper
            .update(&self.input, &tracked, &self.targets, &self.hero, DT);
        for c in &cmds {
            println!("  -> {c:?}");
        }
        for e in self.core.tick(DT, &cmds) {
            println!("     <- {e:?}");
            if let Event::ItemMoved {
                item, gained: true, ..
            } = &e
                && item.starts_with("ItPo")
            {
                // The physics hand now holds the potion.
                self.hero.held_items[Hand::Right.index()] = Some(item.clone());
            }
        }
        self.sent.extend(cmds);
    }

    fn wait(&mut self, frames: usize) {
        for _ in 0..frames {
            self.frame(|_| {});
        }
    }

    fn hand(&mut self, h: Hand, pos: Vec3, rot: Quat) {
        self.frame(|f| f.hand_mut(h).pose = Some(Pose::new(pos, rot)));
    }

    fn button(&mut self, h: Hand, set: fn(&mut ControllerState, bool), on: bool) {
        self.frame(|f| set(f.hand_mut(h), on));
    }

    /// Point the right hand from its rest position at `p`.
    fn point_at(&mut self, p: Vec3) {
        let rot = Quat::from_rotation_arc(Vec3::NEG_Z, (p - RIGHT).normalize());
        self.hand(Hand::Right, RIGHT, rot);
    }

    fn tap(&mut self, h: Hand, set: fn(&mut ControllerState, bool)) {
        self.button(h, set, true);
        self.button(h, set, false);
    }

    fn grab_at(&mut self, h: Hand, p: Vec3) {
        self.hand(h, p, Quat::IDENTITY);
        self.tap(h, grip);
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

fn main() {
    let diego = ActorId(1);
    let gold = Vec3::new(0.5, 0.9, -0.5);
    let body = Vec3::new(-0.6, 0.4, -0.7);
    let door = Vec3::new(1.0, 1.0, -0.6);
    let chest = Vec3::new(0.2, 0.6, -0.5);
    let board = Vec3::new(-0.8, 1.4, -1.2);
    let dummy = Vec3::new(-0.25, 1.4, -12.0);
    let targets = vec![
        Target::new(
            TargetKind::Npc { actor: diego },
            Vec3::new(0.3, 1.4, -2.5),
            0.35,
        ),
        Target::new(TargetKind::Npc { actor: ActorId(9) }, dummy, 0.4),
        Target::new(
            TargetKind::Item {
                entity: 100,
                template: "ItPo_Health_01".into(),
            },
            gold,
            0.05,
        ),
        Target::new(TargetKind::Body { entity: 101 }, body, 0.3),
        Target::new(
            TargetKind::Object {
                entity: 102,
                lock: None,
            },
            door,
            0.05,
        ),
        Target::new(
            TargetKind::Object {
                entity: 103,
                lock: Some(LockSpec::combination(&[Turn::Left, Turn::Right])),
            },
            chest,
            0.05,
        ),
        Target::new(TargetKind::Recipe { recipe: 17 }, board, 0.15),
    ];
    let mut core = StubCore::default();
    core.add(2, StubEntity::Npc)
        .add(
            100,
            StubEntity::Item {
                template: "ItPo_Health_01".into(),
            },
        )
        .add(
            101,
            StubEntity::Body {
                loot: vec!["ItMi_Gold".into(), "ItMw_Club".into()],
            },
        )
        .add(
            102,
            StubEntity::Object {
                name: "door".into(),
                locked: false,
            },
        )
        .add(
            103,
            StubEntity::Object {
                name: "chest".into(),
                locked: true,
            },
        );
    let mut frame = XrFrame::default();
    frame.hand_mut(Hand::Left).pose = Some(Pose::new(LEFT, Quat::IDENTITY));
    frame.hand_mut(Hand::Right).pose = Some(Pose::new(RIGHT, Quat::IDENTITY));
    let mut p = Player {
        input: VrInput::new(),
        frame,
        mapper: InteractMapper::default(),
        hero: HeroState {
            lockpicks: 3,
            spells: vec!["It_Spell_Fireball".into(), "It_Spell_Heal".into()],
            ..Default::default()
        },
        targets,
        core,
        sent: Vec::new(),
    };
    p.wait(5);

    println!("talk: point at Diego, trigger");
    p.point_at(Vec3::new(0.3, 1.4, -2.5));
    p.tap(Hand::Right, trigger);

    println!("craft: point at the recipe board, trigger");
    p.point_at(board);
    p.tap(Hand::Right, trigger);

    println!("pick up the potion, drink it");
    p.grab_at(Hand::Right, gold);
    p.hand(Hand::Right, Vec3::new(0.0, 1.6, -0.08), Quat::IDENTITY);
    p.wait(45);
    p.hero.held_items = [None, None];

    println!("loot the body, open the door");
    p.grab_at(Hand::Left, body + Vec3::Y * 0.2);
    p.hand(Hand::Left, LEFT, Quat::IDENTITY);
    p.grab_at(Hand::Right, door);

    println!("pick the chest's lock: left, right");
    p.hand(Hand::Right, chest, Quat::IDENTITY);
    p.button(Hand::Right, grip, true);
    for deg in [45.0f32, 0.0, -45.0] {
        p.hand(Hand::Right, chest, Quat::from_rotation_z(deg.to_radians()));
        p.wait(3);
    }
    p.button(Hand::Right, grip, false);
    println!("open it");
    p.targets[5].kind = TargetKind::Object {
        entity: 103,
        lock: None,
    };
    p.tap(Hand::Right, grip);

    println!("select the heal spell on the ring, then fireball, cast at Diego, power cast");
    p.hand(Hand::Right, RIGHT, Quat::IDENTITY);
    p.button(Hand::Left, secondary, true);
    p.hand(Hand::Right, LEFT - Vec3::X * 0.15, Quat::IDENTITY);
    p.button(Hand::Left, secondary, false);
    p.button(Hand::Left, secondary, true);
    p.hand(Hand::Right, LEFT + Vec3::X * 0.15, Quat::IDENTITY);
    p.button(Hand::Left, secondary, false);
    p.point_at(Vec3::new(0.3, 1.4, -2.5));
    p.tap(Hand::Right, trigger);
    p.button(Hand::Right, trigger, true);
    p.wait(90);
    p.button(Hand::Right, trigger, false);
    p.mapper.spell = None;

    println!("bow: raise, nock, draw, shoot at the dummy");
    p.hero.bow_equipped = true;
    p.hand(Hand::Right, LEFT, Quat::IDENTITY);
    p.button(Hand::Left, grip, true);
    p.button(Hand::Right, grip, true);
    for k in 1..=10 {
        p.hand(
            Hand::Right,
            LEFT + Vec3::new(0.0, 0.0, 0.05 * k as f32),
            Quat::IDENTITY,
        );
    }
    p.button(Hand::Right, grip, false);
    p.button(Hand::Left, grip, false);
    p.hero.bow_equipped = false;
    p.hand(Hand::Right, RIGHT, Quat::IDENTITY);

    println!("crouch to sneak, stand up, dodge left, hop");
    for i in 0..=60 {
        let y = 1.7 - 0.6 * i as f32 / 60.0;
        p.frame(|f| f.head.position.y = y);
    }
    for i in 0..=60 {
        let y = 1.1 + 0.6 * i as f32 / 60.0;
        p.frame(|f| f.head.position.y = y);
    }
    for _ in 0..4 {
        p.frame(|f| f.head.position.x -= 0.04);
    }
    p.wait(90);
    for dy in [0.025f32; 6].into_iter().chain([-0.025; 6]) {
        p.frame(|f| f.head.position.y += dy);
    }

    let names: Vec<String> = p
        .sent
        .iter()
        .map(|c| {
            format!("{c:?}")
                .split([' ', '('])
                .next()
                .unwrap_or("")
                .to_string()
        })
        .collect();
    let want = [
        "Confirm",
        "Craft",
        "PickUp",
        "UseItem",
        "QuickLoot",
        "UseInteractObject",
        "Lockpick",
        "SelectSpell",
        "CastSpell",
        "ReadyRanged",
        "AimRanged",
        "ReleaseRanged",
        "ToggleSneak",
        "Dodge",
        "Jump",
    ];
    let missing: Vec<_> = want
        .iter()
        .filter(|w| !names.iter().any(|n| n == *w))
        .collect();
    println!(
        "{} commands; inventory {:?}; missing {missing:?}",
        p.sent.len(),
        p.core.inventory
    );
    assert!(
        missing.is_empty(),
        "mappings that did not fire: {missing:?}"
    );
}
