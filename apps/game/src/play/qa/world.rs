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
    sc.step("walk to Reddock", |cx| {
        let goal = cluster(cx, &["reddock_npc_01"]).unwrap_or(Vec3::ZERO);
        if cx.t > 170.0 {
            cx.note("walk timed out: continuing from where the hero stands");
            cx.key_up(KeyCode::KeyW);
            return Poll::Done;
        }
        cx.walk_to(goal, 900.0, true)
    });
    sc.act("record where the walk ended", |cx| {
        let goal = cluster(cx, &["reddock_npc_01"]).unwrap_or(Vec3::ZERO);
        let p = cx.player().ctrl.position;
        let d = Vec2::new(p.x - goal.x, p.z - goal.z).length();
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
    sc.act("stop the frame clock", |cx| {
        cx.st.n.insert("ft_on".into(), 0.0);
    });
    report_frames(&mut sc, "walk to Reddock", 24);
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
