//! Teleporter stones / fast travel as the original does it.
//!
//! A teleporter stone is an artefact item (`It_Teleport_<City>`, `UseType = Cast`) whose spell
//! `Spl_Teleport_<City>` ends in the script function `MagicTeleport` (`magic_teleport`, ported in
//! `gothic-script`): the caster's party is moved to the world entity named `Teleport_<City>`.
//! Those 31 destination entities live in the document `G3_World_Teleport_Points_01` (a
//! `gCAIHelper_FreePoint_PS` each). The core registers them as waypoints (for the party); the
//! hero's body is placed by the host, which owns the character controller:
//! [`MagicEvent::Teleport`] -> controller position + heading, the streaming follows the hero.
use crate::walk::{ORIGIN, Player};
use crate::world_loader::Origin;
use bevy::prelude::*;
use gothic_runtime::{CoreEvent, CoreRes, Event};
use gothic_sim::script::magic::api::MagicEvent;
use gothic_sim::world::Pos;
use std::collections::HashMap;

const CACHE: &str = ".local/teleports.json";
const EXPORT: &str = ".local/objects_world.json";

/// Destination name (lower case) -> original-space position (cm) and heading.
#[derive(Resource, Default, Clone)]
pub struct Destinations {
    pub map: HashMap<String, (Vec3, f32)>,
}

impl Destinations {
    /// From the small cache, built once from the world object export.
    pub fn load() -> Self {
        let rows = std::fs::read(CACHE)
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .or_else(|| {
                let root: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(EXPORT).ok()?).ok()?;
                let rows: Vec<serde_json::Value> = root["objects"]
                    .as_array()?
                    .iter()
                    .filter(|o| {
                        o["name"]
                            .as_str()
                            .is_some_and(|n| n.starts_with("Teleport_"))
                    })
                    .map(|o| {
                        serde_json::json!({
                            "name": o["name"],
                            "position": o["position"],
                            "forward": [o["matrix"][2][0], o["matrix"][2][2]],
                        })
                    })
                    .collect();
                let v = serde_json::Value::Array(rows);
                let _ = std::fs::write(CACHE, serde_json::to_vec(&v).ok()?);
                Some(v)
            });
        let mut map = HashMap::new();
        for r in rows
            .as_ref()
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
        {
            let (Some(name), Some(p)) = (r["name"].as_str(), r["position"].as_array()) else {
                continue;
            };
            let f = |i: usize| p.get(i).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            let heading = (r["forward"][0].as_f64().unwrap_or(0.0))
                .atan2(r["forward"][1].as_f64().unwrap_or(1.0)) as f32;
            map.insert(
                name.to_ascii_lowercase(),
                (Vec3::new(f(0), f(1), f(2)), heading),
            );
        }
        eprintln!("play: {} teleport destinations", map.len());
        Self { map }
    }

    /// Mirrored, origin-relative centimetres of a destination (the space the world is built in).
    pub fn point_cm(&self, name: &str) -> Option<Vec3> {
        self.map
            .get(&name.to_ascii_lowercase())
            .map(|(p, _)| Origin(ORIGIN).point(*p))
    }
}

/// Gives the core the destinations (`MagicTeleport` moves the caster's party to them).
pub fn register_waypoints(core: &mut gothic_runtime::Core, dest: &Destinations) {
    for (key, (p, _)) in &dest.map {
        // The script names keep their case (`Teleport_KapDun`).
        let name = format!("Teleport_{}", &key["teleport_".len()..]);
        let at = Origin(ORIGIN).point(*p);
        core.sim
            .script
            .add_waypoint(&name, Pos::new(at.x, at.y, at.z));
    }
}

pub fn register(app: &mut App) {
    app.add_systems(Update, teleport_hero);
}

/// A hero teleport of the magic system moves the character controller.
fn teleport_hero(
    mut events: MessageReader<CoreEvent>,
    core: Res<CoreRes>,
    dest: Res<Destinations>,
    mut player: ResMut<Player>,
    mut state: ResMut<super::PlayState>,
) {
    for e in events.read() {
        let Event::Magic(MagicEvent::Teleport { who, to }) = &e.0 else {
            continue;
        };
        if who.0.saturating_sub(1) != core.0.hero.0 {
            continue;
        }
        let Some(&(p, heading)) = dest.map.get(&to.to_ascii_lowercase()) else {
            eprintln!("play: teleport destination {to} is not in the world");
            continue;
        };
        let at = Origin(ORIGIN).point(p);
        eprintln!(
            "play: teleport to {to}: {:.0} {:.0} {:.0}",
            at.x, at.y, at.z
        );
        // A little above the free point; the controller waits for the tile's collision before
        // it integrates gravity (`player_motion`), then settles on the ground.
        player.ctrl.position = at + Vec3::Y * 30.0;
        player.ctrl.vertical_speed = 0.0;
        player.placed = true;
        // The free point's forward axis in the mirrored space.
        let (fx, fz) = (-heading.sin(), heading.cos());
        player.yaw = super::yaw_of_dir(fx, fz);
        state.last_hero_cm = None;
    }
}
