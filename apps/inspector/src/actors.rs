//! `actor-survey` / `motion-survey`: parse coverage over every `.xact` / `.xmot` in
//! `_compiledAnimation.pak`. Output is JSON (stdout or an explicit file under `.local/`).
use anyhow::{Context, Result, ensure};
use gothic_formats::{PakArchive, xact::Actor, xmot::Motion};
use serde_json::{Value, json};
use std::{collections::BTreeMap, ffi::OsString, path::Path};

pub fn dispatch(command: &str, args: &[OsString]) -> Result<bool> {
    if !matches!(command, "actor-survey" | "motion-survey") {
        return Ok(false);
    }
    ensure!(args.len() >= 2, "{command} requires DATA_DIR [OUTPUT_JSON]");
    let data_dir = Path::new(&args[1]);
    let mut pak = PakArchive::open(data_dir.join("_compiledAnimation.pak"))?;
    let report = if command == "actor-survey" {
        actor_survey(&mut pak)?
    } else {
        motion_survey(&mut pak)?
    };
    match args.get(2) {
        Some(out) => std::fs::write(out, serde_json::to_vec_pretty(&report)?)?,
        None => serde_json::to_writer_pretty(std::io::stdout().lock(), &report)?,
    }
    Ok(true)
}

fn members(pak: &PakArchive, extension: &str) -> Vec<usize> {
    pak.entries
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            e.path.to_ascii_lowercase().ends_with(extension) && !e.deleted() && !e.payloadless()
        })
        .map(|(i, _)| i)
        .collect()
}

fn actor_survey(pak: &mut PakArchive) -> Result<Value> {
    let indices = members(pak, ".xact");
    let mut ok = 0usize;
    let mut failures: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut files = Vec::new();
    let (mut skinned, mut rigid, mut vertices, mut triangles) = (0usize, 0usize, 0usize, 0usize);
    let mut max_influences = 0usize;
    let mut leftovers: BTreeMap<usize, usize> = BTreeMap::new();
    let mut resolved_material_files = 0usize;
    let mut materials = 0usize;
    for index in indices {
        let path = pak.entries[index].path.clone();
        let result = pak
            .read_payload(index)
            .and_then(|bytes| Actor::parse(&bytes));
        match result {
            Ok(actor) => {
                ok += 1;
                *leftovers.entry(actor.leftover).or_default() += 1;
                let mut verts = 0usize;
                let mut tris = 0usize;
                for mesh in &actor.meshes {
                    if mesh.skinned {
                        skinned += 1;
                    } else {
                        rigid += 1;
                    }
                    max_influences = max_influences.max(mesh.max_influences);
                    for sub in &mesh.submeshes {
                        verts += sub.positions.len();
                        tris += sub.indices.len() / 3;
                    }
                }
                vertices += verts;
                triangles += tris;
                materials += actor.materials.len().saturating_sub(1);
                resolved_material_files += actor.material_files.len();
                files.push(json!({
                    "path": path,
                    "nodes": actor.nodes.len(),
                    "meshes": actor.meshes.len(),
                    "materials": actor.materials.len(),
                    "material_files": actor.material_files.len(),
                    "vertices": verts,
                    "triangles": tris,
                    "chunks": actor.chunk_counts,
                    "leftover": actor.leftover,
                }));
            }
            Err(error) => failures.entry(format!("{error:#}")).or_default().push(path),
        }
    }
    let total = ok + failures.values().map(Vec::len).sum::<usize>();
    eprintln!("actors: {ok}/{total} parsed");
    Ok(json!({
        "archive": "_compiledAnimation.pak",
        "total": total,
        "parsed": ok,
        "coverage": ok as f64 / total.max(1) as f64,
        "skinned_meshes": skinned,
        "rigid_meshes": rigid,
        "vertices": vertices,
        "triangles": triangles,
        "max_influences_per_vertex": max_influences,
        "leftover_histogram": leftovers,
        "non_default_materials": materials,
        "xshmat_names_in_tables": resolved_material_files,
        "failures": failures,
        "files": files,
    }))
}

fn motion_survey(pak: &mut PakArchive) -> Result<Value> {
    let indices = members(pak, ".xmot");
    // Skeleton reference: the player body actor, if present.
    let reference = {
        let at = pak
            .entries
            .iter()
            .position(|e| e.path.eq_ignore_ascii_case("G3_Hero_Body_Player.xact"))
            .context("reference actor G3_Hero_Body_Player.xact not found")?;
        Actor::parse(&pak.read_payload(at)?)?
    };
    let mut ok = 0usize;
    let mut failures: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let (mut pose_only, mut keys, mut nodes) = (0usize, 0usize, 0usize);
    let mut longest = (0f32, String::new());
    let mut intervals: BTreeMap<String, usize> = BTreeMap::new();
    // How many motions address only bones present in the hero skeleton.
    let mut hero_compatible = 0usize;
    let mut hero_names: BTreeMap<String, usize> = BTreeMap::new();
    for index in indices {
        let path = pak.entries[index].path.clone();
        let result = pak
            .read_payload(index)
            .and_then(|bytes| Motion::parse(&bytes));
        match result {
            Ok(motion) => {
                ok += 1;
                keys += motion.key_count;
                nodes += motion.nodes.len();
                if motion.key_count == 0 {
                    pose_only += 1;
                }
                if motion.duration > longest.0 {
                    longest = (motion.duration, path.clone());
                }
                if motion.min_key_interval > 0.0 {
                    *intervals
                        .entry(format!("{:.4}", motion.min_key_interval))
                        .or_default() += 1;
                }
                let matched = motion
                    .nodes
                    .iter()
                    .filter(|n| reference.node_index(&n.name).is_some())
                    .count();
                if matched * 10 >= motion.nodes.len() * 9 {
                    hero_compatible += 1;
                    if let Some(prefix) = path.split('_').next() {
                        *hero_names.entry(prefix.to_string()).or_default() += 1;
                    }
                }
            }
            Err(error) => failures.entry(format!("{error:#}")).or_default().push(path),
        }
    }
    let total = ok + failures.values().map(Vec::len).sum::<usize>();
    eprintln!("motions: {ok}/{total} parsed");
    let mut common: Vec<_> = intervals.into_iter().collect();
    common.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    common.truncate(12);
    Ok(json!({
        "archive": "_compiledAnimation.pak",
        "total": total,
        "parsed": ok,
        "coverage": ok as f64 / total.max(1) as f64,
        "pose_only": pose_only,
        "keys": keys,
        "node_records": nodes,
        "longest_seconds": longest.0,
        "longest_path": longest.1,
        "min_key_interval_histogram": common,
        "matching_90pct_of_hero_player_bones": hero_compatible,
        "hero_compatible_by_prefix": hero_names,
        "failures": failures,
    }))
}
