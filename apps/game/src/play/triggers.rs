//! World triggers fed by physics contacts.
//!
//! The shipped world documents contain exactly one trigger-family property set: `gCTouchDamage_PS`
//! (698 entities, all in the main dynamic document; a loose weapon or arrow each), which is an
//! `eCTrigger_PS` with the script function `OnTouchDamage`. The `eCTrigger_PS`, `eCTriggerList_PS`,
//! `eCEventFilter_PS` and `gCEventScript_PS` classes themselves occur in no parsed document.
//!
//! This module is the host side of the ported trigger runtime (`gothic_world::trigger`): it reads
//! the `gCTouchDamage_PS` property sets once (cached in `.local/touch_triggers.json`), registers
//! the triggers inside the entity processing range of the hero with the core
//! (`Core::triggers`, entities in the core's scene model), turns the hero capsule touching a
//! trigger's shape into `OnTouch` / `OnUntouch`, runs the trigger clock, and records what fired.
//! The shape is approximated by a sphere around the entity origin: the collision shape groups
//! and the mesh collision are not available to the host (declared approximation).
use super::PlayState;
use crate::walk::{ORIGIN, Player};
use crate::world_loader::Origin;
use bevy::prelude::*;
use gothic_formats::propstream::{DecodeStats, PropValue, PropertySet};
use gothic_formats::vfs::VirtualFileSystem;
use gothic_formats::world_node::WorldDocument;
use gothic_runtime::CoreRes;
use gothic_world::pvs::EntityId;
use gothic_world::trigger::{
    Contact, ContactKind, TriggerDef, TriggerEnv, TriggerKind, TriggerOutput,
};
use std::collections::{HashMap, HashSet};
use std::path::Path;

const CACHE: &str = ".local/touch_triggers.json";
const CLASS: &str = "gCTouchDamage_PS";
/// Triggers are live inside this box around the hero (the entity ROI of the processing range).
const ROI: f32 = 4_000.0;
const ROI_EXIT: f32 = 4_250.0;
/// Sphere that stands in for the trigger's collision shape (cm).
const SHAPE_RADIUS: f32 = 60.0;
/// Half height and radius of the hero capsule (cm).
const HERO_HALF_HEIGHT: f32 = 90.0;
const HERO_RADIUS: f32 = 35.0;
const GRID: f32 = 2_500.0;

/// One trigger of the world.
pub struct TrigSrc {
    pub name: String,
    pub doc: String,
    /// Mirrored, origin-relative centimetres.
    pub pos_cm: Vec3,
    pub def: TriggerDef,
}

/// What a fired trigger reported.
#[derive(Debug, Clone)]
pub struct Fired {
    pub name: String,
    pub script: String,
    pub at: f64,
}

#[derive(Resource, Default)]
pub struct WorldTriggers {
    pub src: Vec<TrigSrc>,
    grid: HashMap<(i32, i32), Vec<u32>>,
    live: HashMap<u32, EntityId>,
    touching: HashSet<u32>,
    hero: Option<EntityId>,
    last: f32,
    /// Script triggers that fired, in order.
    pub fired: Vec<Fired>,
    /// Events delivered to entities without a trigger of their own (door, light ...).
    pub delivered: u32,
    pub touches: u32,
    pub untouches: u32,
}

impl WorldTriggers {
    /// Triggers currently registered with the core.
    pub fn live_count(&self) -> usize {
        self.live.len()
    }

    /// The trigger nearest to `p` (mirrored cm) among all of the world.
    pub fn nearest(&self, p: Vec3) -> Option<usize> {
        (0..self.src.len()).min_by(|a, b| {
            let d = |i: &usize| (self.src[*i].pos_cm - p).length();
            d(a).total_cmp(&d(b))
        })
    }

    pub fn is_live(&self, i: usize) -> bool {
        self.live.contains_key(&(i as u32))
    }
}

fn cell(p: Vec3) -> (i32, i32) {
    ((p.x / GRID).floor() as i32, (p.z / GRID).floor() as i32)
}

fn prop<'a>(s: &'a PropertySet, name: &str) -> Option<&'a PropValue> {
    s.get(name)
}

fn b(s: &PropertySet, n: &str, d: bool) -> bool {
    match prop(s, n) {
        Some(PropValue::Bool(v)) => *v,
        _ => d,
    }
}

fn f(s: &PropertySet, n: &str, d: f32) -> f32 {
    match prop(s, n) {
        Some(PropValue::F32(v)) => *v,
        _ => d,
    }
}

fn i(s: &PropertySet, n: &str, d: i32) -> i32 {
    match prop(s, n) {
        Some(PropValue::I16(v)) => i32::from(*v),
        Some(PropValue::I32(v)) => *v,
        Some(PropValue::U16(v)) => i32::from(*v),
        Some(PropValue::Enum { value, .. }) => *value,
        _ => d,
    }
}

fn t(s: &PropertySet, n: &str) -> String {
    match prop(s, n) {
        Some(PropValue::Str(v)) => v.clone(),
        _ => String::new(),
    }
}

/// `TriggerDef` of an `eCTrigger_PS`-family property set (the property names of the class).
pub fn def_of(s: &PropertySet) -> TriggerDef {
    let script = t(s, "ScriptTouchFunc");
    let mut d = TriggerDef::new(&t(s, "TargetEntity"));
    if !script.is_empty() {
        d.kind = TriggerKind::Script { script };
    }
    d.enabled = b(s, "IsEnabled", true);
    d.max_activations = i(s, "MaxActivationCount", -1);
    d.max_per_entity = i(s, "MaxCountPerEntity", -1);
    d.fire_delay = f(s, "FireDelaySec", 0.0);
    d.retrigger_wait = f(s, "RetriggerWaitSec", 0.0);
    d.auto_untrigger = f(s, "AutoUntriggerAfterSec", 0.0);
    d.damage_threshold = i(s, "DamageTreshold", 0);
    d.velocity_threshold = f(s, "VelocityTresholdMSec", 0.0);
    d.send_untrigger = b(s, "SendUntrigger", true);
    d.react_trigger = b(s, "ReactToTrigger", true);
    d.react_touch = b(s, "ReactToTouch", true);
    d.react_damage = b(s, "ReactToDamage", false);
    d.react_intersect = b(s, "ReactToIntersect", false);
    d.react_processing_range = b(s, "ReactToProcessingRange", false);
    d.react_cache_in = b(s, "ReactToCacheInRange", false);
    d.recognize_players = b(s, "RecognizesPlayers", true);
    d.recognize_npcs = b(s, "RecognizesNPCs", false);
    d.recognize_objects = b(s, "RecognizesObjects", false);
    d.recognize_name = t(s, "RecognizesEntityName");
    d.recognize_ps_type = i(s, "RecognizesPSType", 0).max(0) as u16;
    d.recognize_shape_group = i(s, "RecognizesShapeGroup", 0).max(0) as u16;
    d
}

fn to_json(def: &TriggerDef, script: &str) -> serde_json::Value {
    serde_json::json!({
        "script": script, "target": def.target, "enabled": def.enabled,
        "max_activations": def.max_activations, "max_per_entity": def.max_per_entity,
        "fire_delay": def.fire_delay, "retrigger_wait": def.retrigger_wait,
        "auto_untrigger": def.auto_untrigger, "damage_threshold": def.damage_threshold,
        "velocity_threshold": def.velocity_threshold, "send_untrigger": def.send_untrigger,
        "react_trigger": def.react_trigger, "react_touch": def.react_touch,
        "react_damage": def.react_damage, "react_intersect": def.react_intersect,
        "react_processing_range": def.react_processing_range,
        "react_cache_in": def.react_cache_in, "players": def.recognize_players,
        "npcs": def.recognize_npcs, "objects": def.recognize_objects,
        "name": def.recognize_name, "ps_type": def.recognize_ps_type,
        "shape_group": def.recognize_shape_group,
    })
}

fn from_json(v: &serde_json::Value) -> TriggerDef {
    let s = |k: &str| v[k].as_str().unwrap_or("").to_owned();
    let n = |k: &str, d: i64| v[k].as_i64().unwrap_or(d) as i32;
    let x = |k: &str| v[k].as_f64().unwrap_or(0.0) as f32;
    let bo = |k: &str, d: bool| v[k].as_bool().unwrap_or(d);
    let mut d = TriggerDef::new(&s("target"));
    let script = s("script");
    if !script.is_empty() {
        d.kind = TriggerKind::Script { script };
    }
    d.enabled = bo("enabled", true);
    d.max_activations = n("max_activations", -1);
    d.max_per_entity = n("max_per_entity", -1);
    d.fire_delay = x("fire_delay");
    d.retrigger_wait = x("retrigger_wait");
    d.auto_untrigger = x("auto_untrigger");
    d.damage_threshold = n("damage_threshold", 0);
    d.velocity_threshold = x("velocity_threshold");
    d.send_untrigger = bo("send_untrigger", true);
    d.react_trigger = bo("react_trigger", true);
    d.react_touch = bo("react_touch", true);
    d.react_damage = bo("react_damage", false);
    d.react_intersect = bo("react_intersect", false);
    d.react_processing_range = bo("react_processing_range", false);
    d.react_cache_in = bo("react_cache_in", false);
    d.recognize_players = bo("players", true);
    d.recognize_npcs = bo("npcs", false);
    d.recognize_objects = bo("objects", false);
    d.recognize_name = s("name");
    d.recognize_ps_type = n("ps_type", 0).max(0) as u16;
    d.recognize_shape_group = n("shape_group", 0).max(0) as u16;
    d
}

/// All `gCTouchDamage_PS` triggers of the world, from the cache or the installation.
pub fn load(data_dir: &Path) -> Vec<TrigSrc> {
    let origin = Origin(ORIGIN);
    let rows: Vec<serde_json::Value> = std::fs::read(CACHE)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_else(|| {
            let t0 = std::time::Instant::now();
            let mut rows = Vec::new();
            let Ok(mut vfs) = VirtualFileSystem::open_data_dir(data_dir) else {
                return rows;
            };
            let slots: Vec<_> = vfs
                .iter()
                .filter(|(p, _)| {
                    let l = p.to_ascii_lowercase();
                    l.ends_with(".node") || l.ends_with(".lrentdat")
                })
                .map(|(p, s)| (p.to_owned(), s))
                .collect();
            for (path, slot) in slots {
                let Ok(bytes) = vfs.read(slot) else { continue };
                // Cheap pre-check: the class name is a string-table entry of the document.
                if !bytes.windows(CLASS.len()).any(|w| w == CLASS.as_bytes()) {
                    continue;
                }
                let mut stats = DecodeStats::default();
                let Ok((doc, sets)) =
                    WorldDocument::parse_with_properties(&bytes, &|c| c == CLASS, &mut stats)
                else {
                    continue;
                };
                for (n, e) in doc.entities.iter().enumerate() {
                    let Some(set) = sets
                        .get(n)
                        .and_then(|s| s.iter().find(|s| s.class == CLASS))
                    else {
                        continue;
                    };
                    let def = def_of(set);
                    let script = t(set, "ScriptTouchFunc");
                    let m = e.world_matrix[3];
                    let mut row = to_json(&def, &script);
                    row["entity"] = e.name.clone().into();
                    row["doc"] = path.to_ascii_lowercase().into();
                    row["position"] = serde_json::json!([m[0], m[1], m[2]]);
                    rows.push(row);
                }
            }
            eprintln!(
                "play: {} {CLASS} triggers read in {} ms",
                rows.len(),
                t0.elapsed().as_millis()
            );
            if let Ok(json) = serde_json::to_vec(&rows) {
                let _ = std::fs::write(CACHE, json);
            }
            rows
        });
    rows.iter()
        .map(|r| {
            let p = &r["position"];
            let g = |k: usize| p[k].as_f64().unwrap_or(0.0) as f32;
            TrigSrc {
                name: r["entity"].as_str().unwrap_or("").to_owned(),
                doc: r["doc"].as_str().unwrap_or("").to_owned(),
                pos_cm: origin.point(Vec3::new(g(0), g(1), g(2))),
                def: from_json(r),
            }
        })
        .collect()
}

impl WorldTriggers {
    pub fn new(src: Vec<TrigSrc>) -> Self {
        let mut grid: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for (n, s) in src.iter().enumerate() {
            grid.entry(cell(s.pos_cm)).or_default().push(n as u32);
        }
        Self {
            src,
            grid,
            ..Default::default()
        }
    }
}

pub fn register(app: &mut App) {
    app.add_systems(Update, drive_triggers);
}

/// Registers the triggers around the hero, feeds the contacts and runs the trigger clock.
fn drive_triggers(
    time: Res<Time>,
    player: Res<Player>,
    mut core: ResMut<CoreRes>,
    mut tw: ResMut<WorldTriggers>,
    state: Res<PlayState>,
) {
    let now = time.elapsed_secs();
    if now - tw.last < 0.05 || tw.src.is_empty() {
        return;
    }
    tw.last = now;
    let _ = &state;
    let hero_pos = player.ctrl.position;
    let c = &mut core.0;
    let tw = &mut *tw;
    // The hero as an entity of the core's scene model.
    let hero = match tw.hero {
        Some(h) => {
            c.range.scene_mut().move_to(h, hero_pos);
            h
        }
        None => {
            let h = c.range.scene_mut().spawn_dynamic(
                "PC_Hero",
                hero_pos,
                Vec3::new(HERO_RADIUS, HERO_HALF_HEIGHT, HERO_RADIUS),
            );
            tw.hero = Some(h);
            h
        }
    };
    // Leaving the processing range.
    let gone: Vec<u32> = tw
        .live
        .keys()
        .copied()
        .filter(|n| {
            let p = tw.src[*n as usize].pos_cm;
            (p.x - hero_pos.x).abs().max((p.z - hero_pos.z).abs()) > ROI_EXIT
        })
        .collect();
    for n in gone {
        if let Some(id) = tw.live.remove(&n) {
            c.triggers.remove(id);
            c.range.scene_mut().kill(id);
        }
        tw.touching.remove(&n);
    }
    // Entering it.
    let (cx, cz) = cell(hero_pos);
    let reach = (ROI / GRID).ceil() as i32 + 1;
    let mut add: Vec<u32> = Vec::new();
    for gx in (cx - reach)..=(cx + reach) {
        for gz in (cz - reach)..=(cz + reach) {
            for &n in tw.grid.get(&(gx, gz)).into_iter().flatten() {
                let p = tw.src[n as usize].pos_cm;
                if !tw.live.contains_key(&n)
                    && (p.x - hero_pos.x).abs().max((p.z - hero_pos.z).abs()) <= ROI
                {
                    add.push(n);
                }
            }
        }
    }
    for n in add {
        let s = &tw.src[n as usize];
        let id = c
            .range
            .scene_mut()
            .spawn_dynamic(&s.name, s.pos_cm, Vec3::splat(SHAPE_RADIUS));
        c.triggers.add(id, &format!("{}#{n}", s.doc), s.def.clone());
        tw.live.insert(n, id);
    }
    // Contacts: the hero capsule against the trigger sphere.
    let mut touch: Vec<u32> = Vec::new();
    let mut untouch: Vec<u32> = Vec::new();
    for (&n, _) in &tw.live {
        let p = tw.src[n as usize].pos_cm;
        let dxz = Vec2::new(p.x - hero_pos.x, p.z - hero_pos.z).length();
        let dy = (p.y - (hero_pos.y + HERO_HALF_HEIGHT)).abs();
        let hit = dxz < SHAPE_RADIUS + HERO_RADIUS && dy < SHAPE_RADIUS + HERO_HALF_HEIGHT;
        match (hit, tw.touching.contains(&n)) {
            (true, false) => touch.push(n),
            (false, true) => untouch.push(n),
            _ => {}
        }
    }
    let clock = c.clock;
    let contact = Contact {
        kind: ContactKind::Shape,
        ..Contact::default()
    };
    {
        let scene = c.range.scene();
        let env = TriggerEnv::new(&scene.tree, &scene.registry);
        for n in touch {
            tw.touching.insert(n);
            tw.touches += 1;
            if let Some(&id) = tw.live.get(&n) {
                c.triggers.on_touch(&env, clock, id, hero, &contact);
            }
        }
        for n in untouch {
            tw.touching.remove(&n);
            tw.untouches += 1;
            if let Some(&id) = tw.live.get(&n) {
                c.triggers.on_untouch(&env, clock, id, hero);
            }
        }
        c.triggers.process(&env, clock);
    }
    let by_id: HashMap<EntityId, u32> = tw.live.iter().map(|(n, id)| (*id, *n)).collect();
    for out in c.triggers.take_output() {
        match out {
            TriggerOutput::Script { script, owner, .. } => {
                let name = by_id
                    .get(&owner)
                    .map_or_else(String::new, |n| tw.src[*n as usize].name.clone());
                eprintln!("play: trigger {name} fired script {script}");
                tw.fired.push(Fired {
                    name,
                    script,
                    at: clock,
                });
            }
            TriggerOutput::Delivered { .. } => tw.delivered += 1,
        }
    }
}
