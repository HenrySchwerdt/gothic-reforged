//! Core events -> presentation: dialog UI, voices, toasts, hit reactions, death.
use super::anim::{CombatStance, Fallen, OverlayQueue, play_fall, queue_stumble, queue_swing};
use super::{HeroActor, Mode, NpcActor, PlayState, Roster};
use crate::walk::Player;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use gothic_animation::{ActorLibrary, Animator, Locomotion, LocomotionInput};
use gothic_audio::{MusicCommand, PlayVoice, VoiceFinished, VoiceMissing};
use gothic_gameplay::dialog::DialogSession;
use gothic_gameplay::model::QuestStatus;
use gothic_gameplay::transcript::quest_title;
use gothic_runtime::{Command, CoreCommand, CoreEvent, CoreRes, Event};
use gothic_sim::sim::EventKind;
use gothic_ui::{
    ChooseOption, DialogEnded, DialogUpdate, GameSession, Notify, PlayerHeading, PlayerVitals,
    ToastKind,
};
use std::collections::VecDeque;

pub fn register(app: &mut App) {
    app.init_resource::<Voices>()
        .init_resource::<CombatLog>()
        .add_systems(
            Update,
            (
                bind_actors,
                dialog_events,
                voice_flow,
                ui_to_core,
                combat_events,
                misc_events,
                mirror_state,
            ),
        );
}

/// Sequences the voice lines of a conversation (one at a time).
#[derive(Resource, Default)]
pub struct Voices {
    queue: VecDeque<(PlayVoice, f32)>,
    pub playing: bool,
    /// When recording (game time runs slower than the audio clock) a voice ends after this time.
    deadline: f32,
    /// Seconds since the last voice started (for scripts that wait on speech).
    pub since_start: f32,
    pub started_any: bool,
    pending_lines: Vec<(String, String, bool)>,
    session: Option<DialogSession>,
    /// Current dialog menu.
    pub menu: Vec<gothic_gameplay::dialog::Choice>,
}

impl Voices {
    pub fn idle(&self) -> bool {
        self.queue.is_empty() && !self.playing
    }
}

/// What happened in combat (read by the smoke script and the music logic).
#[derive(Resource, Default)]
pub struct CombatLog {
    pub hits_on_npcs: u32,
    pub hits_on_hero: u32,
    pub kills: u32,
    pub first_hit_at: Option<f32>,
    pub fight: bool,
}

fn bind_actors(
    mut commands: Commands,
    mut events: MessageReader<CoreEvent>,
    mut roster: ResMut<Roster>,
    gear: Query<&super::npcs::NpcGear>,
    mut cmd: MessageWriter<CoreCommand>,
) {
    for e in events.read() {
        if let Event::ActorSpawned { actor, .. } = &e.0
            && let Some((entity, cand)) = roster.binds.pop_front()
        {
            commands.entity(entity).insert(NpcActor(*actor));
            roster.by_actor.insert(*actor, entity);
            roster.cands[cand as usize].actor = Some(*actor);
            // The character's real inventory: worn weapons and everything it carries.
            if let Ok(g) = gear.get(entity) {
                for t in &g.equip {
                    cmd.write(CoreCommand(Command::EquipNpc {
                        actor: *actor,
                        template: t.clone(),
                    }));
                }
                for (t, n) in &g.give {
                    cmd.write(CoreCommand(Command::GiveNpc {
                        actor: *actor,
                        template: t.clone(),
                        amount: *n,
                    }));
                }
                commands.entity(entity).remove::<super::npcs::NpcGear>();
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn dialog_events(
    mut events: MessageReader<CoreEvent>,
    mut voices: ResMut<Voices>,
    mut ui: MessageWriter<DialogUpdate>,
    mut notify: MessageWriter<Notify>,
    mut state: ResMut<PlayState>,
    roster: Res<Roster>,
    core: Res<CoreRes>,
    mut cursor: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    for e in events.read() {
        match &e.0 {
            Event::DialogStarted { npc, session } => {
                state.mode = Mode::Dialog;
                state.dialog_npc = Some(*npc);
                state.dialog_cam = None;
                voices.session = Some(session.clone());
                voices.pending_lines.clear();
                voices.queue.clear();
                if let Ok(mut c) = cursor.single_mut() {
                    state.relock_cursor = c.grab_mode != CursorGrabMode::None;
                    c.grab_mode = CursorGrabMode::None;
                    c.visible = true;
                }
            }
            Event::LineSpoken {
                speaker,
                key,
                text,
                player,
                actor,
            } => {
                state.dialog_cut += 1;
                state.dialog_player_speaks = *player;
                let shown = text.clone().unwrap_or_else(|| format!("[{key}]"));
                let chars = shown.chars().count();
                voices.pending_lines.push((speaker.clone(), shown, *player));
                if !key.is_empty() {
                    let v = if *player {
                        PlayVoice::new(key.clone()).with_hint("player")
                    } else if let Some(entity) = actor.and_then(|a| roster.by_actor.get(&a)) {
                        PlayVoice::spatial(key.clone(), *entity)
                    } else {
                        PlayVoice::new(key.clone())
                    };
                    let est = (chars as f32 * 0.045).clamp(1.0, 4.5);
                    voices.queue.push_back((v, est));
                }
            }
            Event::MenuChanged { session, menu } => {
                let lines = std::mem::take(&mut voices.pending_lines);
                voices.menu.clone_from(menu);
                ui.write(DialogUpdate {
                    session: session.clone(),
                    lines,
                    menu: menu.clone(),
                    open: true,
                });
            }
            Event::DialogEnded { .. } => {
                if let Some(s) = voices.session.take() {
                    ui.write(DialogUpdate {
                        session: s,
                        lines: Vec::new(),
                        menu: Vec::new(),
                        open: false,
                    });
                }
                voices.pending_lines.clear();
                state.mode = Mode::Explore;
                state.dialog_npc = None;
                if state.relock_cursor
                    && let Ok(mut c) = cursor.single_mut()
                {
                    c.grab_mode = CursorGrabMode::Locked;
                    c.visible = false;
                }
            }
            Event::QuestChanged { quest, to } => {
                let verb = match to {
                    QuestStatus::Running => "Neue Aufgabe",
                    QuestStatus::Success => "Aufgabe erfüllt",
                    QuestStatus::Failed => "Aufgabe gescheitert",
                    _ => continue,
                };
                notify.write(Notify {
                    text: format!("{verb}: {}", quest_title(&core.0.db, quest)),
                    kind: ToastKind::Quest,
                });
            }
            Event::QuestLogged { quest } => {
                notify.write(Notify {
                    text: format!("Tagebuch: {}", quest_title(&core.0.db, quest)),
                    kind: ToastKind::Quest,
                });
            }
            Event::XpGained {
                amount,
                level_before,
                level_after,
            } => {
                notify.write(Notify {
                    text: format!("+{amount} EP"),
                    kind: ToastKind::Xp,
                });
                if level_after > level_before {
                    notify.write(Notify {
                        text: format!("Stufe {level_after} erreicht!"),
                        kind: ToastKind::Xp,
                    });
                }
            }
            Event::ItemMoved {
                item,
                amount,
                gained,
            } => {
                notify.write(Notify {
                    text: format!("{}{amount} {item}", if *gained { "+" } else { "-" }),
                    kind: ToastKind::Item,
                });
            }
            Event::Notice(t) => {
                notify.write(Notify {
                    text: t.clone(),
                    kind: ToastKind::Info,
                });
            }
            Event::Saved(p) => {
                // Autosaves are silent; a file path never belongs on screen.
                let auto = p
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("auto"));
                if !auto {
                    notify.write(Notify {
                        text: "Spiel gespeichert".into(),
                        kind: ToastKind::Info,
                    });
                }
            }
            Event::Failed(t) => {
                eprintln!("play: {t}");
                notify.write(Notify {
                    text: t.clone(),
                    kind: ToastKind::Info,
                });
            }
            _ => {}
        }
    }
}

fn voice_flow(
    time: Res<Time>,
    mut voices: ResMut<Voices>,
    mut play: MessageWriter<PlayVoice>,
    mut finished: MessageReader<VoiceFinished>,
    mut missing: MessageReader<VoiceMissing>,
    recorder: Option<Res<super::record::Recorder>>,
) {
    let mut ended = false;
    for _ in finished.read() {
        ended = true;
    }
    for m in missing.read() {
        eprintln!("play: no voice recording for {}", m.key);
        ended = true;
    }
    if ended || (recorder.is_some() && voices.playing && time.elapsed_secs() >= voices.deadline) {
        voices.playing = false;
    }
    voices.since_start += time.delta_secs();
    if !voices.playing
        && let Some((v, est)) = voices.queue.pop_front()
    {
        voices.deadline = time.elapsed_secs() + est;
        play.write(v);
        voices.playing = true;
        voices.started_any = true;
        voices.since_start = 0.0;
    }
}

/// UI selections and closes -> core commands.
fn ui_to_core(
    mut choose: MessageReader<ChooseOption>,
    mut ended: MessageReader<DialogEnded>,
    mut commands: MessageWriter<CoreCommand>,
) {
    for c in choose.read() {
        commands.write(CoreCommand(Command::Choose { index: c.index }));
    }
    for _ in ended.read() {
        commands.write(CoreCommand(Command::EndDialog));
    }
}

#[allow(clippy::too_many_arguments)]
fn combat_events(
    mut events: MessageReader<CoreEvent>,
    mut commands: Commands,
    roster: Res<Roster>,
    core: Res<CoreRes>,
    time: Res<Time>,
    mut log: ResMut<CombatLog>,
    mut state: ResMut<PlayState>,
    mut lib: ResMut<ActorLibrary>,
    mut music: MessageWriter<MusicCommand>,
    mut anims: Query<(&mut OverlayQueue, &mut Animator, &CombatStance)>,
    hero: Query<Entity, With<HeroActor>>,
    mut notify: MessageWriter<Notify>,
) {
    let hero_id = core.0.hero;
    let entity_of = |id: gothic_sim::types::ActorId| -> Option<Entity> {
        if id == hero_id {
            hero.single().ok()
        } else {
            roster.by_actor.get(&id).copied()
        }
    };
    for e in events.read() {
        let Event::Sim(kind) = &e.0 else { continue };
        match kind {
            EventKind::Hit {
                attacker,
                victim,
                damage,
                ignored,
                ..
            } => {
                if ignored.is_some() {
                    continue;
                }
                if *victim == hero_id {
                    log.hits_on_hero += 1;
                } else if *attacker == hero_id {
                    log.hits_on_npcs += 1;
                    log.first_hit_at.get_or_insert(time.elapsed_secs());
                }
                if *damage > 0
                    && let Some(en) = entity_of(*victim)
                    && let Ok((mut q, _, _)) = anims.get_mut(en)
                {
                    queue_stumble(&mut q);
                }
                if *attacker != hero_id
                    && let Some(en) = entity_of(*attacker)
                    && let Ok((mut q, _, _)) = anims.get_mut(en)
                {
                    queue_swing(&mut q);
                }
            }
            EventKind::Whiff { attacker, .. } => {
                if *attacker != hero_id
                    && let Some(en) = entity_of(*attacker)
                    && let Ok((mut q, _, _)) = anims.get_mut(en)
                {
                    queue_swing(&mut q);
                }
            }
            EventKind::AttackStarted { target, .. } => {
                if *target == hero_id && !log.fight {
                    log.fight = true;
                    music.write(MusicCommand::SetSituation("fight".into()));
                }
            }
            EventKind::Killed { victim, .. } | EventKind::Defeated { victim, .. } => {
                if *victim == hero_id {
                    state.hero_down = true;
                    notify.write(Notify {
                        text: "Du wurdest niedergeschlagen".into(),
                        kind: ToastKind::Info,
                    });
                    if let Some(en) = entity_of(*victim)
                        && let Ok((_, mut a, cs)) = anims.get_mut(en)
                    {
                        play_fall(&mut lib, &mut a, cs.0);
                        commands
                            .entity(en)
                            .remove::<(LocomotionInput, Locomotion)>()
                            .insert(Fallen);
                    }
                } else if matches!(kind, EventKind::Killed { .. }) {
                    log.kills += 1;
                }
            }
            _ => {}
        }
    }
}

fn misc_events(
    mut events: MessageReader<CoreEvent>,
    mut player: ResMut<Player>,
    mut state: ResMut<PlayState>,
    mut core: ResMut<CoreRes>,
    mut commands: Commands,
    hero: Query<Entity, With<HeroActor>>,
) {
    for e in events.read() {
        if let Event::Loaded { hero: pos, .. } = &e.0 {
            player.ctrl.position = Vec3::new(pos.x, pos.y + 5.0, pos.z);
            player.ctrl.vertical_speed = 0.0;
            state.last_hero_cm = None;
            state.hero_down = core.0.sim.actor(core.0.hero).hp <= 0;
            if !state.hero_down
                && let Ok(en) = hero.single()
            {
                commands.entity(en).remove::<Fallen>();
            }
        }
    }
    let _ = &mut core;
}

/// The character page reads `attributes` of the state; the live values (base plus equipment and
/// effects) are the hero actor's, so they are written into the UI's copy.
fn mirror_attributes(core: &CoreRes, state: &mut gothic_gameplay::GameState) {
    use gothic_sim::types::DamageType as D;
    let hero = core.0.sim.actor(core.0.hero);
    let cfg = &core.0.sim.cfg;
    let a = &mut state.attributes;
    a.insert("str".into(), hero.strength(cfg));
    a.insert("hnt".into(), hero.dexterity(cfg));
    a.insert("int".into(), hero.intelligence(cfg));
    a.insert("smt".into(), hero.base.smithing);
    a.insert("thf".into(), hero.theft(cfg));
    a.insert("alc".into(), hero.base.alchemy);
    for (k, t) in [
        ("prot_blade", D::Blade),
        ("prot_impact", D::Impact),
        ("prot_missile", D::Missile),
        ("prot_fire", D::Fire),
        ("prot_ice", D::Ice),
        ("prot_lightning", D::Lightning),
    ] {
        a.insert(k.into(), hero.protection(cfg, t));
    }
}

/// Mirror core state into the UI's read model and the HUD.
fn mirror_state(
    core: Res<CoreRes>,
    mut session: ResMut<GameSession>,
    mut vitals: ResMut<PlayerVitals>,
    mut heading: ResMut<PlayerHeading>,
    mut slots: ResMut<gothic_ui::QuickSlots>,
    mut focus_health: ResMut<gothic_ui::FocusHealth>,
    play: Res<super::PlayState>,
    player: Res<Player>,
    mut rev: Local<u64>,
) {
    if *rev != core.0.state_rev {
        *rev = core.0.state_rev;
        session.state = core.0.state().clone();
        slots.0 = quick_slots(&play.quick, &session);
        mirror_attributes(&core, &mut session.state);
    }
    // Equipment and effects change the attributes without touching the persistent state.
    if core.0.sim.tick % 30 == 0 {
        let mut fresh = session.state.clone();
        mirror_attributes(&core, &mut fresh);
        if fresh.attributes != session.state.attributes {
            session.state.attributes = fresh.attributes;
        }
    }
    // creature in the fight (or in front of the camera): its health shows in the top bar
    let target = play.combat_focus.map(|(id, _)| id).or(play.focus);
    let fh = target.and_then(|id| {
        let a = core.0.sim.actor(id);
        (id != core.0.hero && a.hp > 0)
            .then(|| a.hp as f32 / a.hp_max(&core.0.sim.cfg).max(1) as f32)
    });
    if focus_health.0 != fh {
        focus_health.0 = fh;
    }
    let hero = core.0.sim.actor(core.0.hero);
    let cfg = &core.0.sim.cfg;
    // Write only on change: a flagged resource re-runs the UI layout every frame.
    let v = gothic_ui::PlayerVitals {
        hp: hero.hp.max(0) as f32,
        hp_max: hero.hp_max(cfg) as f32,
        sp: hero.stamina as f32,
        sp_max: hero.stamina_max(cfg) as f32,
        mp: hero.mana as f32,
        mp_max: hero.mana_max() as f32,
    };
    if vitals.hp != v.hp
        || vitals.hp_max != v.hp_max
        || vitals.sp != v.sp
        || vitals.sp_max != v.sp_max
        || vitals.mp != v.mp
        || vitals.mp_max != v.mp_max
    {
        *vitals = v;
    }
    if heading.0 != -player.yaw {
        heading.0 = -player.yaw;
    }
}

/// The quick bar: the bound template of each slot with the amount carried (slots 1..0). The
/// start kit binds slot 1 the sword, 2 the torch, 3 the bow, 4 the arrows and 5 the health
/// potions (`InventoryAssureStartupItems`).
fn quick_slots(bound: &[Option<String>], s: &GameSession) -> Vec<Option<gothic_ui::QuickSlot>> {
    bound
        .iter()
        .map(|b| {
            b.as_ref().map(|item| gothic_ui::QuickSlot {
                item: item.clone(),
                count: s.state.item_count(item),
            })
        })
        .collect()
}
