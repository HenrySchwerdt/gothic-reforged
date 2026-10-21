//! Level geometry for both stacks: one chunk of triangles (centimetres, as streamed for the
//! game's `CollisionWorld`) becomes a static avian trimesh collider in metres, so physics
//! hands, props and arrows collide with exactly the walls the character controller sees.
use crate::units::tri_to_m;
use avian3d::prelude::*;
use bevy::prelude::*;
use gothic_physics::{CollisionWorld, Tri};
use gothic_vr_physics::VrLayer;
use std::collections::HashMap;

/// Static collider entity of a level chunk.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelChunk(pub u64);

/// Host -> bridge: a chunk was streamed in (triangles in centimetres) or out.
#[derive(Message, Debug, Clone)]
pub enum LevelChunkMessage {
    Loaded { id: u64, tris: Vec<Tri> },
    Unloaded { id: u64 },
}

/// Avian trimesh of `tris` (centimetres), `None` when empty or degenerate.
pub fn trimesh_from_cm(tris: &[Tri]) -> Option<Collider> {
    let mut vertices = Vec::with_capacity(tris.len() * 3);
    let mut indices = Vec::with_capacity(tris.len());
    for t in tris {
        let [a, b, c] = tri_to_m(t);
        if !(a.is_finite() && b.is_finite() && c.is_finite())
            || (b - a).cross(c - a).length() < 1e-10
        {
            continue;
        }
        let i = vertices.len() as u32;
        vertices.extend([a, b, c]);
        indices.push([i, i + 1, i + 2]);
    }
    (!indices.is_empty()).then(|| Collider::trimesh(vertices, indices))
}

/// Physics layers of level geometry for the VR layer.
pub fn level_layers() -> CollisionLayers {
    CollisionLayers::new(
        VrLayer::World,
        [VrLayer::Hand, VrLayer::Prop, VrLayer::Body],
    )
}

/// Put a chunk into the game's collision world and return the bundle for its avian twin;
/// keeps both stacks fed from the same triangles.
pub fn load_both(world: &mut CollisionWorld, id: u64, tris: Vec<Tri>) -> Option<impl Bundle> {
    let collider = trimesh_from_cm(&tris);
    world.insert_chunk(id, tris);
    collider.map(|c| {
        (
            LevelChunk(id),
            RigidBody::Static,
            c,
            level_layers(),
            Transform::IDENTITY,
        )
    })
}

#[derive(Resource, Default)]
struct Chunks(HashMap<u64, Entity>);

pub(crate) struct LevelPlugin;

impl Plugin for LevelPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<LevelChunkMessage>()
            .init_resource::<Chunks>()
            .add_systems(Update, stream_chunks);
    }
}

fn stream_chunks(
    mut commands: Commands,
    mut chunks: ResMut<Chunks>,
    mut msgs: MessageReader<LevelChunkMessage>,
) {
    for m in msgs.read() {
        match m {
            LevelChunkMessage::Loaded { id, tris } => {
                if let Some(old) = chunks.0.remove(id) {
                    commands.entity(old).despawn();
                }
                if let Some(c) = trimesh_from_cm(tris) {
                    let e = commands
                        .spawn((
                            LevelChunk(*id),
                            RigidBody::Static,
                            c,
                            level_layers(),
                            Transform::IDENTITY,
                            Name::new(format!("LevelChunk{id}")),
                        ))
                        .id();
                    chunks.0.insert(*id, e);
                }
            }
            LevelChunkMessage::Unloaded { id } => {
                if let Some(e) = chunks.0.remove(id) {
                    commands.entity(e).despawn();
                }
            }
        }
    }
}
