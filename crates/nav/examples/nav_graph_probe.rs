//! Original navigation graph on the installed Ardea documents: load the authored zones and
//! corridors, run hero -> Hamlar, gate -> plaza and NPC routine-point queries, and time them.
//!
//!   cargo run --release -p gothic-nav --example nav_graph_probe -- "D:/.../Gothic 3/Data" [filter]
//!
//! Positions stay in Gothic space (identity mapping); nothing here is written to disk.
use glam::Vec3;
use gothic_formats::vfs::VirtualFileSystem;
use gothic_nav::{FindWayError, NavGraph, NavScene};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let data = a.get(1).expect("data dir");
    let filter = a
        .get(2)
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_else(|| "ardea".into());
    let mut vfs = VirtualFileSystem::open_data_dir(Path::new(data)).expect("open data");
    let slots: Vec<_> = vfs
        .iter()
        .filter(|(p, _)| {
            let l = p.to_ascii_lowercase();
            l.contains(&filter) && (l.ends_with(".node") || l.ends_with(".lrentdat"))
        })
        .map(|(p, s)| (p.to_owned(), s))
        .collect();
    let t = Instant::now();
    let mut scene = NavScene::default();
    for (path, slot) in &slots {
        let bytes = vfs.read(*slot).expect("read");
        if let Err(e) = scene.add_document(&bytes, true) {
            eprintln!("{path}: {e:#}");
        }
    }
    println!(
        "{} docs: {} zones, {} paths, {} offsets, {} navigations, {} entities in {:?}",
        slots.len(),
        scene.zones.len(),
        scene.paths.len(),
        scene.offsets.len(),
        scene.navigations.len(),
        scene.entities.len(),
        t.elapsed()
    );
    let t = Instant::now();
    let g = NavGraph::build(&scene, &|p| p);
    println!(
        "graph: {} zones {} corridors ({} dangling) built in {:?}",
        g.zone_count(),
        g.path_count(),
        g.dangling_paths,
        t.elapsed()
    );
    let mut nets: BTreeMap<u32, usize> = BTreeMap::new();
    for i in 0..g.zone_count() {
        *nets.entry(g.zone(i as u32).unwrap().net).or_default() += 1;
    }
    println!(
        "nets (zones per net): {:?}",
        nets.values().collect::<Vec<_>>()
    );

    let show = |label: &str, from: Vec3, to: Vec3| {
        let t = Instant::now();
        let r = g.find_way(from, to);
        let dt = t.elapsed();
        match r {
            Ok(r) => println!(
                "{label}: {} points, {:.0} cm (straight {:.0}), {} areas, {dt:?}",
                r.points.len(),
                r.length,
                ((to - from) * Vec3::new(1.0, 0.0, 1.0)).length(),
                r.areas.len()
            ),
            Err(e) => println!(
                "{label}: {e:?} (start {:?}, dest {:?}) {dt:?}",
                g.locate_near(from, 60.0),
                g.locate_near(to, 60.0)
            ),
        }
    };
    let hero = Vec3::new(87_984.94, 5_145.56, -10_197.48);
    let hamlar = Vec3::new(89_151.26, 5_155.97, -10_641.24);
    show("hero -> Hamlar", hero, hamlar);
    show("Hamlar -> hero", hamlar, hero);

    // Named places for the gate / plaza query.
    let find = |pat: &str| {
        scene
            .entities
            .values()
            .filter(|e| e.name.to_ascii_lowercase().contains(pat))
            .map(|e| (e.name.clone(), e.position()))
            .collect::<Vec<_>>()
    };
    for pat in ["sign_ardea", "trader", "hamlar"] {
        let v = find(pat);
        println!(
            "'{pat}': {} entities, first {:?}",
            v.len(),
            v.iter().take(4).collect::<Vec<_>>()
        );
    }

    g.warm();
    let mut names: BTreeMap<String, usize> = BTreeMap::new();
    for e in scene.entities.values() {
        let stem: String = e.name.split('_').take(2).collect::<Vec<_>>().join("_");
        *names.entry(stem).or_default() += 1;
    }
    let mut top: Vec<_> = names.into_iter().collect();
    top.sort_by_key(|x| std::cmp::Reverse(x.1));
    println!("entity stems: {:?}", &top[..top.len().min(40)]);

    // Character to character queries (every pair of NPC placements).
    {
        let spots: Vec<Vec3> = scene
            .navigations
            .iter()
            .map(|n| Vec3::new(n.matrix[3][0], n.matrix[3][1], n.matrix[3][2]))
            .collect();
        let (mut ok, mut fail, mut ms) = (0, BTreeMap::<String, usize>::new(), Vec::new());
        let mut ratio = Vec::new();
        for (i, &a) in spots.iter().enumerate() {
            for (j, &b) in spots.iter().enumerate() {
                if i == j {
                    continue;
                }
                let t = Instant::now();
                let r = g.find_way(a, b);
                ms.push(t.elapsed().as_secs_f64() * 1000.0);
                match r {
                    Ok(r) => {
                        ok += 1;
                        let st = ((b - a) * Vec3::new(1.0, 0.0, 1.0)).length();
                        if st > 500.0 {
                            ratio.push(f64::from(r.length / st));
                        }
                    }
                    Err(e) => *fail.entry(format!("{e:?}")).or_default() += 1,
                }
            }
        }
        ms.sort_by(|a, b| a.total_cmp(b));
        ratio.sort_by(|a, b| a.total_cmp(b));
        let pct = |v: &[f64], p: f64| {
            v.get(((v.len() as f64 - 1.0) * p) as usize)
                .copied()
                .unwrap_or(0.0)
        };
        println!(
            "npc pairs: {ok} ok, {fail:?}; ms p50 {:.4} p95 {:.4} p99 {:.4} max {:.3}; ratio p50 {:.2} p95 {:.2} max {:.2}",
            pct(&ms, 0.5),
            pct(&ms, 0.95),
            pct(&ms, 0.99),
            pct(&ms, 1.0),
            pct(&ratio, 0.5),
            pct(&ratio, 0.95),
            pct(&ratio, 1.0)
        );
    }

    // Routine points of every NPC with a navigation set.
    let mut ok = 0;
    let mut errs: BTreeMap<String, usize> = BTreeMap::new();
    let mut times = Vec::new();
    let mut ratios = Vec::new();
    for n in &scene.navigations {
        let from = Vec3::new(n.matrix[3][0], n.matrix[3][1], n.matrix[3][2]);
        for id in n
            .working_points
            .iter()
            .chain(&n.relaxing_points)
            .chain(&n.sleeping_points)
        {
            let Some(e) = scene.entities.get(id) else {
                *errs.entry("point entity not loaded".into()).or_default() += 1;
                if errs["point entity not loaded"] < 4 {
                    let hex: String = id.iter().map(|b| format!("{b:02x}")).collect();
                    println!("missing point {hex} of {}", n.name);
                }
                continue;
            };
            let t = Instant::now();
            let r = g.find_way(from, e.position());
            times.push(t.elapsed().as_secs_f64() * 1000.0);
            match r {
                Ok(r) => {
                    ok += 1;
                    let straight = ((e.position() - from) * Vec3::new(1.0, 0.0, 1.0)).length();
                    if straight > 200.0 {
                        ratios.push(f64::from(r.length / straight));
                    }
                }
                Err(FindWayError::InvalidStartZone) => {
                    *errs.entry("start outside zones".into()).or_default() += 1;
                }
                Err(e) => *errs.entry(format!("{e:?}")).or_default() += 1,
            }
        }
    }
    times.sort_by(|a, b| a.total_cmp(b));
    ratios.sort_by(|a, b| a.total_cmp(b));
    let pct = |v: &[f64], p: f64| {
        v.get(((v.len() as f64 - 1.0) * p) as usize)
            .copied()
            .unwrap_or(0.0)
    };
    println!(
        "routine queries: {ok} ok, failures {errs:?}; ms p50 {:.3} p95 {:.3} max {:.3}; length/straight p50 {:.2} p95 {:.2}",
        pct(&times, 0.5),
        pct(&times, 0.95),
        pct(&times, 1.0),
        pct(&ratios, 0.5),
        pct(&ratios, 0.95)
    );
}
