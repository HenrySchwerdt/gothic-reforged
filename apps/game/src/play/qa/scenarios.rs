//! The scenarios of `--play --qa <name>`; see the module comment of `super`.
use super::{Cx, Poll, Sc, err, expect, ok};
use bevy::prelude::*;
use gothic_runtime::{Command, CoreCommand};

pub const NAMES: &[&str] = &[
    "startup_sanity",
    "menu_save_load",
    "trade_click",
    "trade_esc",
    "lockpick",
    "sit_pose",
    "quick_slots",
    "teach",
    "loot_chest",
    "craft_anvil",
    "dialog_quest",
    "doors",
    "combat_basic",
    "melee_faithful",
    "bow_faithful",
    "magic_faithful",
    "orc_axes",
    "magic_bow",
    "controls_basic",
    "shaman_duel",
    "npc_orc1",
    "npc_orc2",
    "npc_orc3",
    "npc_shaman",
    "npc_archer",
    "npc_vs_npc",
    "bow_pose",
    "hud_focus",
    "souls_basic",
    "souls_er",
    "swim",
    "routine_reentry",
    "ui_screens",
    "sector_switch",
    "teleport",
    "walk_to_reddock",
    "world_walk",
];

pub fn build(name: &str) -> Option<Sc> {
    Some(match name {
        "startup_sanity" => startup_sanity(),
        "menu_save_load" => menu_save_load(),
        "trade_click" => super::more::trade_click(),
        "loot_chest" => super::more::loot_chest(),
        "lockpick" => super::more3::lockpick(),
        "sit_pose" => super::more3::sit_pose(),
        "quick_slots" => super::more3::quick_slots(),
        "teach" => super::more3::teach(),
        "doors" => super::more3::doors(),
        "trade_esc" => super::more::trade_esc(),
        "craft_anvil" => super::more::craft_anvil(),
        "dialog_quest" => super::more::dialog_quest(),
        "ui_screens" => super::more::ui_screens(),
        "combat_basic" => super::more2::combat_basic(),
        "melee_faithful" => super::melee::melee_faithful(),
        "bow_faithful" => super::bow::bow_faithful(),
        "magic_faithful" => super::magic::magic_faithful(),
        "orc_axes" => super::melee::orc_axes(),
        "souls_basic" => super::more2::souls_basic(),
        "souls_er" => super::more2::souls_er(),
        "magic_bow" => super::more2::magic_bow(),
        "controls_basic" => super::ranged::controls_basic(),
        "bow_pose" => super::ranged::bow_pose(),
        "shaman_duel" => super::ranged::shaman_duel(),
        "npc_orc1" => super::npc_ai::npc_orc1(),
        "npc_orc2" => super::npc_ai::npc_orc2(),
        "npc_orc3" => super::npc_ai::npc_orc3(),
        "npc_shaman" => super::npc_ai::npc_shaman(),
        "npc_archer" => super::npc_ai::npc_archer(),
        "npc_vs_npc" => super::npc_ai::npc_vs_npc(),
        "hud_focus" => super::ranged::hud_focus(),
        "swim" => super::more2::swim(),
        "routine_reentry" => super::more2::routine_reentry(),
        "sector_switch" => super::world::sector_switch(),
        "teleport" => super::world::teleport(),
        "walk_to_reddock" => super::world::walk_to_reddock(),
        "world_walk" => super::world::world_walk(),
        "triggers" => super::world::triggers(),
        "perf_idle" => super::world::perf_idle(),
        _ => return None,
    })
}

/// A cheap fingerprint of the observable persistent state (gold, items, quests, hero pose).
pub fn fingerprint(cx: &mut Cx) -> String {
    let p = cx.hero_pos();
    let c = cx.core();
    let hero = c.0.sim.actor(c.0.hero);
    let items: u32 = hero
        .inventory
        .stacks()
        .iter()
        .map(|s| s.amount.max(0) as u32)
        .sum();
    let running =
        c.0.state()
            .quests
            .keys()
            .filter(|q| c.0.state().quest_status(q) == gothic_gameplay::model::QuestStatus::Running)
            .count();
    format!(
        "gold={} items={items} stacks={} hp={} quests_running={running} pos=({:.0},{:.0},{:.0})",
        hero.inventory.count("It_Gold"),
        hero.inventory.stacks().len(),
        hero.hp,
        p.x / 10.0 * 10.0,
        p.y / 10.0 * 10.0,
        p.z / 10.0 * 10.0,
    )
}

/// Teleport / state set-up helpers (marked as set-up in the log).
pub fn grant(cx: &mut Cx, template: &str, amount: u32) {
    cx.note(format!("setup: grant {amount} x {template}"));
    cx.world.write_message(CoreCommand(Command::GiveHero {
        template: template.into(),
        amount,
    }));
}

fn startup_sanity() -> Sc {
    let mut sc = Sc::new(120.0);
    sc.act("baseline", |cx| {
        let hp = cx.hero_hp();
        cx.st.n.insert("hp0".into(), f64::from(hp));
        cx.st.n.insert("hp_min".into(), f64::from(hp));
        let me = cx.core().0.hero;
        let hero = cx.hero_pos();
        let mut rows: Vec<(f32, String, String)> = Vec::new();
        for a in &cx.core().0.sim.actors {
            if a.id == me || !a.is_alive() {
                continue;
            }
            if let Some(p) = cx.actor_pos(a.id) {
                let d = Vec3::new(p.x - hero.x, 0.0, p.z - hero.z).length() * 0.01;
                rows.push((d, a.name.clone(), format!("{:?}", a.species)));
            }
        }
        rows.sort_by(|a, b| a.0.total_cmp(&b.0));
        let humans = rows.iter().filter(|r| r.2 == "Human").count();
        let creatures = rows.len() - humans;
        cx.note(format!(
            "actors alive: {} ({} human, {} creatures); nearest 12: {}",
            rows.len(),
            humans,
            creatures,
            rows.iter()
                .take(12)
                .map(|r| format!("{} {}@{:.0}m", r.2, r.1, r.0))
                .collect::<Vec<_>>()
                .join(", ")
        ));
        let within = |m: f32| rows.iter().filter(|r| r.0 < m && r.2 != "Human").count();
        cx.note(format!(
            "creatures within 25 m: {}, 50 m: {}, 100 m: {}",
            within(25.0),
            within(50.0),
            within(100.0)
        ));
        // Creatures inside the camera's field of view (horizontal 90 degrees) within 60 m.
        let yaw = cx.player().yaw;
        let fwd = Vec2::new(-yaw.sin(), -yaw.cos());
        let in_view: Vec<String> = cx
            .core()
            .0
            .sim
            .actors
            .iter()
            .filter(|a| {
                a.id != me && a.is_alive() && a.species != gothic_sim::types::Species::Human
            })
            .filter_map(|a| {
                let p = cx.actor_pos(a.id)?;
                let d = Vec2::new(p.x - hero.x, p.z - hero.z);
                (d.length() < 6000.0 && d.normalize_or_zero().dot(fwd) > 0.7)
                    .then(|| format!("{:?}:{}@{:.0}m", a.species, a.name, d.length() * 0.01))
            })
            .collect();
        cx.note(format!(
            "creatures in view (60 m, 90 deg): {}",
            in_view.len()
        ));
        for v in in_view {
            cx.note(format!("  in view: {v}"));
        }
    });
    sc.shot("start");
    sc.watch(|cx| {
        let hp = f64::from(cx.hero_hp());
        let min = cx.st.n.entry("hp_min".into()).or_insert(hp);
        if hp < *min {
            *min = hp;
        }
        // Who targets the hero, and when?
        let me = cx.core().0.hero;
        let attackers: Vec<String> = cx
            .core()
            .0
            .sim
            .actors
            .iter()
            .filter(|a| a.id != me && a.is_alive() && a.target == Some(me))
            .map(|a| a.name.clone())
            .collect();
        if !attackers.is_empty() {
            let seen = cx.st.store.entry("attackers".into()).or_default();
            for a in attackers {
                if !seen.contains(&a) {
                    seen.push_str(&format!("{a}; "));
                }
            }
        }
    });
    sc.wait(30.0);
    sc.shot("t30");
    sc.check("hero hp unchanged after 30 s", |cx| {
        let hp0 = cx.st.n["hp0"] as i32;
        let min = cx.st.n["hp_min"] as i32;
        expect(
            min >= hp0,
            format!("hp {hp0} -> min {min}"),
            format!("hero lost hp: {hp0} -> {min}"),
        )
    });
    sc.check("nobody targets the hero in the first 30 s", |cx| {
        let a = cx.st.store.get("attackers").cloned().unwrap_or_default();
        if a.is_empty() {
            ok("no attacker")
        } else {
            err(format!("targeting hero: {a}"))
        }
    });
    sc.check(
        "original nav graph loaded and routes hero -> Hamlar",
        |cx| {
            let nav = cx
                .world
                .resource::<crate::walk::play::nav::NavRes>()
                .0
                .clone();
            let Some(g) = nav.graph() else {
                return err("no graph installed after 30 s");
            };
            let hero = cx.hero_pos();
            let h = cx
                .core()
                .0
                .find_name("Hamlar")
                .and_then(|a| cx.actor_pos(a));
            let Some(h) = h else { return err("no Hamlar") };
            match nav.graph_route(hero, h) {
                Some(r) => ok(format!(
                    "{} zones, {} corridors; hero->Hamlar {} points {:.0} cm",
                    g.zone_count(),
                    g.path_count(),
                    r.points.len(),
                    r.length
                )),
                None => err(format!(
                    "graph has {} zones but no hero->Hamlar route (hero in zone: {})",
                    g.zone_count(),
                    g.locate_near(hero, 60.0).is_some()
                )),
            }
        },
    );
    sc.check("hero not down", |cx| {
        expect(!cx.play().hero_down, "up", "hero is down")
    });
    sc
}

fn menu_save_load() -> Sc {
    let mut sc = Sc::new(180.0);
    sc.wait(2.0);
    sc.act(
        "click once to grab the mouse (original: the game owns the mouse)",
        |cx| {
            cx.cursor_to(Vec2::new(640.0, 360.0));
            cx.mouse_click(MouseButton::Left);
        },
    );
    sc.wait(0.5);
    sc.act("remember slot count", |cx| {
        let n = std::fs::read_dir(".local/saves").map_or(0, |d| d.count());
        cx.st.n.insert("slots0".into(), n as f64);
        let fp = fingerprint(cx);
        cx.st.store.insert("fp0".into(), fp.clone());
        cx.note(format!("start state {fp}"));
        cx.tap(KeyCode::Escape);
    });
    sc.until("main menu open", 5.0, |cx| {
        cx.ui().screen == gothic_ui::Screen::MainMenu
    });
    sc.wait(0.5);
    sc.act("list menu", |cx| {
        let b: Vec<String> = cx.buttons().iter().map(|b| b.text.clone()).collect();
        cx.note(format!("menu buttons: {b:?}"));
    });
    sc.shot("menu");
    super::click_step(&mut sc, "Spiel speichern", |cx| {
        cx.buttons()
            .iter()
            .find(|b| b.text.contains("speichern"))
            .map(|b| b.center)
    });
    sc.until("files page (save)", 5.0, |cx| {
        cx.ui().screen == gothic_ui::Screen::Files
    });
    sc.wait(0.5);
    sc.shot("files_save");
    super::click_step(&mut sc, "Neuer Spielstand row", |cx| {
        cx.buttons()
            .iter()
            .find(|b| b.text.contains("Neuer Spielstand"))
            .map(|b| b.center)
    });
    sc.act("list buttons", |cx| {
        let b: Vec<String> = cx.buttons().iter().map(|b| b.text.clone()).collect();
        cx.note(format!("files buttons: {b:?}"));
    });
    super::click_step(&mut sc, "Speichern", |cx| {
        cx.buttons()
            .iter()
            .find(|b| b.text == "Speichern")
            .map(|b| b.center)
    });
    sc.wait(1.5);
    sc.shot("after_save");
    sc.check("a new save file exists", |cx| {
        let n0 = cx.st.n["slots0"] as usize;
        let n = std::fs::read_dir(".local/saves").map_or(0, |d| d.count());
        expect(
            n > n0,
            format!("{n0} -> {n} files"),
            format!("still {n} files"),
        )
    });
    sc.act("save state hash", |cx| {
        let h = cx.state_hash();
        cx.st.store.insert("hash_saved".into(), h.to_string());
        let fp = fingerprint(cx);
        cx.st.store.insert("fp_saved".into(), fp);
    });
    // Leave the menu with Esc, as many times as it takes.
    sc.act("Esc (back)", |cx| cx.tap(KeyCode::Escape));
    sc.wait(0.5);
    sc.act("Esc (close)", |cx| {
        if cx.ui().screen != gothic_ui::Screen::None {
            cx.tap(KeyCode::Escape);
        }
    });
    sc.until("menu closed", 5.0, |cx| {
        cx.ui().screen == gothic_ui::Screen::None
    });
    sc.act("grab mouse again", |cx| {
        cx.mouse_click(MouseButton::Left);
    });
    sc.wait(0.3);
    sc.act(
        "change the state by playing: run forward, pick up gold via setup",
        |cx| {
            grant(cx, "It_Gold", 321);
            cx.key_down(KeyCode::KeyW);
        },
    );
    sc.wait(3.0);
    sc.act("stop", |cx| cx.key_up(KeyCode::KeyW));
    sc.wait(0.5);
    sc.check("state changed after the save", |cx| {
        let fp = fingerprint(cx);
        let saved = cx.st.store["fp_saved"].clone();
        expect(fp != saved, fp.clone(), format!("unchanged: {fp}"))
    });
    sc.act("Esc", |cx| cx.tap(KeyCode::Escape));
    sc.until("main menu open (2)", 5.0, |cx| {
        cx.ui().screen == gothic_ui::Screen::MainMenu
    });
    sc.wait(0.4);
    super::click_step(&mut sc, "Spiel laden", |cx| {
        cx.buttons()
            .iter()
            .find(|b| b.text.contains("laden"))
            .map(|b| b.center)
    });
    sc.until("files page (load)", 5.0, |cx| {
        cx.ui().screen == gothic_ui::Screen::Files
    });
    sc.wait(0.5);
    sc.shot("files_load");
    super::click_step(&mut sc, "newest slot row", |cx| {
        cx.buttons()
            .iter()
            .find(|b| b.text.contains("Stufe"))
            .map(|b| b.center)
    });
    sc.shot("slot_selected");
    super::click_step(&mut sc, "Laden", |cx| {
        cx.buttons()
            .iter()
            .find(|b| b.text == "Laden")
            .map(|b| b.center)
    });
    sc.wait(2.0);
    sc.shot("after_load");
    sc.check("state after load equals the saved state", |cx| {
        let fp = fingerprint(cx);
        let saved = cx.st.store["fp_saved"].clone();
        let h = cx.state_hash().to_string();
        let hs = cx.st.store["hash_saved"].clone();
        cx.note(format!("hash saved {hs} / after load {h}"));
        expect(
            fp == saved,
            format!("{fp}"),
            format!("saved {saved} / now {fp}"),
        )
    });
    sc.check("UI closed after load", |cx| {
        let s = cx.ui().screen;
        expect(
            s == gothic_ui::Screen::None,
            "world visible",
            format!("screen still {s:?}"),
        )
    });
    let _ = Poll::Done;
    sc
}
