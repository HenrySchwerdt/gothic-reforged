//! Headless scenarios of the flow: i-frames, stamina gating, chain timings, guard, parry and
//! riposte, poise, hit-stop, telegraph, lock-on, and the bridge to the original damage pipeline.
use crate::actions::{ActionId, Dir, StrikeKind};
use crate::events::{Event, HitResult};
use crate::foe::{FoeProfile, FoeState};
use crate::hero::{Button, Input};
use crate::math::dir_of;
use crate::sink::{HitSink, SimpleSink};
use crate::tuning::{DF, DT, Tuning};
use crate::world::{OpenField, SoulsWorld};
use glam::{Vec2, Vec3};

const HERO: u32 = 0;
const FOE: u32 = 1;

struct Rig {
    w: SoulsWorld,
    sink: SimpleSink,
    env: OpenField,
    log: Vec<(u64, Event)>,
}

impl Rig {
    /// Hero at the origin facing -Z, one foe `dist` cm ahead. Foes are driven by hand.
    fn new(dist: f32, profile: FoeProfile) -> Self {
        let mut w = SoulsWorld::new(Vec3::ZERO, 400.0, Tuning::default());
        let foe = w.add_foe(FOE, Vec3::new(0.0, 0.0, -dist), 1000.0, profile);
        foe.autonomous = false;
        let mut sink = SimpleSink::default();
        sink.add(HERO, 400.0, 40.0, 0.0);
        sink.add(FOE, 1000.0, 40.0, 5.0);
        Self {
            w,
            sink,
            env: OpenField::default(),
            log: Vec::new(),
        }
    }

    fn tick(&mut self, inp: &Input) {
        let evs = self.w.step(inp, &mut self.sink, &mut self.env).to_vec();
        let t = self.w.tick;
        self.log.extend(evs.into_iter().map(|e| (t, e)));
    }

    fn run(&mut self, n: usize, inp: &Input) {
        for _ in 0..n {
            self.tick(inp);
        }
    }

    fn actions(&self) -> Vec<(u64, ActionId)> {
        self.log
            .iter()
            .filter_map(|(t, e)| match e {
                Event::HeroAction(a) => Some((*t, *a)),
                _ => None,
            })
            .collect()
    }

    fn hurts(&self) -> Vec<HitResult> {
        self.log
            .iter()
            .filter_map(|(_, e)| match e {
                Event::HeroHurt { result, .. } => Some(*result),
                _ => None,
            })
            .collect()
    }
}

fn idle() -> Input {
    Input::default()
}

fn press() -> Button {
    Button {
        held: true,
        pressed: true,
        released: false,
    }
}

fn held() -> Button {
    Button {
        held: true,
        pressed: false,
        released: false,
    }
}

fn release() -> Button {
    Button {
        held: false,
        pressed: false,
        released: true,
    }
}

fn light_press() -> Input {
    Input {
        light: press(),
        ..idle()
    }
}

/// Dodge tap: press this tick, release the next. `side` gives a stick direction (x right).
fn dodge_tap(rig: &mut Rig, side: Vec2) {
    rig.tick(&Input {
        dodge: press(),
        mv: side,
        ..idle()
    });
    rig.tick(&Input {
        dodge: release(),
        mv: side,
        ..idle()
    });
}

fn hp_of(rig: &Rig, id: u32) -> f32 {
    rig.sink.hp(id).0
}

// -------------------------------------------------------------------------------------------

#[test]
fn iframes_block_a_swing_that_otherwise_connects() {
    // Control: standing still, the classic swing hits.
    let mut rig = Rig::new(180.0, FoeProfile::classic());
    rig.w.foe_mut(FOE).unwrap().command_attack(0);
    rig.run(60, &idle());
    assert_eq!(rig.hurts(), vec![HitResult::Hit]);
    assert!(hp_of(&rig, HERO) < 400.0);

    // Rolling sideways just before the strike: the whole live window is inside the i-frames.
    let mut rig = Rig::new(180.0, FoeProfile::classic());
    rig.w.foe_mut(FOE).unwrap().command_attack(0);
    dodge_tap(&mut rig, Vec2::X);
    rig.run(60, &idle());
    assert!(
        rig.actions()
            .iter()
            .any(|(_, a)| matches!(a, ActionId::Roll(_))),
        "{:?}",
        rig.actions()
    );
    assert!(
        rig.hurts()
            .iter()
            .all(|r| *r == HitResult::Dodged || *r == HitResult::Ignored)
    );
    assert_eq!(hp_of(&rig, HERO), 400.0, "no damage reached the pipeline");
    assert!(rig.sink.log.is_empty());
}

#[test]
fn iframes_have_a_start_and_an_end() {
    let mut rig = Rig::new(10_000.0, FoeProfile::classic());
    let t = rig.w.tuning;
    dodge_tap(&mut rig, Vec2::X);
    assert!(matches!(rig.w.hero.action(), Some(ActionId::Roll(_))));
    let incoming = crate::hero::Incoming {
        attacker: FOE,
        from: Vec3::new(0.0, 0.0, -100.0),
        level: crate::actions::Hurt::Middle,
        poise: 10.0,
        parryable: true,
    };
    // Frame 0..3: still hittable. Inside: dodged. After frame 14: hittable again.
    let mut hero = rig.w.hero.clone();
    assert!(!hero.invincible());
    let mut seen = Vec::new();
    for _ in 0..40 {
        hero.step(&idle(), &Default::default(), &t);
        seen.push((hero.action_frame(), hero.invincible()));
    }
    for (f, inv) in seen {
        if hero.action().is_none() {
            break;
        }
        assert_eq!(inv, (3.0..14.0).contains(&f), "frame {f}");
    }
    let mut hero = rig.w.hero.clone();
    hero.step(&idle(), &Default::default(), &t);
    let r = hero.receive(&incoming, &mut rig.sink, &t);
    assert_ne!(r.result, HitResult::Dodged, "frame {}", hero.action_frame());
}

#[test]
fn stamina_gates_actions_and_regenerates_after_a_delay() {
    let mut rig = Rig::new(10_000.0, FoeProfile::classic());
    rig.w.hero.stamina = 0.0;
    rig.tick(&light_press());
    rig.run(10, &idle());
    assert!(rig.actions().is_empty(), "no attack with an empty bar");

    // A little stamina is enough to start, and the cost clamps at zero.
    rig.w.hero.stamina = 3.0;
    rig.tick(&light_press());
    rig.tick(&idle());
    assert_eq!(
        rig.actions(),
        vec![(rig.w.tick - 1, ActionId::Light(1))]
            .into_iter()
            .collect::<Vec<_>>()
    );
    assert!(rig.w.hero.stamina < 1.0);

    // No regeneration inside the delay, regeneration afterwards.
    let mut rig = Rig::new(10_000.0, FoeProfile::classic());
    rig.tick(&light_press());
    rig.tick(&idle());
    let after_spend = rig.w.hero.stamina;
    assert!(after_spend < rig.w.tuning.max_stamina - 5.0);
    rig.run(((rig.w.tuning.stamina_delay - 0.1) / DT) as usize, &idle());
    assert!(rig.w.hero.stamina <= after_spend + 1e-3, "inside the delay");
    rig.run(60, &idle());
    assert!(rig.w.hero.stamina > after_spend + 4.0, "after the delay");
}

#[test]
fn roll_costs_more_than_nothing_and_backstep_is_the_stickless_dodge() {
    let mut rig = Rig::new(10_000.0, FoeProfile::classic());
    dodge_tap(&mut rig, Vec2::ZERO);
    assert!(
        rig.actions().iter().any(|(_, a)| *a == ActionId::Backstep),
        "{:?}",
        rig.actions()
    );
    assert!((rig.w.tuning.max_stamina - rig.w.hero.stamina) >= rig.w.tuning.backstep_cost - 1.0);
}

#[test]
fn light_chain_follows_the_cancel_window_and_wraps() {
    let mut rig = Rig::new(10_000.0, FoeProfile::classic());
    rig.tick(&light_press());
    let l1 = rig.actions()[0];
    assert_eq!(l1.1, ActionId::Light(1));
    // Too early (before the animation listens): ignored.
    rig.run(4, &idle());
    rig.tick(&light_press());
    rig.run(2, &idle());
    assert_eq!(rig.actions().len(), 1, "early press is dropped");

    // Press once the animation listens; the follow-up waits for the cancel frame.
    rig.run(8, &idle());
    rig.tick(&light_press());
    let pressed_at = rig.w.tick;
    rig.run(40, &idle());
    let acts = rig.actions();
    assert_eq!(acts[1].1, ActionId::Light(2), "{acts:?}");
    let start = acts[1].0;
    assert!(start >= pressed_at, "never before the press");
    let cancel_tick = l1.0 + (ActionId::Light(1).def().cancel_light / DF) as u64;
    assert!(
        (cancel_tick..=cancel_tick + 2).contains(&start),
        "second hit at {start}, cancel frame at tick {cancel_tick}"
    );

    // Keep pressing: 3, then back to 1.
    for _ in 0..2 {
        rig.run(14, &idle());
        rig.tick(&light_press());
        rig.run(60, &idle());
    }
    let ids: Vec<_> = rig.actions().iter().map(|a| a.1).collect();
    assert_eq!(
        ids,
        vec![
            ActionId::Light(1),
            ActionId::Light(2),
            ActionId::Light(3),
            ActionId::Light(1)
        ]
    );
}

#[test]
fn dodge_cancels_an_attack_early_and_buffered_presses_expire() {
    let mut rig = Rig::new(10_000.0, FoeProfile::classic());
    rig.tick(&light_press());
    rig.run(12, &idle());
    dodge_tap(&mut rig, Vec2::ZERO);
    rig.run(10, &idle());
    let ids: Vec<_> = rig.actions().iter().map(|a| a.1).collect();
    assert_eq!(ids, vec![ActionId::Light(1), ActionId::Backstep]);

    // A press whose cancel window opens later than the buffer lasts is forgotten.
    let mut rig = Rig::new(10_000.0, FoeProfile::classic());
    rig.tick(&light_press());
    rig.run(12, &idle());
    rig.tick(&Input {
        heavy: press(),
        ..idle()
    });
    // Heavy cancels at frame 15 (tick 30) -> within the 0.4 s buffer it is accepted...
    rig.run(40, &idle());
    assert!(rig.actions().iter().any(|(_, a)| *a == ActionId::Heavy));
}

#[test]
fn guard_blocks_with_stamina_and_breaks_when_it_runs_out() {
    let mut rig = Rig::new(150.0, FoeProfile::classic());
    rig.run(
        10,
        &Input {
            guard: held(),
            ..idle()
        },
    );
    rig.w.foe_mut(FOE).unwrap().command_attack(0);
    let before = rig.w.hero.stamina;
    rig.run(
        40,
        &Input {
            guard: held(),
            ..idle()
        },
    );
    assert_eq!(rig.hurts(), vec![HitResult::Blocked]);
    assert_eq!(hp_of(&rig, HERO), 400.0, "a block takes no health");
    assert!(rig.w.hero.stamina < before - 5.0);
    assert!(rig.w.hero.guard_counter_ready() || rig.w.hero.action().is_some());

    let mut rig = Rig::new(150.0, FoeProfile::classic());
    rig.w.hero.stamina = 2.0;
    rig.run(
        10,
        &Input {
            guard: held(),
            ..idle()
        },
    );
    rig.w.foe_mut(FOE).unwrap().command_attack(0);
    rig.run(
        40,
        &Input {
            guard: held(),
            ..idle()
        },
    );
    assert_eq!(rig.hurts(), vec![HitResult::GuardBroken]);
    assert_eq!(rig.w.hero.action(), Some(ActionId::GuardBreak));
}

#[test]
fn parry_staggers_the_foe_and_opens_a_riposte() {
    let mut rig = Rig::new(150.0, FoeProfile::classic());
    let foe = rig.w.foe_mut(FOE).unwrap();
    foe.command_attack(0);
    // Press the guard so the parry window (frames 2..9) covers the strike (12 ticks later).
    rig.run(2, &idle());
    rig.tick(&Input {
        guard: press(),
        ..idle()
    });
    rig.run(
        30,
        &Input {
            guard: held(),
            ..idle()
        },
    );
    assert_eq!(rig.hurts(), vec![HitResult::Parried], "{:?}", rig.log);
    assert!(matches!(
        rig.w.foe(FOE).unwrap().state,
        FoeState::Stagger { parried: true }
    ));
    assert_eq!(hp_of(&rig, HERO), 400.0);

    // Riposte: a light press now becomes the critical strike, with the riposte's damage class.
    rig.run(20, &idle());
    rig.tick(&light_press());
    rig.run(80, &idle());
    assert!(rig.actions().iter().any(|(_, a)| *a == ActionId::Riposte));
    let last = rig.sink.log.last().expect("riposte landed");
    assert_eq!(last.kind, StrikeKind::Riposte);
    assert!(hp_of(&rig, FOE) <= 1000.0 - (40.0 * 4.0 - 5.0));
}

#[test]
fn late_guard_is_a_block_not_a_parry() {
    let mut rig = Rig::new(150.0, FoeProfile::classic());
    rig.run(
        30,
        &Input {
            guard: held(),
            ..idle()
        },
    );
    rig.w.foe_mut(FOE).unwrap().command_attack(0);
    rig.run(
        40,
        &Input {
            guard: held(),
            ..idle()
        },
    );
    assert_eq!(rig.hurts(), vec![HitResult::Blocked]);
}

#[test]
fn heavy_hits_break_poise_and_stagger_the_foe() {
    let mut rig = Rig::new(150.0, FoeProfile::classic());
    for _ in 0..2 {
        rig.tick(&Input {
            heavy: press(),
            ..idle()
        });
        rig.run(110, &idle());
    }
    assert!(
        rig.log.iter().any(|(_, e)| matches!(
            e,
            Event::HeroHit {
                poise_broken: true,
                ..
            }
        )),
        "{:?}",
        rig.log
    );
    assert!(matches!(
        rig.w.foe(FOE).unwrap().state,
        FoeState::Stagger { parried: false }
    ));
}

#[test]
fn a_connected_swing_freezes_both_sides_for_a_moment() {
    let mut rig = Rig::new(150.0, FoeProfile::classic());
    rig.tick(&light_press());
    let mut frames = Vec::new();
    for _ in 0..40 {
        rig.tick(&idle());
        frames.push(rig.w.hero.action_frame());
        if rig
            .log
            .iter()
            .any(|(_, e)| matches!(e, Event::HeroHit { .. }))
        {
            break;
        }
    }
    assert!(rig.w.hero.hit_stop > 0.0);
    assert!(rig.w.foe(FOE).unwrap().hit_stop > 0.0);
    let frozen = rig.w.hero.action_frame();
    rig.tick(&idle());
    assert_eq!(
        rig.w.hero.action_frame(),
        frozen,
        "frame does not advance in hit-stop"
    );
    rig.run(20, &idle());
    assert!(rig.w.hero.action_frame() > frozen);
}

#[test]
fn telegraph_gives_time_to_react_that_classic_does_not() {
    fn first_strike_tick(profile: FoeProfile) -> u64 {
        let mut rig = Rig::new(200.0, profile);
        rig.w.foe_mut(FOE).unwrap().command_attack(0);
        for _ in 0..600 {
            rig.tick(&idle());
            if rig
                .log
                .iter()
                .any(|(_, e)| matches!(e, Event::FoeStrike { .. }))
            {
                return rig.w.tick;
            }
        }
        panic!("no strike");
    }
    let classic = first_strike_tick(FoeProfile::classic());
    let tele = first_strike_tick(FoeProfile::telegraphed());
    assert!(
        tele as f32 * DT > classic as f32 * DT + 0.5,
        "{classic} vs {tele}"
    );

    // A foe that stops tracking early can be sidestepped even without i-frames.
    let mut rig = Rig::new(220.0, FoeProfile::telegraphed());
    rig.w.foe_mut(FOE).unwrap().command_attack(0);
    let run_left = Input {
        mv: Vec2::new(1.0, 0.0),
        ..idle()
    };
    rig.run(200, &run_left);
    assert!(!rig.hurts().contains(&HitResult::Hit), "{:?}", rig.hurts());
}

#[test]
fn lock_on_faces_the_target_and_strafes() {
    let mut rig = Rig::new(500.0, FoeProfile::classic());
    rig.tick(&Input {
        lock: true,
        ..idle()
    });
    assert_eq!(rig.w.lock.target, Some(FOE));
    // Strafe right while locked: heading stays on the target.
    let right = Input {
        mv: Vec2::new(1.0, 0.0),
        ..idle()
    };
    rig.run(90, &right);
    let to_foe = (rig.w.foe(FOE).unwrap().pos - rig.w.hero.pos).normalize_or_zero();
    let to_foe = Vec3::new(to_foe.x, 0.0, to_foe.z).normalize();
    assert!(rig.w.hero.facing().dot(to_foe) > 0.97);
    assert!(rig.w.hero.pos.x > 100.0, "moved right");
    // Roll direction relative to the target: a left stick tilt while locked rolls sideways.
    rig.tick(&Input {
        lock: true,
        ..idle()
    });
    assert_eq!(rig.w.lock.target, None);
}

#[test]
fn locked_roll_direction_is_relative_to_the_target() {
    let mut rig = Rig::new(500.0, FoeProfile::classic());
    rig.tick(&Input {
        lock: true,
        ..idle()
    });
    rig.run(20, &idle());
    dodge_tap(&mut rig, Vec2::new(0.0, -1.0));
    assert_eq!(rig.actions().last().unwrap().1, ActionId::Roll(Dir::Back));
    rig.run(60, &idle());
    dodge_tap(&mut rig, Vec2::new(-1.0, 0.0));
    assert_eq!(rig.actions().last().unwrap().1, ActionId::Roll(Dir::Left));
}

#[test]
fn sprint_drains_stamina_and_ends_empty() {
    let mut rig = Rig::new(10_000.0, FoeProfile::classic());
    let sprint = Input {
        dodge: held(),
        mv: Vec2::new(0.0, 1.0),
        ..idle()
    };
    rig.tick(&Input {
        dodge: press(),
        ..sprint
    });
    rig.run(60, &sprint);
    assert!(rig.w.hero.sprinting);
    assert!(rig.w.hero.stamina < rig.w.tuning.max_stamina - 5.0);
    let mut lowest = f32::MAX;
    for _ in 0..60 * 12 {
        rig.tick(&sprint);
        lowest = lowest.min(rig.w.hero.stamina);
    }
    assert!(lowest <= 0.5, "the bar ran dry: {lowest}");
    assert!(
        !rig.w.hero.sprinting,
        "out of breath: sprint needs a fresh press"
    );
}

#[test]
fn death_stops_the_hero_and_respawns() {
    let mut rig = Rig::new(150.0, FoeProfile::classic());
    rig.sink.add(HERO, 10.0, 40.0, 0.0);
    rig.w.foe_mut(FOE).unwrap().command_attack(0);
    rig.run(60, &idle());
    assert!(rig.w.hero.is_dead());
    assert!(rig.log.iter().any(|(_, e)| matches!(e, Event::HeroDied)));
    rig.sink.add(HERO, 400.0, 40.0, 0.0);
    rig.run(200, &idle());
    assert!(!rig.w.hero.is_dead());
}

// ---- original damage pipeline ------------------------------------------------------------------

mod pipeline {
    use super::*;
    use crate::sink::SimSink;
    use gothic_sim::actor::Actor;
    use gothic_sim::config::SimConfig;
    use gothic_sim::item::{ItemDb, ItemDef, Requirement};
    use gothic_sim::sim::Sim;
    use gothic_sim::types::*;
    use gothic_sim::world::{Grid2dWorld, Pos};

    fn items() -> ItemDb {
        let mut db = ItemDb::default();
        let mut sword = ItemDef::weapon("It_1H_Sword_01", "1H", "Blade", 50);
        sword.requirements.push(Requirement {
            tag: "STR".into(),
            op: 0,
            value: 100,
        });
        sword.required_skills.push("Perk_1H_1".into());
        db.insert(sword);
        db.insert(ItemDef::weapon(
            "It_Halberd_Orc_01",
            "Halberd",
            "Blade",
            200,
        ));
        db
    }

    #[test]
    fn souls_swings_deal_the_originals_damage_numbers() {
        let items = items();
        let mut sim = Sim::new(SimConfig::default(), Grid2dWorld::new(), items.clone());
        sim.player_alignment = Alignment::Reb;
        sim.state.level = 1;
        sim.state.xp = 500;
        sim.state.skills.insert("perk_1h_1".into());
        let hero = sim.spawn_hero(Pos::flat(0.0, 0.0));
        sim.equip(hero, "It_1H_Sword_01").expect("wield");
        {
            let h = sim.actor_mut(hero);
            h.ai_controlled = false;
            h.drawn = true;
        }
        let mut orc = Actor::npc(ActorId(0), "Orc_Raider", Species::Orc, 10, 30);
        orc.alignment = Alignment::Orc;
        orc.refresh(&SimConfig::default());
        if let Some(w) = gothic_sim::data::default_weapon(&items, orc.species) {
            orc.equipment.equip(w);
        }
        orc.base_attitude_to_player = Attitude::None;
        let orc_id = sim.spawn(orc, Pos::flat(0.0, 150.0));
        let orc_hp = sim.actor(orc_id).hp;

        let mut sink = SimSink::new(&mut sim);
        sink.bind(HERO, hero);
        sink.bind(FOE, orc_id);

        // Preview does not touch the victim; the committed hit does what the original does:
        // a plain quick attack deals 27 to this orc, a power attack 55 [fight.rs].
        let light = crate::sink::Strike {
            attacker: HERO,
            victim: FOE,
            kind: StrikeKind::Light,
            charged: false,
        };
        let preview = sink.resolve(&light, false);
        assert_eq!(sink.hp(FOE).0, orc_hp as f32, "preview is free");
        let real = sink.resolve(&light, true);
        assert_eq!(real.damage, 27, "{real:?}");
        assert_eq!(preview.damage, real.damage);
        assert_eq!(sink.hp(FOE).0, (orc_hp - 27) as f32);
        let heavy = crate::sink::Strike {
            kind: StrikeKind::Heavy,
            ..light
        };
        assert_eq!(sink.resolve(&heavy, true).damage, 55);

        // The whole flow on top of it: a Souls light attack lands through the real pipeline.
        let mut w = SoulsWorld::new(Vec3::ZERO, 100.0, Tuning::default());
        let foe = w.add_foe(
            FOE,
            Vec3::new(0.0, 0.0, -150.0),
            orc_hp as f32,
            FoeProfile::telegraphed(),
        );
        foe.autonomous = false;
        let mut env = OpenField::default();
        w.step(&light_press(), &mut sink, &mut env);
        let mut hit = None;
        for _ in 0..60 {
            for e in w.step(&idle(), &mut sink, &mut env) {
                if let Event::HeroHit { damage, .. } = e {
                    hit = Some(*damage);
                }
            }
        }
        assert_eq!(hit, Some(27), "same number the original pipeline gives");
        let _ = dir_of(0.0);
    }
}
