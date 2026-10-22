//! More scenarios: dialog, trade, loot, crafting, UI screens.
use super::scenarios::{fingerprint, grant};
use super::{Cx, Poll, Sc, err, expect, ok};
use crate::walk::play::events::Voices;
use crate::walk::play::interact::Interaction;
use crate::walk::play::{Mode, Target};
use bevy::prelude::*;
use gothic_sim::types::ActorId;
use std::sync::Arc;

pub(super) type Goal = Arc<dyn Fn(&mut Cx) -> Option<Vec3> + Send + Sync>;
pub(super) type Pred = Arc<dyn Fn(&mut Cx) -> bool + Send + Sync>;

const DIGITS: [KeyCode; 9] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
];

pub fn npc_pos(name: &'static str) -> Goal {
    Arc::new(move |cx| {
        let id = cx.core().0.find_name(name)?;
        cx.actor_pos(id)
    })
}

pub fn npc_id(cx: &Cx, name: &str) -> Option<ActorId> {
    cx.core().0.find_name(name)
}

/// Walk to `goal` (running, steering with mouse motion) and look at it until `ok` holds.
pub fn approach(sc: &mut Sc, label: &str, goal: Goal, stop: f32, aim_pitch: f32, ok: Pred) {
    let g = goal.clone();
    let l = label.to_string();
    sc.step(format!("walk to {label}"), move |cx| {
        let Some(p) = g(cx) else {
            return if cx.t > 15.0 {
                Poll::Fail(format!("{l}: no such target in the world"))
            } else {
                Poll::Wait
            };
        };
        if cx.t > 120.0 {
            cx.release_all();
            let hero = cx.player().ctrl.position;
            cx.note(format!(
                "stuck walking to {l}: hero at {hero:?}, goal {p:?}, distance {:.0} cm, grounded {}",
                (p - hero).length(),
                cx.player().ctrl.grounded
            ));
            cx.shot("stuck");
            return Poll::Fail(format!("could not reach {l} in 120 s"));
        }
        let k = (cx.t / 15.0) as u32;
        if k > 0 && cx.st.n.insert(format!("walk_log_{l}"), f64::from(k)) != Some(f64::from(k)) {
            let hero = cx.player().ctrl.position;
            eprintln!(
                "qa: walking to {l}: hero {hero:?}, goal {p:?}, dist {:.0}, detour {:.2}, hp {}",
                (p - hero).length(),
                cx.st.walk_detour,
                cx.hero_hp()
            );
        }
        // Steer along the original navigation graph / grid (what the NPCs use), straight when it
        // has no route.
        let hero = cx.player().ctrl.position;
        let nav = cx.world.resource::<crate::walk::play::nav::NavRes>().0.clone();
        let steer = nav
            .path(hero, p)
            .and_then(|path| {
                path.points
                    .iter()
                    .copied()
                    .find(|q| Vec2::new(q.x - hero.x, q.z - hero.z).length() > 180.0)
            })
            .unwrap_or(p);
        let polled = cx.walk_to(steer, 60.0, true);
        let flat = Vec2::new(p.x - hero.x, p.z - hero.z).length();
        if flat <= stop {
            cx.release_all();
            return Poll::Done;
        }
        if matches!(polled, Poll::Done) && flat > stop {
            // reached a waypoint: the next frame picks the next one
            return Poll::Wait;
        }
        polled
    });
    let l = label.to_string();
    sc.step(format!("focus {label}"), move |cx| {
        if let Some(p) = goal(cx) {
            cx.aim_at(p, aim_pitch);
        }
        if ok(cx) {
            Poll::Done
        } else if cx.t > 10.0 {
            Poll::Fail(format!("{l} never became the focus"))
        } else {
            Poll::Wait
        }
    });
}

/// The talk key (Enter) on the focused NPC, then the dialog opens.
pub fn start_dialog(sc: &mut Sc) {
    sc.act("use key (left click)", |cx| cx.use_key());
    sc.until("dialog mode", 6.0, |cx| cx.play().mode == Mode::Dialog);
}

/// Wait for the NPC's lines to be spoken and the option menu to be there.
pub fn wait_menu(sc: &mut Sc) {
    let mut since: Option<f32> = None;
    sc.step("menu ready", move |cx| {
        let v = cx.world.resource::<Voices>();
        if v.idle() && !v.menu.is_empty() {
            let s = *since.get_or_insert(cx.now);
            if cx.now - s > 0.6 {
                since = None;
                return Poll::Done;
            }
        } else {
            since = None;
        }
        if cx.t > 40.0 {
            return Poll::Fail("dialog menu never appeared".into());
        }
        Poll::Wait
    });
}

/// Picks the first menu option whose label contains one of `needles` with the keyboard: the
/// arrow keys move the selection, Enter chooses (what a player without a mouse does).
pub fn pick_option(sc: &mut Sc, needles: &'static [&'static str]) {
    let mut idx: Option<usize> = None;
    let mut next_at = 0.0f32;
    sc.step(format!("pick option {needles:?}"), move |cx| {
        if idx.is_none() {
            let labels: Vec<String> = cx
                .world
                .resource::<Voices>()
                .menu
                .iter()
                .map(|c| c.label.clone())
                .collect();
            cx.note(format!("menu: {labels:?}"));
            idx = needles.iter().find_map(|n| {
                labels
                    .iter()
                    .position(|l| l.to_lowercase().contains(&n.to_lowercase()))
            });
            if idx.is_none() {
                cx.check(
                    "dialog option available",
                    false,
                    format!("none of {needles:?} in {labels:?}"),
                );
                return Poll::Fail("option missing".into());
            }
        }
        if cx.now < next_at {
            return Poll::Wait;
        }
        next_at = cx.now + 0.08;
        let want = idx.unwrap_or(0);
        let sel = cx.ui().options.selected().unwrap_or(0);
        if sel < want {
            cx.tap(KeyCode::ArrowDown);
        } else if sel > want {
            cx.tap(KeyCode::ArrowUp);
        } else {
            cx.tap(KeyCode::Enter);
            return Poll::Done;
        }
        if cx.t > 20.0 {
            return Poll::Fail("could not move the option selection".into());
        }
        Poll::Wait
    });
}

fn win_size(cx: &mut Cx) -> Vec2 {
    cx.world
        .query_filtered::<&Window, With<bevy::window::PrimaryWindow>>()
        .iter(cx.world)
        .next()
        .map_or(Vec2::new(1280.0, 720.0), |w| {
            Vec2::new(w.width(), w.height())
        })
}

/// Centres of the 44 x 44 item cells (any list page), left / right half of the window.
fn cells(cx: &mut Cx, left: bool) -> Vec<Vec2> {
    let w = win_size(cx);
    let btns = cx.buttons();
    // Cells are 44 design pixels wide; the UI scales with the window height / 768.
    let want = 44.0 * w.y / 768.0;
    let mut v: Vec<Vec2> = btns
        .iter()
        .filter(|b| (b.size.x - want).abs() < want * 0.2 && (b.size.y - want).abs() < want * 0.2)
        .filter(|b| (b.center.x < w.x * 0.5) == left)
        .map(|b| b.center)
        .collect();
    v.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)));
    v
}

/// Opening the trade page with Hamlar and pressing Enter / Esc without any basket: the page
/// must close again.
pub fn trade_esc() -> Sc {
    let mut sc = Sc::new(240.0);
    trade_open(&mut sc);
    super::click_step(&mut sc, "vendor cell", |cx| {
        cells(cx, true).first().copied()
    });
    sc.act("Enter with a basket that is not balanced", |cx| {
        cx.tap(KeyCode::Enter)
    });
    sc.wait(0.8);
    sc.shot("enter_empty");
    sc.act("Esc", |cx| cx.tap(KeyCode::Escape));
    sc.wait(1.2);
    sc.shot("esc");
    sc.check("Esc closes the trade page (window and screen)", |cx| {
        let s = cx.ui().screen;
        expect(
            cx.inter().window.is_none() && s == gothic_ui::Screen::None,
            "closed",
            format!("window open {} screen {s:?}", cx.inter().window.is_some()),
        )
    });
    sc
}

fn trade_open(sc: &mut Sc) {
    let sc = &mut *sc;
    sc.wait(1.0);
    sc.act("grab the mouse with a click", |cx| {
        cx.cursor_to(Vec2::new(640.0, 360.0));
        cx.mouse_click(MouseButton::Left);
    });
    sc.act("setup: 500 gold so the hero can buy", |cx| {
        grant(cx, "It_Gold", 500)
    });
    sc.wait(0.5);
    approach(
        sc,
        "Hamlar",
        npc_pos("Hamlar"),
        200.0,
        -0.2,
        Arc::new(|cx| {
            npc_id(cx, "Hamlar").is_some_and(|id| cx.play().target == Some(Target::Npc(id)))
        }),
    );
    start_dialog(sc);
    wait_menu(sc);
    sc.shot("dialog");
    sc.act(
        "setup: Hamlar's start dialog has no trade option (the rebel traders are story-disabled); raise the same host request the DoTrade script command raises",
        |cx| {
            let id = npc_id(cx, "Hamlar");
            cx.world.resource_mut::<Interaction>().pending_trade = id;
        },
    );
    sc.until("trade page open", 15.0, |cx| {
        matches!(
            cx.inter().window.as_ref().map(|w| &w.kind),
            Some(crate::walk::play::interact::WindowKind::Trade { .. })
        ) && cx.ui().screen == gothic_ui::Screen::Trade
    });
    sc.wait(0.8);
    sc.shot("trade_open");
}

pub fn trade_click() -> Sc {
    let mut sc = Sc::new(240.0);
    trade_open(&mut sc);
    sc.act("record before", |cx| {
        let g = cx.gold();
        let items = cx
            .core()
            .0
            .sim
            .actor(cx.core().0.hero)
            .inventory
            .stacks()
            .len();
        cx.st.n.insert("gold0".into(), f64::from(g));
        cx.st.n.insert("stacks0".into(), items as f64);
        let b: Vec<String> = cx
            .buttons()
            .iter()
            .filter(|b| !b.text.is_empty())
            .map(|b| b.text.clone())
            .collect();
        cx.note(format!("trade page labelled buttons: {b:?}"));
        let c = cells(cx, true).len();
        let p = cells(cx, false).len();
        cx.note(format!("cells: vendor {c}, player {p}; gold {g}"));
    });
    // Click the first affordable-looking vendor cell: cell order is the vendor's stack order.
    super::click_step(&mut sc, "vendor cell", |cx| {
        cells(cx, true).first().copied()
    });
    sc.wait(0.4);
    sc.shot("basket");
    sc.check("a vendor click puts an item in the ask basket", |cx| {
        let Some(crate::walk::play::interact::WindowKind::Trade { npc }) =
            cx.inter().window.as_ref().map(|w| w.kind.clone())
        else {
            return err("trade window gone");
        };
        let t = cx.core_mut().0.sim.trade_totals(npc);
        expect(
            t.ask > 0,
            format!("ask total {}", t.ask),
            "ask total 0 after the click",
        )
    });
    super::click_step(&mut sc, "Gold-Ausgleich", |cx| {
        cx.buttons()
            .iter()
            .find(|b| b.text.contains("Ausgleich"))
            .map(|b| b.center)
    });
    sc.wait(0.5);
    sc.shot("balanced");
    super::click_step(&mut sc, "Tauschen (deal)", |cx| {
        cx.buttons()
            .iter()
            .find(|b| b.text.contains("Tauschen"))
            .map(|b| b.center)
    });
    sc.wait(1.0);
    sc.shot("after_deal");
    sc.check("deal changed gold or items", |cx| {
        let g0 = cx.st.n["gold0"] as u32;
        let s0 = cx.st.n["stacks0"] as usize;
        let g = cx.gold();
        let s = cx
            .core()
            .0
            .sim
            .actor(cx.core().0.hero)
            .inventory
            .stacks()
            .len();
        expect(
            g != g0 || s != s0,
            format!("gold {g0} -> {g}, stacks {s0} -> {s}"),
            format!("nothing changed: gold {g0} -> {g}, stacks {s0} -> {s}"),
        )
    });
    sc.act("Esc closes the page", |cx| cx.tap(KeyCode::Escape));
    sc.wait(1.0);
    sc.check("page closed and the world is usable again", |cx| {
        let s = cx.ui().screen;
        expect(
            cx.inter().window.is_none() && s == gothic_ui::Screen::None,
            "closed",
            format!("window {:?} screen {s:?}", cx.inter().window.is_some()),
        )
    });
    sc
}

/// Open the nearest unlocked chest with Enter, take the contents with clicks.
pub fn loot_chest() -> Sc {
    let mut sc = Sc::new(240.0);
    sc.wait(1.0);
    sc.act("grab the mouse", |cx| {
        cx.cursor_to(Vec2::new(640.0, 360.0));
        cx.mouse_click(MouseButton::Left);
    });
    let goal: Goal = Arc::new(|cx| {
        let hero = cx.hero_pos();
        let o = cx
            .world
            .resource::<crate::walk::play::objects::WorldObjects>();
        o.list
            .iter()
            .filter(|o| {
                o.entity.is_some()
                    && o.def.script_use() == gothic_sim::script::enums::UseType::Chest
                    && o.def.lock.as_ref().is_none_or(|l| !l.locked)
            })
            .min_by(|a, b| {
                (a.pos_cm - hero)
                    .length()
                    .total_cmp(&(b.pos_cm - hero).length())
            })
            .map(|o| o.pos_cm)
    });
    approach(
        &mut sc,
        "chest",
        goal,
        140.0,
        -0.3,
        Arc::new(|cx| matches!(cx.play().target, Some(Target::Object(_)))),
    );
    sc.act("record inventory", |cx| {
        let n: u32 = cx
            .core()
            .0
            .sim
            .actor(cx.core().0.hero)
            .inventory
            .stacks()
            .iter()
            .map(|s| s.amount.max(0) as u32)
            .sum();
        cx.st.n.insert("items0".into(), f64::from(n));
        cx.use_key();
    });
    sc.until("loot page", 10.0, |cx| {
        cx.ui().screen == gothic_ui::Screen::Loot
    });
    sc.wait(1.0);
    sc.shot("loot_open");
    sc.act("list", |cx| {
        let b: Vec<String> = cx
            .buttons()
            .iter()
            .filter(|b| !b.text.is_empty())
            .map(|b| b.text.clone())
            .collect();
        cx.note(format!("loot labelled buttons: {b:?}"));
        let n = cx.inter().window.as_ref().map_or(0, |w| w.lines.len());
        cx.note(format!("window lines: {n}"));
    });
    super::click_step(&mut sc, "first loot row/cell", |cx| {
        let w = win_size(cx);
        let want = 44.0 * w.y / 768.0;
        let btns = cx.buttons();
        // list rows carry text; the left page lists the container
        btns.iter()
            .find(|b| (b.size.x - want).abs() < want * 0.2 && b.center.x < w.x * 0.5)
            .map(|b| b.center)
            .or_else(|| {
                btns.iter()
                    .find(|b| b.text.contains(" x "))
                    .map(|b| b.center)
            })
    });
    sc.wait(1.0);
    sc.shot("loot_taken");
    sc.check("clicking a loot entry moved it to the inventory", |cx| {
        let n: u32 = cx
            .core()
            .0
            .sim
            .actor(cx.core().0.hero)
            .inventory
            .stacks()
            .iter()
            .map(|s| s.amount.max(0) as u32)
            .sum();
        let n0 = cx.st.n["items0"] as u32;
        expect(
            n > n0,
            format!("items {n0} -> {n}"),
            format!("items still {n}"),
        )
    });
    sc.act("Esc", |cx| cx.tap(KeyCode::Escape));
    sc.wait(1.5);
    sc.check("window closed", |cx| {
        let grabbed = cx.cursor_grabbed();
        expect(
            cx.inter().window.is_none() && cx.ui().screen == gothic_ui::Screen::None,
            "closed",
            format!(
                "screen {:?}, window open {}, page_shown {}, window_close flag {}, grabbed {}",
                cx.ui().screen,
                cx.inter().window.is_some(),
                cx.inter().page_shown,
                cx.inter().window_close,
                grabbed
            ),
        )
    });
    sc
}

pub fn craft_anvil() -> Sc {
    let mut sc = Sc::new(240.0);
    sc.wait(1.0);
    sc.act("grab the mouse", |cx| {
        cx.cursor_to(Vec2::new(640.0, 360.0));
        cx.mouse_click(MouseButton::Left);
    });
    sc.act(
        "setup: hammer, blueprint and ingredients (the quests would hand them out)",
        |cx| {
            let (bp, perk, ingredients) = {
                let o = cx
                    .world
                    .resource::<crate::walk::play::objects::WorldObjects>();
                let bp = o
                    .recipe_cat
                    .iter()
                    .filter(|(_, c)| **c == gothic_sim::script::player::api::SkillCategory::Smith)
                    .map(|(k, _)| k.clone())
                    .min();
                let r = bp
                    .as_ref()
                    .and_then(|b| o.recipe_ids.get(b))
                    .and_then(|id| cx.core().0.sim.script.recipe_of(*id).cloned());
                (
                    bp,
                    r.as_ref()
                        .map(|r| r.required_perk.clone())
                        .unwrap_or_default(),
                    r.map(|r| r.ingredients.clone()).unwrap_or_default(),
                )
            };
            grant(cx, "It_Hammer_Smith", 1);
            if let Some(bp) = bp {
                grant(cx, &bp, 1);
                cx.st.store.insert("blueprint".into(), bp);
            }
            if !perk.is_empty() {
                cx.world.write_message(gothic_runtime::CoreCommand(
                    gothic_runtime::Command::GrantPerk(perk),
                ));
            }
            for (item, n) in ingredients {
                grant(cx, &item, n.max(0) as u32);
            }
        },
    );
    sc.wait(0.6);
    let goal: Goal = Arc::new(|cx| {
        let o = cx
            .world
            .resource::<crate::walk::play::objects::WorldObjects>();
        o.list
            .iter()
            .find(|o| {
                o.entity.is_some()
                    && o.def.script_use() == gothic_sim::script::enums::UseType::Anvil
            })
            .map(|o| o.pos_cm)
    });
    approach(
        &mut sc,
        "anvil",
        goal,
        140.0,
        -0.3,
        Arc::new(|cx| matches!(cx.play().target, Some(Target::Object(_)))),
    );
    sc.act("use key", |cx| cx.use_key());
    sc.until("recipe page", 10.0, |cx| {
        cx.ui().screen == gothic_ui::Screen::Recipe
    });
    sc.wait(1.0);
    sc.shot("recipe_page");
    sc.act("record", |cx| {
        let b: Vec<String> = cx.buttons().iter().map(|b| b.text.clone()).collect();
        cx.note(format!("recipe page buttons: {b:?}"));
        let n: u32 = cx
            .core()
            .0
            .sim
            .actor(cx.core().0.hero)
            .inventory
            .stacks()
            .iter()
            .map(|s| s.amount.max(0) as u32)
            .sum();
        cx.st.n.insert("items0".into(), f64::from(n));
        let fp = fingerprint(cx);
        cx.note(fp);
    });
    super::click_step(&mut sc, "recipe row", |cx| {
        let b = cx.buttons();
        b.iter()
            .find(|b| {
                let t = b.text.to_lowercase();
                !t.contains("herstellen") && !t.contains("schlie") && !t.contains("zur")
            })
            .map(|b| b.center)
    });
    sc.wait(0.5);
    sc.shot("recipe_selected");
    super::click_step(&mut sc, "craft button", |cx| {
        cx.buttons()
            .iter()
            .find(|b| {
                let t = b.text.to_lowercase();
                t.contains("herstellen") || t.contains("schmieden") || t.contains("craft")
            })
            .map(|b| b.center)
    });
    sc.wait(1.5);
    sc.shot("crafted");
    sc.check("a crafted item arrived (log has Crafted)", |cx| {
        let made = cx.inter().log.iter().any(|l| l.contains("Crafted"));
        expect(
            made,
            "crafted",
            format!(
                "log: {:?}",
                cx.inter().log.iter().rev().take(5).collect::<Vec<_>>()
            ),
        )
    });
    sc.act("Esc", |cx| cx.tap(KeyCode::Escape));
    sc.wait(1.5);
    sc.check("window closed", |cx| {
        let grabbed = cx.cursor_grabbed();
        expect(
            cx.inter().window.is_none() && cx.ui().screen == gothic_ui::Screen::None,
            "closed",
            format!(
                "screen {:?}, window open {}, page_shown {}, window_close flag {}, grabbed {}",
                cx.ui().screen,
                cx.inter().window.is_some(),
                cx.inter().page_shown,
                cx.inter().window_close,
                grabbed
            ),
        )
    });
    sc
}

pub fn dialog_quest() -> Sc {
    let mut sc = Sc::new(420.0);
    sc.wait(1.0);
    sc.act("grab the mouse", |cx| {
        cx.cursor_to(Vec2::new(640.0, 360.0));
        cx.mouse_click(MouseButton::Left);
    });
    sc.act("baseline quests", |cx| {
        let fp = fingerprint(cx);
        cx.st.store.insert("fp0".into(), fp);
    });
    for name in ["Diego"] {
        approach(
            &mut sc,
            name,
            npc_pos(name),
            200.0,
            -0.2,
            Arc::new(move |cx| {
                npc_id(cx, name).is_some_and(|id| cx.play().target == Some(Target::Npc(id)))
            }),
        );
        start_dialog(&mut sc);
        wait_menu(&mut sc);
        sc.shot(&format!("dialog_{name}"));
        // The first entry that is neither a teacher request nor the exit.
        sc.act(format!("log menu of {name}"), |cx| {
            let labels: Vec<String> = cx
                .world
                .resource::<Voices>()
                .menu
                .iter()
                .map(|c| c.label.clone())
                .collect();
            cx.note(format!("menu: {labels:?}"));
        });
        pick_first(&mut sc);
        sc.wait(1.0);
        sc.until("dialog goes on or ends", 60.0, |cx| {
            cx.play().mode == Mode::Explore || {
                let v = cx.world.resource::<Voices>();
                v.idle() && !v.menu.is_empty()
            }
        });
        sc.act("note the hero position after the dialog", |cx| {
            let p = cx.hero_pos();
            cx.note(format!("hero after dialog: {p:?}"));
        });
        sc.act("leave", |cx| {
            if cx.play().mode == Mode::Dialog {
                // the exit entry is the last one of the menu
                cx.st.store.insert("want_exit".into(), "1".into());
            }
        });
        pick_exit(&mut sc);
        sc.until("explore mode", 40.0, |cx| cx.play().mode == Mode::Explore);
    }
    sc.act("L", |cx| cx.tap(KeyCode::KeyL));
    sc.until("journal screen", 5.0, |cx| {
        cx.ui().screen == gothic_ui::Screen::Journal
    });
    sc.wait(0.8);
    sc.shot("journal");
    sc.check("a quest is running after the dialogs", |cx| {
        let before = cx.st.store["fp0"].clone();
        let now = fingerprint(cx);
        let q0 = before
            .split("quests_running=")
            .nth(1)
            .unwrap_or("0")
            .split(' ')
            .next()
            .unwrap_or("0")
            .to_string();
        let q1 = now
            .split("quests_running=")
            .nth(1)
            .unwrap_or("0")
            .split(' ')
            .next()
            .unwrap_or("0")
            .to_string();
        expect(
            q0 != q1,
            format!("quests running {q0} -> {q1}"),
            format!("quests running still {q1}"),
        )
    });
    sc.act("L closes", |cx| cx.tap(KeyCode::KeyL));
    sc.wait(0.6);
    sc.check("journal closed by L", |cx| {
        expect(
            cx.ui().screen == gothic_ui::Screen::None,
            "closed",
            format!("{:?}", cx.ui().screen),
        )
    });
    sc
}

/// Enter on the first entry of the menu (what the highlighted entry is when it opens).
fn pick_first(sc: &mut Sc) {
    sc.act("Enter on the highlighted option", |cx| {
        cx.tap(KeyCode::Enter)
    });
}

/// Moves the selection to the last entry (the exit) and chooses it, when a menu is up.
fn pick_exit(sc: &mut Sc) {
    let mut next_at = 0.0f32;
    sc.step("choose the exit entry", move |cx| {
        if cx.play().mode != Mode::Dialog {
            return Poll::Done;
        }
        let n = cx.world.resource::<Voices>().menu.len();
        if n == 0 || !cx.world.resource::<Voices>().idle() {
            return if cx.t > 40.0 {
                Poll::Fail("no exit entry".into())
            } else {
                Poll::Wait
            };
        }
        if cx.now < next_at {
            return Poll::Wait;
        }
        next_at = cx.now + 0.08;
        let sel = cx.ui().options.selected().unwrap_or(0);
        if sel + 1 < n {
            cx.tap(KeyCode::ArrowDown);
        } else {
            cx.tap(KeyCode::Enter);
        }
        if cx.t > 40.0 {
            return Poll::Fail("could not reach the exit entry".into());
        }
        Poll::Wait
    });
}

pub fn ui_screens() -> Sc {
    let mut sc = Sc::new(120.0);
    sc.wait(1.0);
    sc.act("grab the mouse", |cx| {
        cx.cursor_to(Vec2::new(640.0, 360.0));
        cx.mouse_click(MouseButton::Left);
    });
    sc.wait(0.4);
    sc.act(
        "control: mouse motion with no screen open turns the camera",
        |cx| {
            let y = cx.player().yaw;
            cx.st.n.insert("yaw_c".into(), f64::from(y));
            cx.mouse_motion(Vec2::new(200.0, 0.0));
        },
    );
    sc.wait(0.5);
    sc.check("control: injected mouse motion reaches the camera", |cx| {
        let y0 = cx.st.n["yaw_c"] as f32;
        let y = cx.player().yaw;
        expect(
            (y - y0).abs() > 0.1,
            format!("yaw {y0:.2} -> {y:.2}"),
            format!("yaw unchanged ({y0:.2}): the 'camera does not turn' checks prove nothing"),
        )
    });
    for (key, screen, name) in [
        (KeyCode::KeyI, gothic_ui::Screen::Inventory, "inventory"),
        (KeyCode::KeyC, gothic_ui::Screen::Character, "character"),
        (KeyCode::KeyL, gothic_ui::Screen::Journal, "journal"),
    ] {
        sc.act(format!("open {name}"), move |cx| {
            let yaw = cx.player().yaw;
            cx.st.n.insert("yaw".into(), f64::from(yaw));
            cx.tap(key);
        });
        sc.until(format!("{name} open"), 4.0, move |cx| {
            cx.ui().screen == screen
        });
        sc.act("mouse motion while the screen is open", |cx| {
            cx.mouse_motion(Vec2::new(200.0, 0.0));
        });
        sc.wait(0.6);
        sc.shot(name);
        sc.check(
            format!("{name}: the camera does not turn under the screen"),
            |cx| {
                let y0 = cx.st.n["yaw"] as f32;
                let y = cx.player().yaw;
                expect(
                    (y - y0).abs() < 0.06,
                    "yaw stable",
                    format!("yaw {y0} -> {y}"),
                )
            },
        );
        sc.check(format!("{name}: the mouse cursor is free to click the page"), |cx| {
            let g = cx.cursor_grabbed();
            expect(!g, "cursor free", "the cursor is still grabbed / hidden: the page cannot be clicked with the mouse")
        });
        sc.check(
            format!("{name}: damage protection (gui_open) set while the screen is up"),
            |cx| {
                let g = cx.core().0.sim.gui_open;
                expect(
                    g,
                    "gui_open",
                    "gui_open is false: the hero can be hurt behind the page",
                )
            },
        );
        sc.act(format!("close {name} with the same key"), move |cx| {
            cx.tap(key)
        });
        sc.wait(0.6);
        sc.check(format!("{name} closed by its key"), |cx| {
            expect(
                cx.ui().screen == gothic_ui::Screen::None,
                "closed",
                format!("{:?}", cx.ui().screen),
            )
        });
    }
    let _ = (ok("x"), Poll::Done);
    sc
}
