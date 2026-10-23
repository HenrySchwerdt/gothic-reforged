//! Coarse on-device frame profile: wall time of the fixed-step loop (physics, VR hands, melee, ragdolls), the
//! variable-step game systems and PostUpdate (animation, transform propagation), logged every 2 s.
use bevy::app::{FixedFirst, FixedLast};
use bevy::prelude::*;
use std::time::{Duration, Instant};

#[derive(Resource, Default)]
struct Timing {
    mark: Option<Instant>,
    fixed_start: Option<Instant>,
    fixed: Duration,
    fixed_steps: u32,
    update: Duration,
    post: Duration,
    frame: Duration,
    frame_start: Option<Instant>,
    frames: u32,
    last_log: Option<Instant>,
}

pub struct FrameTimingPlugin;

impl Plugin for FrameTimingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Timing>()
            .add_systems(First, frame_begin)
            .add_systems(FixedFirst, |mut t: ResMut<Timing>| t.fixed_start = Some(Instant::now()))
            .add_systems(FixedLast, fixed_end)
            .add_systems(PreUpdate, mark)
            .add_systems(Update, update_end.after(mark).ambiguous_with_all())
            .add_systems(PostUpdate, post_start.before(bevy::transform::TransformSystems::Propagate))
            .add_systems(Last, frame_end);
    }
}

fn frame_begin(mut t: ResMut<Timing>) {
    t.frame_start = Some(Instant::now());
}
fn fixed_end(mut t: ResMut<Timing>) {
    if let Some(s) = t.fixed_start.take() {
        t.fixed += s.elapsed();
        t.fixed_steps += 1;
    }
}
fn mark(mut t: ResMut<Timing>) {
    t.mark = Some(Instant::now());
}
fn update_end(mut t: ResMut<Timing>) {
    if let Some(s) = t.mark.take() {
        t.update += s.elapsed();
    }
}
fn post_start(mut t: ResMut<Timing>) {
    t.mark = Some(Instant::now());
}
fn frame_end(mut t: ResMut<Timing>) {
    if let Some(s) = t.mark.take() {
        t.post += s.elapsed();
    }
    if let Some(s) = t.frame_start.take() {
        t.frame += s.elapsed();
        t.frames += 1;
    }
    let now = Instant::now();
    let last = *t.last_log.get_or_insert(now);
    if now - last >= Duration::from_secs(2) && t.frames > 0 {
        let n = t.frames as f32;
        let ms = |d: Duration| d.as_secs_f32() * 1000.0 / n;
        log::info!(
            "frame timing: main-world {:.1} ms/frame | fixed {:.1} ms ({:.1} steps) | update {:.1} ms | post {:.1} ms",
            ms(t.frame),
            ms(t.fixed),
            t.fixed_steps as f32 / n,
            ms(t.update),
            ms(t.post),
        );
        *t = Timing {
            last_log: Some(now),
            ..default()
        };
    }
}
