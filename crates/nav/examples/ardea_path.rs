//! Headless Ardea scenario on the cooked cache: build the collision world around Ardea, bake the
//! navmesh, find paths hero start -> Hamlar and across the city, and walk them with the
//! character controller (slopes, steps, timing).
//!
//!   cargo run --release -p gothic-nav --example ardea_path -- [--cache DIR]
//!
//! Coordinates are the cooked ones (Gothic X mirrored, centimetres).
use anyhow::{Context, Result};
use glam::{Vec2, Vec3};
use gothic_cook::Cache;
use gothic_nav::{NavMesh, NavParams, PathFollower};
use gothic_physics::{CharacterController, CollisionWorld, MoveMode, MovementProfile};
use std::time::Instant;

/// Gothic position -> cooked space (X mirrored).
fn cooked(p: [f32; 3]) -> Vec3 {
    Vec3::from(gothic_coords::arr3(p))
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let arg = |n: &str| {
        args.iter()
            .position(|a| a == n)
            .and_then(|i| args.get(i + 1))
    };
    let root = arg("--cache")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| gothic_cook::default_cache_root().join("desktop-high"));
    let cache = Cache::open(&root).context("open cache")?;

    // PC_Hero start and the NPC Hamlar (positions from the world data), Gothic space.
    let hero = cooked([87_984.94, 5_145.56, -10_197.48]);
    let hamlar = cooked([89_151.26, 5_155.97, -10_641.24]);
    let centre = cooked([91_000.0, 0.0, -11_000.0]);

    let t = Instant::now();
    let mut world = CollisionWorld::new();
    let (mut tris, mut props) = (0usize, 0usize);
    let keep = |v: Vec3| (v.x - hero.x).abs() < 16_000.0 && (v.z - hero.z).abs() < 16_000.0;
    for gx in 7..=9 {
        for gz in -3..=-1 {
            let Some(cell) = cache.load_cell(gx, gz)? else {
                continue;
            };
            let mut list: Vec<[Vec3; 3]> = Vec::new();
            for tri in cell.collision_triangles() {
                let t = tri.map(Vec3::from);
                if keep(t[0]) {
                    list.push(t);
                    tris += 1;
                }
            }
            for inst in cell.instances() {
                if inst.collision == 0 {
                    continue;
                }
                let tr = inst.transform;
                let origin = Vec3::from(tr[3]);
                if !keep(origin) {
                    continue;
                }
                let Some(mesh) = cache.collision(inst.collision) else {
                    continue;
                };
                let m = |v: [f32; 3]| {
                    Vec3::from(tr[0]) * v[0]
                        + Vec3::from(tr[1]) * v[1]
                        + Vec3::from(tr[2]) * v[2]
                        + origin
                };
                for t in mesh.indices.as_chunks::<3>().0 {
                    list.push([
                        m(mesh.vertices[t[0] as usize]),
                        m(mesh.vertices[t[1] as usize]),
                        m(mesh.vertices[t[2] as usize]),
                    ]);
                    props += 1;
                }
            }
            world.insert_chunk(((gx + 100) as u64) << 32 | (gz + 100) as u64, list);
        }
    }
    println!(
        "collision: {tris} terrain + {props} prop triangles in {:.2} s",
        t.elapsed().as_secs_f32()
    );

    let ground = world
        .ground_below(Vec3::new(hero.x, hero.y + 500.0, hero.z), 3000.0)
        .context("no ground under the hero start")?;
    println!("ground under hero start {ground:.0} (data y {:.0})", hero.y);

    let params = NavParams::for_profile(MovementProfile::human_npc());
    println!(
        "nav: cell {} cm, radius {:.1}, step {}, max slope {:.1} deg",
        params.cell, params.radius, params.step_height, params.max_slope_deg
    );
    let mut nav = NavMesh::new(params);
    let t = Instant::now();
    let n = nav.bake_disc(&world, Vec3::new(hero.x, ground, hero.z), 11_000.0);
    println!(
        "navmesh: {n} walkable cells ({} pending) in {:.2} s",
        nav.pending(),
        t.elapsed().as_secs_f32()
    );

    // Hero start -> Hamlar.
    let report = |name: &str, from: Vec3, to: Vec3| -> Option<Vec<Vec3>> {
        let t = Instant::now();
        let p = nav.find_path(from, to);
        let dt = t.elapsed();
        match p {
            Some(p) => {
                let straight = Vec2::new(from.x - to.x, from.z - to.z).length();
                println!(
                    "{name}: {} waypoints, {:.0} cm path vs {straight:.0} cm straight, {:.2} ms",
                    p.points.len(),
                    p.length,
                    dt.as_secs_f64() * 1000.0
                );
                Some(p.points)
            }
            None => {
                println!("{name}: no path ({:.2} ms)", dt.as_secs_f64() * 1000.0);
                None
            }
        }
    };
    let h = Vec3::new(
        hamlar.x,
        nav.height_at(hamlar.x, hamlar.z).unwrap_or(hamlar.y),
        hamlar.z,
    );
    let near_h = nav.is_walkable(h.x, h.z);
    println!("Hamlar cell walkable: {near_h}");
    let p1 = report("hero start -> Hamlar", Vec3::new(hero.x, ground, hero.z), h);

    // City gate stand-in: the walkable cell farthest from Hamlar on the Ardea perimeter ring.
    let mut gate: Option<(f32, Vec3)> = None;
    for ang in (0..720).map(|i| i as f32 * 0.5f32.to_radians()) {
        for r in [6_800.0, 6_000.0, 5_200.0] {
            let (x, z) = (centre.x + ang.cos() * r, centre.z + ang.sin() * r);
            if let Some(y) = nav.height_at(x, z) {
                let d = Vec2::new(x - h.x, z - h.z).length();
                if gate.is_none_or(|g| d > g.0) {
                    gate = Some((d, Vec3::new(x, y, z)));
                }
                break;
            }
        }
    }
    let p2 = gate.and_then(|(d, g)| {
        println!(
            "perimeter point {:.0} cm from Hamlar at ({:.0}, {:.0}, {:.0})",
            d, g.x, g.y, g.z
        );
        report("perimeter -> Hamlar", g, h)
    });

    if let Some(dir) = arg("--dump") {
        let rows: Vec<_> = [&p1, &p2]
            .iter()
            .map(|p| {
                p.as_ref()
                    .map(|v| v.iter().map(|p| [p.x, p.y, p.z]).collect::<Vec<_>>())
            })
            .collect();
        std::fs::write(dir, format!("{rows:?}"))?;
    }
    // Drive the controller along the paths.
    for (name, path) in [("hero start -> Hamlar", p1), ("perimeter -> Hamlar", p2)] {
        let Some(path) = path else { continue };
        walk(name, &world, &nav, path)?;
    }
    Ok(())
}

/// Largest rise between 40 cm samples and steepest sample-to-sample angle along a polyline.
fn profile(nav: &NavMesh, pts: &[Vec3]) -> (f32, f32, f32) {
    let (mut rise, mut angle, mut total_up) = (0.0f32, 0.0f32, 0.0f32);
    let mut last: Option<f32> = None;
    for w in pts.windows(2) {
        let n = ((w[1] - w[0]).length() / 40.0).ceil().max(1.0) as usize;
        for i in 0..=n {
            let p = w[0].lerp(w[1], i as f32 / n as f32);
            if let Some(y) = nav.height_at(p.x, p.z) {
                if let Some(l) = last {
                    let d = y - l;
                    rise = rise.max(d.abs());
                    angle = angle.max((d.abs() / 40.0).atan().to_degrees());
                    total_up += d.max(0.0);
                }
                last = Some(y);
            }
        }
    }
    (rise, angle, total_up)
}

fn walk(name: &str, world: &CollisionWorld, nav: &NavMesh, points: Vec<Vec3>) -> Result<()> {
    let (rise, angle, up) = profile(nav, &points);
    println!(
        "path profile {name}: max rise {rise:.0} cm per 40 cm, steepest {angle:.0} deg, total climb {up:.0} cm"
    );
    let start = points[0];
    let goal = *points.last().context("empty path")?;
    let mut c = CharacterController::new(MovementProfile::hero(), start + Vec3::Y * 5.0);
    let mut follower = PathFollower {
        path: points,
        next: 1,
        goal,
    };
    let dt = 1.0 / 60.0;
    let (mut t, mut max_step, mut max_slope, mut fell, mut slid) = (0.0f32, 0.0f32, 0.0f32, 0, 0);
    let mut last_y = c.position.y;
    let mut dist = 0.0f32;
    for _ in 0..(120.0 / dt) as usize {
        // Steer towards the next waypoint like an NPC at run speed.
        let target = follower.path.get(follower.next).copied().unwrap_or(goal);
        let to = Vec3::new(target.x - c.position.x, 0.0, target.z - c.position.z);
        if to.length() < 30.0 && follower.next < follower.path.len() {
            follower.next += 1;
        }
        let dir = to.normalize_or_zero();
        let before = c.position;
        let r = c.step_gait(world, dir, gothic_physics::Gait::Run, false, dt);
        t += dt;
        dist += Vec2::new(c.position.x - before.x, c.position.z - before.z).length();
        max_step = max_step.max((c.position.y - last_y).abs());
        last_y = c.position.y;
        max_slope = max_slope.max(c.ground_normal.y.clamp(-1.0, 1.0).acos().to_degrees());
        if r.took_off {
            fell += 1;
        }
        if r.started_sliding {
            slid += 1;
        }
        if Vec2::new(c.position.x - goal.x, c.position.z - goal.z).length() < 40.0 {
            break;
        }
    }
    let d = Vec2::new(c.position.x - goal.x, c.position.z - goal.z).length();
    println!(
        "walk {name}: {} after {t:.1} s, {dist:.0} cm walked (mean {:.0} cm/s), {d:.0} cm from goal, max frame climb {max_step:.1} cm, max ground angle {max_slope:.0} deg, falls {fell}, slides {slid}, mode {:?}",
        if d < 60.0 { "arrived" } else { "STUCK" },
        dist / t.max(0.01),
        c.mode
    );
    debug_assert!(c.mode != MoveMode::Slide || d < 60.0);
    Ok(())
}
