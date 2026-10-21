//! The original navigation graph on the installed Ardea documents. Skipped when the game data
//! is not present (set `GOTHIC3_DATA` to the `Data` folder to point elsewhere).
use glam::Vec3;
use gothic_formats::vfs::VirtualFileSystem;
use gothic_nav::{AreaId, NavGraph, NavScene};
use std::path::PathBuf;
use std::time::Instant;

fn data_dir() -> Option<PathBuf> {
    let p = std::env::var_os("GOTHIC3_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"D:\SteamLibrary\steamapps\common\Gothic 3\Data"));
    p.is_dir().then_some(p)
}

fn load() -> Option<(NavScene, NavGraph)> {
    let dir = data_dir()?;
    let mut vfs = VirtualFileSystem::open_data_dir(&dir).ok()?;
    let slots: Vec<_> = vfs
        .iter()
        .filter(|(p, _)| {
            let l = p.to_ascii_lowercase();
            l.contains("ardea") && (l.ends_with(".node") || l.ends_with(".lrentdat"))
        })
        .map(|(p, s)| (p.to_owned(), s))
        .collect();
    let mut scene = NavScene::default();
    for (_, slot) in &slots {
        let bytes = vfs.read(*slot).ok()?;
        scene.add_document(&bytes, true).ok()?;
    }
    let graph = NavGraph::build(&scene, &|p| p);
    graph.warm();
    Some((scene, graph))
}

fn flat(a: Vec3, b: Vec3) -> f32 {
    ((a - b) * Vec3::new(1.0, 0.0, 1.0)).length()
}

/// Every sample along the route lies inside some zone or corridor.
fn assert_route_on_ground(g: &NavGraph, pts: &[Vec3]) {
    for w in pts.windows(2) {
        let n = (flat(w[0], w[1]) / 150.0).ceil().max(1.0) as i32;
        for i in 0..=n {
            let p = w[0].lerp(w[1], i as f32 / n as f32);
            // Heights between waypoints are interpolated; the band is generous enough.
            assert!(
                g.locate_near(p, 80.0).is_some(),
                "route leaves the navigation areas at {p:?} (leg {:?} -> {:?})",
                w[0],
                w[1]
            );
        }
    }
}

#[test]
fn hero_start_to_hamlar_and_gate_to_plaza() {
    let Some((scene, g)) = load() else {
        eprintln!("skipped: no game data");
        return;
    };
    assert!(g.zone_count() >= 25, "{} zones", g.zone_count());
    assert!(g.path_count() >= 20, "{} corridors", g.path_count());

    // PC_Hero start and Hamlar (positions from the world documents, Gothic space).
    let hero = Vec3::new(87_984.94, 5_145.56, -10_197.48);
    let hamlar = Vec3::new(89_151.26, 5_155.97, -10_641.24);
    let r = g.find_way(hero, hamlar).expect("hero -> Hamlar");
    let straight = flat(hero, hamlar);
    assert!(
        r.length >= straight - 1.0 && r.length < straight * 1.5,
        "{}",
        r.length
    );
    assert_route_on_ground(&g, &r.points);

    // The outer ring (largest zone) meets the city through a corridor: that is the gate.
    let big = (0..g.zone_count() as u32)
        .max_by(|a, b| {
            let area = |i: u32| {
                let o = g.zone(i).unwrap().outline();
                let (mut lo, mut hi) = (o[0], o[0]);
                for p in o {
                    lo = lo.min(*p);
                    hi = hi.max(*p);
                }
                (hi - lo).length()
            };
            area(*a).total_cmp(&area(*b))
        })
        .unwrap();
    let gate = (0..g.path_count() as u32)
        .filter_map(|i| g.corridor(i))
        .find(|c| c.zone_a == big || c.zone_b == big)
        .map(|c| c.at(c.len() * 0.5))
        .expect("a corridor of the largest zone");
    // A trader's stand on the market (position of the entity named Trader).
    let plaza = scene
        .entities
        .values()
        .filter(|e| e.name == "Trader")
        .map(|e| e.position())
        .find(|p| g.locate(*p).is_some())
        .expect("a trader inside the graph");
    let r = g.find_way(gate, plaza).expect("gate -> plaza");
    assert!(r.length >= flat(gate, plaza) - 1.0);
    assert!(
        r.length < flat(gate, plaza) * 4.0,
        "{} vs {}",
        r.length,
        flat(gate, plaza)
    );
    assert_route_on_ground(&g, &r.points);
    assert!(
        r.areas.len() >= 2,
        "a gate to plaza route passes several areas: {:?}",
        r.areas
    );
    // Reverse direction has the same length.
    let back = g.find_way(plaza, gate).unwrap();
    assert!((back.length - r.length).abs() < 5.0);
}

#[test]
fn npc_routine_navigation_reaches_the_work_stations() {
    let Some((scene, g)) = load() else {
        eprintln!("skipped: no game data");
        return;
    };
    let mut queries = 0;
    for n in &scene.navigations {
        let from = Vec3::new(n.matrix[3][0], n.matrix[3][1], n.matrix[3][2]);
        for id in n
            .working_points
            .iter()
            .chain(&n.relaxing_points)
            .chain(&n.sleeping_points)
        {
            // Points in static geometry layers are not in the dynamic documents: skipped.
            let Some(e) = scene.entities.get(id) else {
                continue;
            };
            let to = e.position();
            let r = g
                .find_way(from, to)
                .unwrap_or_else(|e2| panic!("{} -> {} ({to:?}): {e2:?}", n.name, e.name));
            assert!(r.length >= flat(from, to) - 1.0);
            assert_route_on_ground(&g, &r.points);
            queries += 1;
        }
    }
    assert!(queries >= 40, "only {queries} routine queries");
}

#[test]
fn every_npc_pair_is_routable_and_fast() {
    let Some((scene, g)) = load() else {
        eprintln!("skipped: no game data");
        return;
    };
    let spots: Vec<Vec3> = scene
        .navigations
        .iter()
        .map(|n| Vec3::new(n.matrix[3][0], n.matrix[3][1], n.matrix[3][2]))
        .collect();
    let mut ms = Vec::new();
    for (i, &a) in spots.iter().enumerate() {
        for (j, &b) in spots.iter().enumerate() {
            if i == j {
                continue;
            }
            let t = Instant::now();
            let r = g.find_way(a, b);
            ms.push(t.elapsed().as_secs_f64() * 1000.0);
            let r = r.unwrap_or_else(|e| panic!("npc {i} -> {j}: {e:?}"));
            if (i * 7 + j) % 11 == 0 {
                assert_route_on_ground(&g, &r.points);
            }
        }
    }
    ms.sort_by(|a, b| a.total_cmp(b));
    let p95 = ms[(ms.len() as f64 * 0.95) as usize];
    eprintln!(
        "{} queries, p50 {:.4} ms, p95 {p95:.4} ms, max {:.3} ms",
        ms.len(),
        ms[ms.len() / 2],
        ms[ms.len() - 1]
    );
    assert!(p95 < 1.0, "p95 {p95} ms");
}

#[test]
fn offsets_of_interaction_objects_load() {
    let Some((scene, g)) = load() else {
        eprintln!("skipped: no game data");
        return;
    };
    assert!(scene.offsets.len() >= 40);
    let campfire = scene
        .offsets
        .iter()
        .find(|o| o.name.starts_with("G3_Object_Interact_Campfire"))
        .expect("campfire offset");
    let off = gothic_nav::NavOffset::from_record(campfire, &|p| p);
    let pos = Vec3::new(
        campfire.matrix[3][0],
        campfire.matrix[3][1],
        campfire.matrix[3][2],
    );
    if let Some(area) = g.locate_near(pos, 400.0) {
        let from = pos + Vec3::new(300.0, 0.0, 0.0);
        if let Some(c) = off.choose(&g, area, from) {
            assert!(flat(c.position, pos) < 600.0);
            assert!((c.facing.length() - 1.0).abs() < 1.0e-3);
        }
    }
    assert!(matches!(
        g.locate(pos),
        Some(AreaId::Zone(_)) | Some(AreaId::Path(_)) | None
    ));
}
