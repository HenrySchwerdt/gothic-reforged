//! Optional local clip pack for `--combat souls`.
//!
//! When a clip pack generated on the user's machine exists (default `.local/er/er_clips.grcp`,
//! or `--er-clips FILE` / `GR_CLIP_PACK`), the Souls flow plays its roll, backstep, attack,
//! guard, lock-on strafe and reaction clips instead of the stock Gothic motions, and takes the
//! action windows (input, cancel, i-frame, hit, travel) from the metadata stored with the
//! clips. Without the file nothing here does anything and the Gothic motions stay in charge;
//! `--no-er-clips` switches it off explicitly. The pack is derived data and is never part of
//! the repository (see `gothic_animation::clip_pack`).
//!
//! The pack clips are full-body overlays whose time follows the action's frame counter, so the
//! pose always matches the state machine: hit-stop, charge holds and cancels need no extra
//! bookkeeping.
use super::souls::SoulsGate;
use super::{PlayState, anim::CombatStance};
use crate::walk::Player;
use bevy::prelude::*;
use gothic_animation::{ActorLibrary, Animator, Overlay, pack_overlay as ov};
use gothic_combat_souls::{
    animmap::{self, PackPose},
    plugin::SoulsRes,
};
use std::path::{Path, PathBuf};

/// Name prefix of the pack's clips in the library.
pub use gothic_combat_souls::pack::{PREFIX, style};
const DEFAULT_PACK: &str = ".local/er/er_clips.grcp";
/// Seconds a pack overlay takes to fade out when its action ends.
const FADE_OUT: f32 = 0.14;

/// Whether an overlay was started from the pack.
pub fn is_pack(o: &Overlay) -> bool {
    ov::is_pack(o, PREFIX)
}

#[derive(Resource)]
struct PackState {
    /// Lock-on strafe clips exist.
    strafe: bool,
}

/// Loads the pack if present and wires it into the Souls flow. Called from `souls::register`
/// once the library and the Souls resources exist.
pub fn install(app: &mut App) {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--no-er-clips") {
        return;
    }
    let explicit = args
        .iter()
        .position(|a| a == "--er-clips")
        .and_then(|i| args.get(i + 1))
        .filter(|s| !s.starts_with("--"))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("GR_CLIP_PACK").map(PathBuf::from));
    let asked = explicit.is_some() || args.iter().any(|a| a == "--er-clips");
    let path = explicit.unwrap_or_else(|| PathBuf::from(DEFAULT_PACK));
    if !path.exists() {
        if asked {
            eprintln!(
                "play: clip pack {} not found, using Gothic motions",
                path.display()
            );
        }
        return;
    }
    let pack = match app
        .world_mut()
        .resource_mut::<ActorLibrary>()
        .load_clip_pack(Path::new(&path))
    {
        Ok(p) => p,
        Err(e) => {
            eprintln!("play: clip pack unusable ({e:#}), using Gothic motions");
            return;
        }
    };
    let installed = {
        let mut res = app.world_mut().resource_mut::<SoulsRes>();
        gothic_combat_souls::pack::install(&mut res.anim, &pack)
    };
    let strafe = installed.strafe;
    eprintln!(
        "play: clip pack {} ({} clips): {} actions remapped, {} with pack timing, strafe {}",
        path.display(),
        pack.clips.len(),
        installed.remapped,
        installed.timed,
        strafe
    );
    app.insert_resource(PackState { strafe })
        .add_systems(Update, sync);
}

#[allow(clippy::too_many_arguments)]
fn sync(
    time: Res<Time>,
    gate: Res<SoulsGate>,
    state: Res<PlayState>,
    res: Res<SoulsRes>,
    player: Res<Player>,
    pack: Res<PackState>,
    mut lib: ResMut<ActorLibrary>,
    mut anims: Query<(&mut Animator, &CombatStance)>,
) {
    let Some(hero) = state.hero_entity else {
        return;
    };
    let Ok((mut animator, stance)) = anims.get_mut(hero) else {
        return;
    };
    let dt = time.delta_secs().min(0.1);
    let Some(style) = style(stance.0).filter(|_| gate.active) else {
        ov::fade_out(&mut animator, PREFIX, dt, FADE_OUT);
        return;
    };
    let pose = animmap::pack_pose(
        &res.world.hero,
        &res.world.tuning,
        pack.strafe && res.world.lock.target.is_some(),
        player.ctrl.velocity,
    );
    match pose {
        // The action's overlay (started with the action) follows its frame counter.
        PackPose::Action { frame, .. } => {
            ov::follow_time(
                &mut animator,
                PREFIX,
                frame / gothic_combat_souls::tuning::ANIM_FPS,
            );
            return;
        }
        PackPose::Guard => {
            let name = format!("{PREFIX}{style}_Guard");
            if lib.has_motion(&name) && ov::hold_loop(&mut lib, &mut animator, PREFIX, &name, 1.0) {
                return;
            }
        }
        PackPose::Strafe { gait, dir, rate } => {
            let name = format!("{PREFIX}{gait}_{dir}");
            if lib.has_motion(&name) && ov::hold_loop(&mut lib, &mut animator, PREFIX, &name, rate)
            {
                return;
            }
        }
        PackPose::None => {}
    }
    ov::fade_out(&mut animator, PREFIX, dt, FADE_OUT);
}
