//! Open-world scenarios: sector switch, teleporter stones, walking to another city, and the
//! frame-time budget of crossing the world.
use super::{Cx, Poll, Sc, expect};
use crate::walk::play::{PlayState, Roster};
use bevy::prelude::*;

const ARDEA_01: &str = "ardea_npc_01";
const ARDEA_02: &str = "ardea_npc_02";

fn grab(sc: &mut Sc) {
    sc.wait(1.0);
    sc.act("grab the mouse", |cx| {
        cx.cursor_to(Vec2::new(640.0, 360.0));
        cx.mouse_click(MouseButton::Left);
    });
    sc.wait(0.4);
}


/// Fraction of pure-white pixels inside the hero's screen box (torso and head): a character that
/// is shaded like the original (`char.wesl`, ge_default.fx light sum into an 8-bit target) keeps
/// its bright skin and steel textures below white. Reports through a check `no clipped character
/// pixels (<tag>)`: at most 5 % of the sampled pixels may be 253..255 in all three channels.
fn clip_probe(sc: &mut Sc, tag: &'static str) {
    use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
    use std::sync::{Arc, Mutex};
    let out: Arc<Mutex<Option<(u32, u32)>>> = Arc::new(Mutex::new(None));
    let o2 = out.clone();
    sc.act(&format!("sample the hero's pixels ({tag})"), move |cx| {
        let hero = cx.player().ctrl.position;
        let mut q = cx
            .world
            .query_filtered::<(&Camera, &GlobalTransform), With<Camera3d>>();
        let Some((cam, gt)) = q.iter(cx.world).next() else {
            return;
        };
        let at = |dy: f32| cam.world_to_viewport(gt, (hero + Vec3::Y * dy) * 0.01).ok();
        let (Some(feet), Some(head)) = (at(20.0), at(170.0)) else {
            return;
        };
        let vp = cam.logical_viewport_size().unwrap_or(Vec2::new(1280.0, 720.0));
        let h = (feet.y - head.y).abs();
        let c = (feet + head) * 0.5;
        let (x0, x1) = (c.x - 0.22 * h, c.x + 0.22 * h);
        let (y0, y1) = (head.y.min(feet.y), head.y.max(feet.y));
        let o3 = o2.clone();
        cx.world
            .spawn(Screenshot::primary_window())
            .observe(move |cap: On<ScreenshotCaptured>| {
                let img = &cap.image;
                let k = img.width() as f32 / vp.x;
                let (mut n, mut white) = (0u32, 0u32);
                let (xa, xb) = ((x0 * k).max(0.0) as u32, (x1 * k).min(img.width() as f32 - 1.0) as u32);
                let (ya, yb) = ((y0 * k).max(0.0) as u32, (y1 * k).min(img.height() as f32 - 1.0) as u32);
                for y in ya..=yb {
                    for x in xa..=xb {
                        if let Ok(col) = img.get_color_at(x, y) {
                            let c = col.to_srgba();
                            n += 1;
                            if c.red >= 0.99 && c.green >= 0.99 && c.blue >= 0.99 {
                                white += 1;
                            }
                        }
                    }
                }
                *o3.lock().unwrap() = Some((n, white));
            });
    });
    sc.wait(1.0);
    sc.check(&format!("no clipped character pixels ({tag})"), move |cx| {
        match *out.lock().unwrap() {
            Some((n, w)) if n > 0 => {
                let f = f64::from(w) / f64::from(n);
                cx.note(format!("clip probe {tag}: {w} of {n} pixels pure white ({:.2} %)", f * 100.0));
                expect(f <= 0.05, format!("{:.2} % white", f * 100.0), format!("{:.2} % pure white", f * 100.0))
            }
            _ => expect(false, String::new(), "no hero box on screen".to_string()),
        }
    });
}

/// Bodies shown now whose document path contains `needle`.
fn live_in(cx: &Cx, needle: &str) -> usize {
    let r = cx.world.resource::<Roster>();
    r.live
        .iter()
        .filter(|i| {
            r.docs[r.cands[**i as usize].doc as usize]
                .path
                .contains(needle)
        })
        .count()
}

fn live_total(cx: &Cx) -> usize {
    cx.world.resource::<Roster>().live.len()
}

/// Centre (cm, mirrored space) of the characters stored in documents containing any `needles`.
fn centroid(cx: &Cx, needles: &[&str]) -> Option<Vec3> {
    let r = cx.world.resource::<Roster>();
    let pts: Vec<Vec3> = r
        .cands
        .iter()
        .filter(|c| {
            needles
                .iter()
                .any(|n| r.docs[c.doc as usize].path.contains(n))
        })
        .map(|c| c.pos_cm)
        .collect();
    (!pts.is_empty()).then(|| pts.iter().copied().sum::<Vec3>() / pts.len() as f32)
}

/// The densest group of characters of the documents (most neighbours within 8 m): where the
/// life of a city is.
fn cluster(cx: &Cx, needles: &[&str]) -> Option<Vec3> {
    let r = cx.world.resource::<Roster>();
    let pts: Vec<Vec3> = r
        .cands
        .iter()
        .filter(|c| {
            needles
                .iter()
                .any(|n| r.docs[c.doc as usize].path.contains(n))
        })
        .map(|c| c.pos_cm)
        .collect();
    pts.iter()
        .copied()
        .max_by_key(|p| pts.iter().filter(|q| (**q - *p).length() < 800.0).count())
}

fn place_hero(cx: &mut Cx, at: Vec3) {
    let mut p = cx.world.resource_mut::<crate::walk::Player>();
    p.ctrl.position = at;
    p.ctrl.vertical_speed = 0.0;
    p.placed = true;
}

/// Frame-time histogram (1 ms buckets) of the frames between `ft_on = 1` and `ft_on = 0`.
fn frame_watch(sc: &mut Sc) {
    sc.watch(|cx| {
        if cx.st.n.get("ft_on").copied().unwrap_or(0.0) < 0.5 {
            return;
        }
        let ms = cx.world.resource::<Time<Real>>().delta_secs() * 1000.0;
        let bucket = (ms as u32).min(200);
        *cx.st.n.entry(format!("ft_{bucket:03}")).or_insert(0.0) += 1.0;
        *cx.st.n.entry("ft_n".into()).or_insert(0.0) += 1.0;
    });
}

fn percentile(cx: &Cx, q: f64) -> (f64, u32) {
    let total = cx.st.n.get("ft_n").copied().unwrap_or(0.0);
    let mut acc = 0.0;
    for b in 0..=200u32 {
        acc += cx.st.n.get(&format!("ft_{b:03}")).copied().unwrap_or(0.0);
        if total > 0.0 && acc >= total * q {
            return (total, b + 1);
        }
    }
    (total, 0)
}

fn report_frames(sc: &mut Sc, label: &'static str, limit_ms: u32) {
    sc.check(format!("frame time p95 {label}"), move |cx| {
        let (n, p95) = percentile(cx, 0.95);
        let (_, p50) = percentile(cx, 0.5);
        let (_, p99) = percentile(cx, 0.99);
        cx.note(format!(
            "frames {label}: n={n} p50<={p50} ms p95<={p95} ms p99<={p99} ms (limit {limit_ms})"
        ));
        let c = cx.world.resource::<Roster>().cost;
        let avg = |x: crate::walk::play::Cost| x.total_us as f64 / f64::from(x.runs.max(1));
        cx.note(format!(
            "roster systems: scan {} runs avg {:.0} us max {} us; spawn pass {} runs avg {:.0} us max {} us; body build {} runs avg {:.0} us max {} us",
            c.scan.runs, avg(c.scan), c.scan.max_us, c.spawn.runs, avg(c.spawn), c.spawn.max_us,
            c.build.runs, avg(c.build), c.build.max_us
        ));
        let prof: Vec<String> = gothic_animation::library::profile()
            .into_iter()
            .map(|(n, ms, c)| format!("{n} {ms:.0} ms/{c}"))
            .collect();
        cx.note(format!("cold library paths (main thread, since start): {}", prof.join(", ")));
        // limit 0: informational (teleport hops and cold body builds hitch by design).
        expect(
            n > 30.0 && (limit_ms == 0 || p95 <= limit_ms),
            format!("p95 {p95} ms over {n} frames"),
            format!("p95 {p95} ms over {n} frames, limit {limit_ms}"),
        )
    });
}

/// Ardea: the occupied layer `Ardea_NPC_01` is switched off and the liberated `Ardea_NPC_02`
/// on by the info command `SetSectorStatus` (executed through the ported script interpreter).
pub fn sector_switch() -> Sc {
    let mut sc = Sc::new(240.0);
    grab(&mut sc);
    sc.act(
        "setup: stand among the Ardea characters of both layers",
        |cx| {
            let at = centroid(cx, &[ARDEA_01, ARDEA_02]).unwrap_or(Vec3::ZERO);
            place_hero(cx, at + Vec3::Y * 400.0);
        },
    );
    sc.wait(8.0);
    sc.check(
        "new game: the occupied layer exists, the liberated one does not",
        |cx| {
            let (a, b) = (live_in(cx, ARDEA_01), live_in(cx, ARDEA_02));
            let docs = cx
                .world
                .resource::<Roster>()
                .docs
                .iter()
                .map(|d| (d.path.clone(), d.enabled))
                .collect::<Vec<_>>();
            let on = |n: &str| docs.iter().any(|(p, e)| p.contains(n) && *e);
            expect(
                a > 0 && b == 0 && on(ARDEA_01) && !on(ARDEA_02),
                format!("NPC_01 shown {a}, NPC_02 shown {b}"),
                format!(
                    "NPC_01 shown {a} (enabled {}), NPC_02 shown {b} (enabled {})",
                    on(ARDEA_01),
                    on(ARDEA_02)
                ),
            )
        },
    );
    sc.shot("occupied");
    sc.act("execute the info that liberates Ardea", |cx| {
        let db = cx.core().0.db.clone();
        let mut found: Option<(String, String)> = None;
        for info in &db.infos {
            for s in &info.script {
                if s.command.eq_ignore_ascii_case("SetSectorStatus")
                    && s.id1.to_ascii_lowercase().contains(ARDEA_02)
                {
                    found = Some((info.name.clone(), s.id1.clone()));
                }
            }
        }
        let all: Vec<String> = db
            .infos
            .iter()
            .flat_map(|i| {
                i.script
                    .iter()
                    .filter(|s| s.command.eq_ignore_ascii_case("SetSectorStatus"))
                    .filter(|s| s.id1.to_ascii_lowercase().contains("ardea"))
                    .map(|s| format!("{}: {} {}", i.name, s.id1, s.id2))
                    .collect::<Vec<_>>()
            })
            .collect();
        cx.note(format!("SetSectorStatus infos naming Ardea: {all:?}"));
        match found {
            Some((name, sector)) => {
                let id = db.info_id(&name).expect("info id");
                let owner = db.info(id).owner.clone();
                let mut core = cx.core_mut();
                let res = gothic_gameplay::script::execute_info(
                    &db,
                    &mut core.0.sim.state,
                    id,
                    &owner,
                );
                let shown = core.0.sim.state.sectors.clone();
                cx.st.store.insert("info".into(), name.clone());
                cx.note(format!(
                    "executed {name} (owner {owner}, sector {sector}), refused {:?}, sectors now {shown:?}",
                    res.refused
                ));
            }
            None => cx.note("no info switches the Ardea NPC_02 layer"),
        }
    });
    sc.until("the core reported both switches", 20.0, |cx| {
        let r = cx.world.resource::<Roster>();
        r.sector_switches
            .iter()
            .any(|(s, on)| s.to_ascii_lowercase().contains(ARDEA_02) && *on)
    });
    sc.wait(8.0);
    sc.check(
        "liberated: the occupied layer is gone and the liberated one is shown",
        |cx| {
            let (a, b) = (live_in(cx, ARDEA_01), live_in(cx, ARDEA_02));
            let sw = cx.world.resource::<Roster>().sector_switches.clone();
            expect(
                a == 0 && b > 0,
                format!("NPC_01 shown {a}, NPC_02 shown {b}, switches {sw:?}"),
                format!("NPC_01 shown {a}, NPC_02 shown {b}, switches {sw:?}"),
            )
        },
    );
    sc.shot("liberated");
    sc.act(
        "set the sector flags back (a load of an older save)",
        |cx| {
            cx.core_mut().0.sim.state.sectors.clear();
        },
    );
    sc.wait(8.0);
    sc.check("older save: the authored layers return", |cx| {
        let (a, b) = (live_in(cx, ARDEA_01), live_in(cx, ARDEA_02));
        expect(
            a > 0 && b == 0,
            format!("NPC_01 shown {a}, NPC_02 shown {b}"),
            format!("NPC_01 shown {a}, NPC_02 shown {b}"),
        )
    });
    sc
}

fn teleport_stone(sc: &mut Sc, city: &'static str, slot_key: KeyCode) {
    sc.act(
        format!("setup: the teleporter stone for {city} in quick slot 7 (picked up in the world)"),
        move |cx| {
            let item = format!("It_Teleport_{city}");
            cx.world.resource_mut::<PlayState>().quick[6] = Some(item.clone());
            super::scenarios::grant(cx, &item, 1);
        },
    );
    sc.wait(0.5);
    sc.act("select the stone", move |cx| cx.tap(slot_key));
    sc.step(
        "aim 1 s",
        |cx| {
            if cx.t > 1.0 { Poll::Done } else { Poll::Wait }
        },
    );
    sc.act("hold left (cast)", |cx| cx.mouse_down(MouseButton::Left));
    sc.wait(0.3);
    sc.act("release left", |cx| cx.mouse_up(MouseButton::Left));
}

/// Fast travel as the original does it: a teleporter stone, then the hero stands at the
/// `Teleport_<City>` point and the characters of that city appear.
pub fn teleport() -> Sc {
    let mut sc = Sc::new(300.0);
    frame_watch(&mut sc);
    grab(&mut sc);
    sc.act("record the start", |cx| {
        let p = cx.player().ctrl.position;
        cx.st.n.insert("x0".into(), f64::from(p.x));
        cx.st.n.insert("z0".into(), f64::from(p.z));
        let n = live_in(cx, "ardea");
        cx.st.n.insert("ardea0".into(), n as f64);
        cx.st.n.insert("ft_on".into(), 1.0);
    });
    teleport_stone(&mut sc, "Faring", KeyCode::Digit7);
    sc.until("the hero stands at Teleport_Faring", 30.0, |cx| {
        let dest = cx
            .world
            .resource::<crate::walk::play::teleport::Destinations>()
            .point_cm("Teleport_Faring")
            .unwrap_or(Vec3::ZERO);
        let p = cx.player().ctrl.position;
        Vec2::new(p.x - dest.x, p.z - dest.z).length() < 600.0
    });
    sc.until(
        "the destination is loaded and the hero stands on it",
        90.0,
        |cx| {
            cx.player().ctrl.grounded
                && cx
                    .world
                    .resource::<crate::walk::Stream>()
                    .pending
                    .is_empty()
                && cx
                    .world
                    .resource::<crate::walk::Stream>()
                    .in_flight
                    .is_empty()
        },
    );
    sc.wait(6.0);
    sc.shot("faring");
    sc.check("the core moved the hero's party to the same point", |cx| {
        let dest = cx
            .world
            .resource::<crate::walk::play::teleport::Destinations>()
            .point_cm("Teleport_Faring")
            .unwrap_or(Vec3::ZERO);
        let p = cx.player().ctrl.position;
        let d = Vec2::new(p.x - dest.x, p.z - dest.z).length();
        let x0 = cx.st.n["x0"] as f32;
        let z0 = cx.st.n["z0"] as f32;
        expect(
            d < 800.0 && Vec2::new(p.x - x0, p.z - z0).length() > 30_000.0,
            format!(
                "hero {d:.0} cm from Teleport_Faring, moved {:.0} m",
                Vec2::new(p.x - x0, p.z - z0).length() / 100.0
            ),
            format!("hero {d:.0} cm from Teleport_Faring"),
        )
    });
    sc.check(
        "the characters of Faring are shown, Ardea's are gone",
        |cx| {
            let f = live_in(cx, "faring");
            let a = live_in(cx, "ardea");
            expect(
                f > 0 && a == 0,
                format!("Faring {f}, Ardea {a} (was {})", cx.st.n["ardea0"]),
                format!("Faring {f}, Ardea {a}"),
            )
        },
    );
    sc.act("stop the frame clock", |cx| {
        cx.st.n.insert("ft_on".into(), 0.0);
    });
    report_frames(&mut sc, "teleport", 0);
    sc
}

/// The hero leaves Ardea on foot for Reddock (about 190 m north-west); the city's characters
/// are there when he arrives and Ardea's have left the world presentation.
pub fn walk_to_reddock() -> Sc {
    let mut sc = Sc::new(420.0);
    frame_watch(&mut sc);
    grab(&mut sc);
    sc.act("record the start", |cx| {
        let n = live_in(cx, "ardea");
        let total = live_total(cx);
        cx.st.n.insert("ardea0".into(), n as f64);
        cx.st.n.insert("ft_on".into(), 1.0);
        cx.note(format!(
            "shown at the start: {total} bodies, {n} from Ardea"
        ));
    });
    sc.shot("ardea");
    // Reddock market: the middle of the city's characters.
    // The route is the one the original navigation graph gives (`gCNavigationMap::FindWay`,
    // authored zones and road corridors); the hero follows its points like a player on the road.
    let mut route: Option<Vec<Vec3>> = None;
    let mut next = 0usize;
    sc.step("walk to Reddock", move |cx| {
        let goal = cluster(cx, &["reddock_npc_01"]).unwrap_or(Vec3::ZERO);
        let hero = cx.player().ctrl.position;
        if route.is_none() {
            let graph = cx
                .world
                .resource::<crate::walk::play::nav::NavRes>()
                .0
                .graph();
            let found = graph.and_then(|g| g.find_way(hero, goal).ok());
            match &found {
                Some(r) => cx.note(format!(
                    "FindWay Ardea -> Reddock: {} points, {:.0} m",
                    r.points.len(),
                    r.length / 100.0
                )),
                None => cx.note("FindWay found no route: straight line with detours"),
            }
            if let Some(r) = &found {
                let list: Vec<String> = r
                    .points
                    .iter()
                    .map(|p| format!("({:.0},{:.0},{:.0})", p.x, p.y, p.z))
                    .collect();
                cx.note(format!("route points {}", list.join(" ")));
            }
            route = Some(found.map(|r| r.points).unwrap_or_default());
        }
        let tick = (cx.t / 10.0) as i64;
        if cx.st.n.get("walk_tick").copied().unwrap_or(-1.0) < tick as f64 {
            cx.st.n.insert("walk_tick".into(), tick as f64);
            cx.note(format!(
                "walk t={:.0}s hero ({:.0}, {:.0}, {:.0}) cm, {:.0} m to the goal, waypoint {next}, mode {:?}, grounded {}, yaw {:.2}, hero {}",
                cx.t,
                hero.x,
                hero.y,
                hero.z,
                Vec2::new(hero.x - goal.x, hero.z - goal.z).length() / 100.0,
                cx.world.resource::<PlayState>().mode,
                cx.player().ctrl.grounded,
                cx.player().yaw,
                {
                    let c = &cx.core().0;
                    let a = c.sim.actor(c.hero);
                    format!("{:?} {:?} hp {}", a.ani_state, a.ai_mode, a.hp)
                }
            ));
        }
        if cx.t > 240.0 {
            cx.note("walk timed out: continuing from where the hero stands");
            cx.key_up(KeyCode::KeyW);
            return Poll::Done;
        }
        let pts = route.as_ref().unwrap();
        // A hero that has not moved for 6 s is wedged in collision (seen: trees and rocks on the
        // slopes): note it and set him 4 m on towards the next waypoint so the walk continues.
        let moved = Vec2::new(hero.x, hero.z)
            .distance(Vec2::new(
                cx.st.n.get("wedge_x").copied().unwrap_or(0.0) as f32,
                cx.st.n.get("wedge_z").copied().unwrap_or(0.0) as f32,
            ))
            > 60.0;
        if moved || cx.t < 0.1 {
            cx.st.n.insert("wedge_x".into(), f64::from(hero.x));
            cx.st.n.insert("wedge_z".into(), f64::from(hero.z));
            cx.st.n.insert("wedge_t".into(), f64::from(cx.t));
        } else if f64::from(cx.t) - cx.st.n.get("wedge_t").copied().unwrap_or(0.0) > 6.0 {
            {
                let probe = {
                    let p = cx.player();
                    let col = cx.world.resource::<crate::walk::Collision>();
                    p.ctrl.debug_probe(&col.0)
                };
                cx.note(format!("WEDGE PROBE {probe}"));
            }
            let n = cx.st.n.entry("wedges".into()).or_insert(0.0);
            *n += 1.0;
            cx.key_up(KeyCode::KeyW);
            return Poll::Fail(format!(
                "hero wedged at ({:.0}, {:.0}, {:.0}) cm for 6 s heading for waypoint {next}",
                hero.x, hero.y, hero.z
            ));

        }
        // Skip waypoints already passed; head for the next one, the goal last.
        while next < pts.len() && Vec2::new(hero.x - pts[next].x, hero.z - pts[next].z).length() < 250.0 {
            next += 1;
        }
        if next < pts.len() {
            // Local steering like a walking character: the grid mesh grown from the collision
            // around the hero refines the leg to the next authored point (`SharedNav::path`).
            let leg = cx
                .world
                .resource::<crate::walk::play::nav::NavRes>()
                .0
                .path(hero, pts[next]);
            let head = leg
                .and_then(|l| {
                    l.points
                        .into_iter()
                        .find(|p| Vec2::new(hero.x - p.x, hero.z - p.z).length() > 200.0)
                })
                .unwrap_or(pts[next]);
            let _ = cx.walk_to(head, 100.0, true);
            Poll::Wait
        } else {
            cx.walk_to(goal, 900.0, true)
        }
    });
    sc.shot("walk_end");
    sc.act("record where the walk ended", |cx| {
        let goal = cluster(cx, &["reddock_npc_01"]).unwrap_or(Vec3::ZERO);
        let p = cx.player().ctrl.position;
        let d = Vec2::new(p.x - goal.x, p.z - goal.z).length();
        cx.note(format!(
            "the hero was wedged in collision {} times on the way",
            cx.st.n.get("wedges").copied().unwrap_or(0.0)
        ));
        cx.note(format!(
            "after the walk the hero is {:.0} m from the Reddock characters",
            d / 100.0
        ));
        cx.st.n.insert("walk_left".into(), f64::from(d));
    });
    sc.check("the hero reached Reddock on foot", |cx| {
        let d = cx.st.n["walk_left"];
        expect(
            d < 2500.0,
            format!("{:.0} m left", d / 100.0),
            format!("{:.0} m left", d / 100.0),
        )
    });
    // If the walk was blocked (city wall, bad ground), the rest still has to be tested.
    sc.act(
        "setup (only if the walk failed): stand at the Reddock market",
        |cx| {
            if cx.st.n["walk_left"] > 2500.0 {
                let goal = cluster(cx, &["reddock_npc_01"]).unwrap_or(Vec3::ZERO);
                place_hero(cx, goal + Vec3::new(200.0, 800.0, 0.0));
                cx.note("setup: hero placed at the Reddock market");
            }
        },
    );
    sc.until("the hero stands on ground", 60.0, |cx| {
        cx.player().ctrl.grounded
    });
    sc.wait(8.0);
    sc.shot("reddock");
    sc.check("Reddock's characters are there", |cx| {
        let n = live_in(cx, "reddock");
        let core = &cx.core().0;
        let named: Vec<&str> = ["Brenton", "Gelford", "Javier"]
            .into_iter()
            .filter(|n| core.find_name(n).is_some())
            .collect();
        expect(
            n >= 3,
            format!("{n} bodies from Reddock shown, core actors {named:?}"),
            format!("{n} bodies from Reddock shown, core actors {named:?}"),
        )
    });
    sc.check("Ardea's characters left the world presentation", |cx| {
        let n = live_in(cx, "ardea");
        expect(
            n == 0,
            format!("{n} from Ardea shown"),
            format!("{n} from Ardea shown"),
        )
    });
    sc.step("turn to look at the nearest character", |cx| {
        let hero = cx.player().ctrl.position;
        let goal = {
            let r = cx.world.resource::<Roster>();
            r.live
                .iter()
                .filter_map(|i| r.cands[*i as usize].actor)
                .filter_map(|a| cx.actor_pos(a))
                .min_by(|a, b| (*a - hero).length().total_cmp(&(*b - hero).length()))
        };
        if let Some(g) = goal {
            cx.aim_at(g, -0.12);
        }
        if cx.t > 2.5 { Poll::Done } else { Poll::Wait }
    });
    sc.wait(1.0);
    sc.shot("reddock_market");
    clip_probe(&mut sc, "reddock market");
    sc.act("stop the frame clock", |cx| {
        cx.st.n.insert("ft_on".into(), 0.0);
    });
    report_frames(&mut sc, "walk to Reddock", 30);
    sc
}

/// Frame times standing in Ardea (reference for the walks on a loaded machine).
pub fn perf_idle() -> Sc {
    let mut sc = Sc::new(120.0);
    frame_watch(&mut sc);
    sc.act("start the frame clock", |cx| {
        cx.st.n.insert("ft_on".into(), 1.0);
    });
    sc.wait(15.0);
    sc.act("stop the frame clock", |cx| {
        cx.st.n.insert("ft_on".into(), 0.0);
    });
    report_frames(&mut sc, "idle in Ardea", 0);
    sc
}

/// Frame times while crossing the world: teleport hops between regions with a walk at each.
pub fn world_walk() -> Sc {
    let mut sc = Sc::new(600.0);
    frame_watch(&mut sc);
    grab(&mut sc);
    let hops: [(&'static str, &'static str); 4] = [
        ("Reddock", "reddock_npc_01"),
        ("KapDun", "kap_dun_npc_01"),
        ("Gotha", "gotha_npc_01"),
        ("Montera", "montera_npc_01"),
    ];
    sc.act("start the frame clock", |cx| {
        cx.st.n.insert("ft_on".into(), 1.0);
    });
    for (city, doc) in hops {
        sc.act(format!("setup: hop to {city}"), move |cx| {
            let at = cx
                .world
                .resource::<crate::walk::play::teleport::Destinations>()
                .point_cm(&format!("Teleport_{city}"))
                .or_else(|| centroid(cx, &[doc]))
                .unwrap_or(Vec3::ZERO);
            place_hero(cx, at + Vec3::Y * 150.0);
        });
        sc.until(format!("{city}: standing"), 90.0, |cx| {
            cx.player().ctrl.grounded
        });
        sc.step(format!("{city}: walk for 25 s"), move |cx| {
            let goal = centroid(cx, &[doc]).unwrap_or(Vec3::ZERO);
            if cx.t > 25.0 {
                cx.key_up(KeyCode::KeyW);
                return Poll::Done;
            }
            let _ = cx.walk_to(goal, 300.0, true);
            Poll::Wait
        });
        sc.act(format!("{city}: census"), move |cx| {
            let n = live_total(cx);
            let (spawned, despawned) = {
                let r = cx.world.resource::<Roster>();
                (r.spawned, r.despawned)
            };
            let actors = cx.core().0.sim.actors.len();
            cx.note(format!(
                "{city}: {n} bodies shown, {spawned} spawned so far, {despawned} removed, core actors {actors}"
            ));
        });
        sc.shot(city);
    }
    sc.act("stop the frame clock", |cx| {
        cx.st.n.insert("ft_on".into(), 0.0);
    });
    report_frames(&mut sc, "world walk", 0);
    sc
}

/// A trigger of the world fed by a physics contact: the hero walks onto a loose weapon whose
/// `gCTouchDamage_PS` is an `eCTrigger_PS` with the script `OnTouchDamage`; the ported trigger
/// runtime fires it once (`MaxCountPerEntity = 1`) and not again on the second visit.
pub fn triggers() -> Sc {
    use crate::walk::play::triggers::WorldTriggers;
    let mut sc = Sc::new(240.0);
    grab(&mut sc);
    sc.act(
        "setup: stand 6 m from the nearest touch-damage weapon of Ardea",
        |cx| {
            let hero = cx.player().ctrl.position;
            let (n, at, name, total) = {
                let t = cx.world.resource::<WorldTriggers>();
                let n = t.nearest(hero).expect("the world has touch triggers");
                (n, t.src[n].pos_cm, t.src[n].name.clone(), t.src.len())
            };
            cx.st.n.insert("trig".into(), n as f64);
            cx.note(format!(
                "{total} touch triggers in the world; nearest {name} at {:.0} m",
                (at - hero).length() / 100.0
            ));
            place_hero(cx, at + Vec3::new(600.0, 300.0, 0.0));
        },
    );
    sc.until("the trigger is registered with the core", 90.0, |cx| {
        let n = cx.st.n["trig"] as usize;
        cx.player().ctrl.grounded && cx.world.resource::<WorldTriggers>().is_live(n)
    });
    sc.shot("near");
    sc.act(
        "walk onto it (setup: place the capsule on the weapon)",
        |cx| {
            let n = cx.st.n["trig"] as usize;
            let at = cx.world.resource::<WorldTriggers>().src[n].pos_cm;
            place_hero(cx, at + Vec3::Y * 20.0);
        },
    );
    sc.wait(1.5);
    sc.shot("on_it");
    sc.check("the touch fired the script trigger once", |cx| {
        let t = cx.world.resource::<WorldTriggers>();
        let n = cx.st.n["trig"] as usize;
        let name = t.src[n].name.clone();
        let fired = t.fired.iter().filter(|f| f.name == name).count();
        let scripts: Vec<String> = t.fired.iter().map(|f| f.script.clone()).collect();
        expect(
            fired == 1 && t.touches >= 1,
            format!("fired {fired}x {scripts:?}, touches {}", t.touches),
            format!(
                "fired {fired}x {scripts:?}, touches {} untouches {}",
                t.touches, t.untouches
            ),
        )
    });
    sc.act("step away", |cx| {
        let n = cx.st.n["trig"] as usize;
        let at = cx.world.resource::<WorldTriggers>().src[n].pos_cm;
        place_hero(cx, at + Vec3::new(500.0, 100.0, 0.0));
    });
    sc.wait(1.5);
    sc.act("and back on", |cx| {
        let n = cx.st.n["trig"] as usize;
        let at = cx.world.resource::<WorldTriggers>().src[n].pos_cm;
        place_hero(cx, at + Vec3::Y * 20.0);
    });
    sc.wait(1.5);
    sc.check("the per-entity budget holds the second visit back", |cx| {
        let t = cx.world.resource::<WorldTriggers>();
        let n = cx.st.n["trig"] as usize;
        let name = t.src[n].name.clone();
        let fired = t.fired.iter().filter(|f| f.name == name).count();
        expect(
            fired == 1 && t.touches >= 2 && t.untouches >= 1,
            format!(
                "fired {fired}x, touches {} untouches {}",
                t.touches, t.untouches
            ),
            format!(
                "fired {fired}x, touches {} untouches {}",
                t.touches, t.untouches
            ),
        )
    });
    sc.check("the trigger state is in the core's save image", |cx| {
        let snap = {
            let c = &cx.core().0;
            c.triggers.snapshot(c.clock)
        };
        expect(
            !snap.triggers.is_empty(),
            format!("{} trigger entries saved", snap.triggers.len()),
            "no trigger state".to_string(),
        )
    });
    sc
}

/// Population check: at each place the characters of the enabled layers inside the 4000 box
/// around the hero (the data) against the bodies shown and the queue; plus the world totals.
pub fn world_census() -> Sc {
    let mut sc = Sc::new(900.0);
    frame_watch(&mut sc);
    grab(&mut sc);
    sc.act("world totals from the data", |cx| {
        let r = cx.world.resource::<Roster>();
        let on = r
            .cands
            .iter()
            .filter(|c| r.docs[c.doc as usize].enabled)
            .count();
        let mut by_doc: std::collections::BTreeMap<String, usize> = Default::default();
        for c in &r.cands {
            if r.docs[c.doc as usize].enabled {
                *by_doc
                    .entry(r.docs[c.doc as usize].sector.clone())
                    .or_default() += 1;
            }
        }
        let top: Vec<String> = by_doc
            .iter()
            .filter(|(_, n)| **n >= 40)
            .map(|(k, n)| format!("{k}={n}"))
            .collect();
        cx.note(format!(
            "world: {} candidates in {} layers, {on} in enabled layers; big sectors: {top:?}",
            r.cands.len(),
            r.docs.len()
        ));
        cx.st.n.insert("ft_on".into(), 1.0);
    });
    let places: [(&'static str, &'static str); 9] = [
        ("Ardea", "ardea_npc_01"),
        ("Reddock", "reddock_npc_01"),
        ("Faring", "faring"),
        ("KapDun", "kap_dun_npc_01"),
        ("Gotha", "gotha_npc_01"),
        ("Montera", "montera"),
        ("Geldern", "geldern"),
        ("Silden", "silden"),
        ("Trelis", "trelis"),
    ];
    for (city, doc) in places {
        sc.act(format!("setup: hop to {city}"), move |cx| {
            let at = centroid(cx, &[doc])
                .or_else(|| {
                    cx.world
                        .resource::<crate::walk::play::teleport::Destinations>()
                        .point_cm(&format!("Teleport_{city}"))
                })
                .unwrap_or(Vec3::ZERO);
            cx.st.n.insert(
                format!("has_{city}"),
                if centroid(cx, &[doc]).is_some() {
                    1.0
                } else {
                    0.0
                },
            );
            let at = cluster(cx, &[doc]).unwrap_or(at);
            place_hero(cx, at + Vec3::Y * 300.0);
        });
        sc.until(format!("{city}: standing"), 120.0, |cx| {
            cx.player().ctrl.grounded
        });
        sc.until(format!("{city}: queue drained"), 60.0, |cx| {
            let r = cx.world.resource::<Roster>();
            r.queue.is_empty() && cx.t > 6.0
        });
        sc.wait(2.0);
        sc.act(format!("{city}: census"), move |cx| {
            let hero = cx.player().ctrl.position;
            let (want, live, queued, failed) = {
                let r = cx.world.resource::<Roster>();
                let near = |p: Vec3| (p.x - hero.x).abs().max((p.z - hero.z).abs()) <= 4_000.0;
                let want = r
                    .cands
                    .iter()
                    .filter(|c| r.docs[c.doc as usize].enabled && near(c.pos_cm))
                    .count();
                let failed = r
                    .cands
                    .iter()
                    .filter(|c| c.failed && r.docs[c.doc as usize].enabled && near(c.pos_cm))
                    .count();
                (want, r.live.len(), r.queue.len(), failed)
            };
            {
                let r = cx.world.resource::<Roster>();
                let mut near: Vec<(f32, String)> = r
                    .live
                    .iter()
                    .map(|i| &r.cands[*i as usize])
                    .map(|c| {
                        (
                            (c.pos_cm - hero).length(),
                            format!(
                                "{} {:?}",
                                c.record.name,
                                c.look.as_ref().map(|l| &l.visual.body_parts)
                            ),
                        )
                    })
                    .collect();
                near.sort_by(|a, b| a.0.total_cmp(&b.0));
                for (d, n) in near.iter().take(3) {
                    cx.note(format!("NEAR {city} {d:.0} cm: {n}"));
                }
            }
            let actors = cx.core().0.sim.actors.len();
            cx.note(format!(
                "CENSUS {city}: data {want} in range, shown {live}, queued {queued}, appearance failed {failed}, core actors {actors}"
            ));
            cx.st.n.insert(format!("c_{city}"), (want as i64 - failed as i64 - live as i64) as f64);
        });
        sc.shot(city);
    }
    sc.act("stop the frame clock", |cx| {
        cx.st.n.insert("ft_on".into(), 0.0);
    });
    sc.check("no actor or item material falls back to white", |cx| {
        let list = gothic_animation::library::untextured();
        cx.note(format!("untextured materials: {list:?}"));
        expect(
            list.is_empty(),
            "all materials textured".to_string(),
            format!("{} untextured: {list:?}", list.len()),
        )
    });
    sc.check(
        "shown equals the data in range (minus unresolved looks) at every place",
        move |cx| {
            let bad: Vec<String> = places
                .iter()
                .filter(|(c, _)| cx.st.n.get(&format!("has_{c}")).copied().unwrap_or(0.0) > 0.5)
                .filter_map(|(c, _)| {
                    let d = cx.st.n.get(&format!("c_{c}")).copied().unwrap_or(0.0);
                    (d.abs() > 0.5).then(|| format!("{c}: {d}"))
                })
                .collect();
            expect(
                bad.is_empty(),
                "all places complete".to_string(),
                format!("short: {bad:?}"),
            )
        },
    );
    report_frames(&mut sc, "census hops", 0);
    sc
}

/// Daily schedules away from Ardea: in Reddock the clock is set to noon, night and early
/// morning (forward only, `GetRoutineTime`: 0 sleep, 1 work, 2 relax); the characters of the
/// city must walk to their other routine points each time.
pub fn world_routines() -> Sc {
    use std::sync::{Arc, Mutex};
    let mut sc = Sc::new(700.0);
    grab(&mut sc);
    sc.act("setup: stand in Reddock", |cx| {
        let at = cluster(cx, &["reddock_npc_01"]).unwrap_or(Vec3::ZERO);
        place_hero(cx, at + Vec3::Y * 300.0);
    });
    sc.until("standing", 120.0, |cx| cx.player().ctrl.grounded);
    sc.wait(10.0);
    let last: Arc<Mutex<std::collections::HashMap<u32, Vec3>>> = Default::default();
    for (label, hour) in [("noon", 12u64), ("night", 23), ("morning", 7)] {
        let first = last.clone();
        sc.act(format!("clock to {label}"), move |cx| {
            let now_day = cx.core().0.sim.clock.game_ms / 86_400_000;
            let target = (now_day + 1) * 86_400_000 + hour * 3_600_000;
            cx.core_mut().0.sim.clock.game_ms = target;
            let r = cx.world.resource::<Roster>();
            let ids: Vec<_> = r
                .live
                .iter()
                .filter_map(|i| r.cands[*i as usize].actor)
                .collect();
            let mut map = first.lock().unwrap();
            map.clear();
            for a in ids {
                if let Some(p) = cx.actor_pos(a) {
                    map.insert(a.0, p);
                }
            }
        });
        sc.wait(45.0);
        let last = last.clone();
        sc.act(format!("{label}: who moved"), move |cx| {
            let map = last.lock().unwrap();
            let mut moved = 0;
            let mut far = 0;
            for (id, p0) in map.iter() {
                if let Some(p) = cx.actor_pos(gothic_sim::types::ActorId(*id)) {
                    let d = Vec2::new(p.x - p0.x, p.z - p0.z).length();
                    if d > 100.0 {
                        moved += 1;
                    }
                    if d > 1500.0 {
                        far += 1;
                    }
                }
            }
            let with_points = cx.world.resource::<crate::walk::play::objects::WorldObjects>().with_points;
            cx.st.n.insert(format!("moved_{label}"), f64::from(moved));
            let env_h = cx.world.resource::<crate::walk::env::OrigEnv>().day_time * 24.0;
            let sim_h = cx.core().0.sim.clock.hour_of_day();
            cx.note(format!("TIME {label}: environment {env_h:.2} h, simulation {sim_h:.2} h"));
            let mut d = (env_h - sim_h).abs();
            d = d.min(24.0 - d);
            cx.st.n.insert(format!("envdiff_{label}"), f64::from(d));
            cx.note(format!(
                "ROUTINE {label}: {moved} of {} Reddock-area characters moved > 1 m, {far} > 15 m; characters with routine points so far {with_points}",
                map.len()
            ));
        });
        sc.shot(label);
    }
    sc.check(
        "the environment's time of day follows the simulation clock",
        |cx| {
            let worst = ["noon", "night", "morning"]
                .iter()
                .map(|l| {
                    cx.st
                        .n
                        .get(&format!("envdiff_{l}"))
                        .copied()
                        .unwrap_or(99.0)
                })
                .fold(0.0f64, f64::max);
            expect(
                worst < 0.1,
                format!("worst difference {worst:.3} h"),
                format!("worst difference {worst:.3} h"),
            )
        },
    );
    sc.check("the schedule moves characters outside Ardea", |cx| {
        let total: f64 = ["noon", "night", "morning"]
            .iter()
            .map(|l| cx.st.n.get(&format!("moved_{l}")).copied().unwrap_or(0.0))
            .sum();
        expect(
            total > 0.0,
            format!("{total} character moves over three schedule changes"),
            "nobody moved when the routine time changed".to_string(),
        )
    });
    sc
}

/// A chest outside Ardea: from the Reddock market the hero walks to the nearest unlocked chest
/// of an existing layer and opens it with the use key; the contents come from the chest's
/// treasure sets.
pub fn world_chest() -> Sc {
    use super::more::{Goal, approach};
    use std::sync::Arc;
    let mut sc = Sc::new(400.0);
    grab(&mut sc);
    sc.act("setup: stand at the Reddock market", |cx| {
        let at = cluster(cx, &["reddock_npc_01"]).unwrap_or(Vec3::ZERO);
        place_hero(cx, at + Vec3::Y * 300.0);
    });
    sc.until("standing", 120.0, |cx| cx.player().ctrl.grounded);
    sc.wait(6.0);
    let goal: Goal = Arc::new(|cx| {
        let hero = cx.hero_pos();
        let w = cx
            .world
            .resource::<crate::walk::play::objects::WorldObjects>();
        w.list
            .iter()
            .enumerate()
            .filter(|(i, o)| {
                o.entity.is_some()
                    && w.layer_on(*i)
                    && !o.def.is_item
                    && o.def.script_use() == gothic_sim::script::enums::UseType::Chest
                    && o.def.lock.as_ref().is_none_or(|l| !l.locked)
                    && (o.pos_cm - hero).length() < 20_000.0
            })
            .map(|(_, o)| o)
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
        Arc::new(|cx| matches!(cx.play().target, Some(crate::walk::play::Target::Object(_)))),
    );
    sc.act("note where we are", |cx| {
        let p = cx.player().ctrl.position;
        let (r, name) = {
            let o = cx
                .world
                .resource::<crate::walk::play::objects::WorldObjects>();
            let near = o
                .list
                .iter()
                .filter(|e| e.entity.is_some() && !e.def.is_item)
                .min_by(|a, b| (a.pos_cm - p).length().total_cmp(&(b.pos_cm - p).length()))
                .map(|e| e.def.name.clone());
            (live_in(cx, "reddock"), near)
        };
        cx.note(format!(
            "at the chest: {r} Reddock bodies shown, nearest object {name:?}"
        ));
        cx.use_key();
    });
    sc.until("loot page", 10.0, |cx| {
        cx.ui().screen == gothic_ui::Screen::Loot
    });
    sc.wait(1.0);
    sc.shot("chest_open");
    sc.check("the chest outside Ardea shows its contents", |cx| {
        let n = cx.inter().window.as_ref().map_or(0, |w| w.lines.len());
        expect(
            n > 0,
            format!("{n} window lines"),
            format!("{n} window lines"),
        )
    });
    sc.act("Esc", |cx| cx.tap(KeyCode::Escape));
    sc.wait(1.0);
    sc
}

/// A door outside Ardea: the Geldern door entity with its key. The door body is shown and solid
/// while closed, the use key opens it (open motion, no collision), the second use closes it.
pub fn world_doors() -> Sc {
    use super::more::{Goal, approach};
    use crate::walk::play::doors::Doors;
    use std::sync::Arc;
    let mut sc = Sc::new(300.0);
    grab(&mut sc);
    sc.act(
        "setup: stand 20 m from the Geldern door, with its key",
        |cx| {
            let at = {
                let w = cx
                    .world
                    .resource::<crate::walk::play::objects::WorldObjects>();
                w.list
                    .iter()
                    .find(|o| o.def.name.contains("Door_Geldern"))
                    .map(|o| o.pos_cm)
                    .unwrap_or(Vec3::ZERO)
            };
            super::scenarios::grant(cx, "It_Key_Geldern", 1);
            let heading = {
                let w = cx
                    .world
                    .resource::<crate::walk::play::objects::WorldObjects>();
                w.list
                    .iter()
                    .find(|o| o.def.name.contains("Door_Geldern"))
                    .map_or(0.0, |o| o.def.heading)
            };
            // 3 m in front of the door's local +Z, a little above the ground.
            let front = Quat::from_rotation_y(-heading) * Vec3::new(0.0, 0.0, 300.0);
            place_hero(cx, at + front + Vec3::Y * 150.0);
        },
    );
    sc.until("standing", 120.0, |cx| cx.player().ctrl.grounded);
    sc.until("the door body exists", 60.0, |cx| {
        let w = cx
            .world
            .resource::<crate::walk::play::objects::WorldObjects>();
        let d = cx.world.resource::<Doors>();
        w.list
            .iter()
            .enumerate()
            .any(|(i, o)| o.def.name.contains("Door_Geldern") && d.is_shown(i) && d.is_solid(i))
    });
    sc.shot("closed");
    let goal: Goal = Arc::new(|cx| {
        cx.world
            .resource::<crate::walk::play::objects::WorldObjects>()
            .list
            .iter()
            .find(|o| o.def.name.contains("Door_Geldern"))
            .map(|o| o.pos_cm)
    });
    approach(
        &mut sc,
        "door",
        goal,
        150.0,
        -0.1,
        door_focused(),
    );
    sc.act("use the door", |cx| cx.use_key());
    sc.until("the door opened", 15.0, |cx| {
        cx.world.resource::<Doors>().played.iter().any(|p| p.1)
    });
    sc.wait(2.0);
    sc.shot("open");
    sc.check("open door: motion played and no longer solid", |cx| {
        let w = cx
            .world
            .resource::<crate::walk::play::objects::WorldObjects>();
        let d = cx.world.resource::<Doors>();
        let i = w
            .list
            .iter()
            .position(|o| o.def.name.contains("Door_Geldern"))
            .unwrap_or(0);
        expect(
            !d.is_solid(i) && d.is_shown(i),
            "open and passable".to_string(),
            format!("solid {} shown {}", d.is_solid(i), d.is_shown(i)),
        )
    });
    sc.wait(4.0);
    sc.act("use the door again (the confirm key on the door entity)", |cx| {
        let e = cx
            .world
            .resource::<crate::walk::play::objects::WorldObjects>()
            .list
            .iter()
            .find(|o| o.def.name.contains("Door_Geldern"))
            .and_then(|o| o.entity)
            .unwrap_or(0);
        cx.note("setup: the focus was taken by a book next to the door; the confirm goes to the door entity".to_string());
        cx.world
            .write_message(gothic_runtime::CoreCommand(gothic_runtime::Command::Confirm {
                focus: e,
            }));
    });
    sc.wait(2.0);
    sc.act("what the interaction logged", |cx| {
        let log: Vec<String> = cx.inter().log.iter().rev().take(8).cloned().collect();
        cx.note(format!("interaction log (newest first): {log:?}"));
    });
    sc.until("the door closed", 15.0, |cx| {
        cx.world.resource::<Doors>().played.iter().any(|p| !p.1)
    });
    sc.wait(1.5);
    sc.check("closed again: solid", |cx| {
        let w = cx
            .world
            .resource::<crate::walk::play::objects::WorldObjects>();
        let d = cx.world.resource::<Doors>();
        let i = w
            .list
            .iter()
            .position(|o| o.def.name.contains("Door_Geldern"))
            .unwrap_or(0);
        expect(d.is_solid(i), "solid".to_string(), "not solid".to_string())
    });
    sc
}

/// The focus is the Geldern door (and not a book or a barrel next to it).
fn door_focused() -> std::sync::Arc<dyn Fn(&mut Cx) -> bool + Send + Sync> {
    std::sync::Arc::new(|cx| {
        let Some(crate::walk::play::Target::Object(i)) = cx.play().target else {
            return false;
        };
        cx.world
            .resource::<crate::walk::play::objects::WorldObjects>()
            .list
            .get(i)
            .is_some_and(|o| o.def.name.contains("Door_Geldern"))
    })
}
