//! NPC routines on screen: the ported routine states (`ZS_Smith`, `ZS_Guard`, `ZS_Sleep`, ...)
//! queue `Action` / `AniStateAction` commands; the simulation reports them as `Event::Anim` and
//! the character plays the clip of that action (`pose.rs`) at its work point.
use super::npcs::NpcMotion;
use super::pose::{self, Posed};
use super::{NpcActor, Roster};
use bevy::prelude::*;
use gothic_animation::{ActorLibrary, Animator, Stance};
use gothic_runtime::{CoreEvent, CoreRes, Event};
use gothic_sim::script::host::AiCommand;

/// What the routines showed (smoke report).
#[derive(Resource, Default)]
pub struct RoutineLog {
    /// `(character, action)` of every pose that was started.
    pub poses: Vec<(String, String)>,
    /// Commands for which the motion catalog has no clip.
    pub unavailable: Vec<(String, String)>,
    /// How often each kind of command arrived (`Action`, `StandUp`, ...).
    pub kinds: std::collections::BTreeMap<String, u32>,
}

pub fn register(app: &mut App) {
    app.init_resource::<RoutineLog>()
        .add_systems(Update, (npc_anims, release_when_moving).chain());
}

fn npc_anims(
    mut events: MessageReader<CoreEvent>,
    roster: Res<Roster>,
    core: Res<CoreRes>,
    lib: Res<ActorLibrary>,
    anims: Query<&Animator, With<NpcActor>>,
    mut posed: Query<&mut Posed>,
    mut commands: Commands,
    mut log: ResMut<RoutineLog>,
) {
    for e in events.read() {
        let Event::Anim {
            actor,
            command,
            use_type,
        } = &e.0
        else {
            continue;
        };
        let kind = format!("{command:?}");
        let kind = kind.split([' ', '(', '{']).next().unwrap_or("").to_string();
        *log.kinds.entry(kind).or_default() += 1;
        let Some(entity) = roster.by_actor.get(actor).copied() else {
            continue;
        };
        let Ok(animator) = anims.get(entity) else {
            continue;
        };
        let (action, secs): (String, Option<f32>) = match command {
            AiCommand::Action { action, secs } => (action.name().to_string(), Some(*secs as f32)),
            AiCommand::ActionAtPoint { secs } => (use_type.name().to_string(), Some(*secs as f32)),
            AiCommand::AniStateAction(a) => {
                // Already sitting / lying in that pose: nothing to play again.
                if posed
                    .get(entity)
                    .is_ok_and(|p| !p.is_releasing() && p.action == a.name())
                {
                    continue;
                }
                (a.name().to_string(), None)
            }
            AiCommand::StandUp => {
                if let Ok(mut p) = posed.get_mut(entity) {
                    p.release();
                }
                continue;
            }
            _ => continue,
        };
        let who = core.0.sim.actor(*actor).name.clone();
        let family = animator.rig.family.clone();
        // An action inside a seat or bed pose the character holds plays that pose's clip.
        let in_pose = posed
            .get(entity)
            .ok()
            .filter(|p| !p.is_releasing() && pose::POSE_ACTIONS.contains(&p.action.as_str()))
            .map(|p| p.action.clone());
        match pose::build(
            &lib,
            &family,
            Stance::Unarmed,
            &action,
            secs,
            in_pose.as_deref(),
        ) {
            Some(p) => {
                commands.entity(entity).insert(p);
                log.poses.push((who, action));
            }
            None => log.unavailable.push((who, action)),
        }
    }
}

/// A character that walks off ends its pose.
fn release_when_moving(mut q: Query<(&NpcMotion, &mut Posed), With<NpcActor>>) {
    for (m, mut p) in &mut q {
        if m.velocity.length() > 60.0 && !p.is_releasing() {
            p.release();
        }
    }
}
