//! `--combat souls` HUD: lock state (target name, line-of-sight grace, camera lift), the
//! telegraph / commit cue of the most urgent enemy swing and the last impact feedback, plus a
//! marker above the locked target. A presentation layer only: it reads `SoulsEvent` messages,
//! `SoulsRes` and [`LockView`]; it is hidden whenever the Souls flow is not driving the hero.
use super::Roster;
use super::souls::SoulsGate;
use super::souls_wire::LockView;
use crate::walk::PlayerCamera;
use bevy::prelude::*;
use gothic_combat_souls::events::{Event, FeedbackCause};
use gothic_combat_souls::plugin::{SoulsEvent, SoulsRes};
use gothic_combat_souls::tuning::DT;
use gothic_runtime::CoreRes;
use gothic_sim::types::ActorId;
use gothic_ui::UiAssets;
use std::collections::BTreeMap;

/// Seconds the commit cue flashes.
pub const COMMIT_FLASH: f32 = 0.35;
/// Seconds an impact word stays up.
pub const FEEDBACK_SHOW: f32 = 0.8;
/// A telegraph is dropped this long after its swing was due (the sim sends no strike event).
const TELEGRAPH_LINGER: f32 = 0.15;
/// Marker height above the target's feet (cm).
const MARKER_HEIGHT: f32 = 210.0;
const MARKER_W: f32 = 40.0;

const WHITE: Color = Color::srgb(0.95, 0.95, 0.95);
const AMBER: Color = Color::srgb(1.0, 0.62, 0.15);
const DIM: Color = Color::srgb(0.85, 0.82, 0.72);

/// One enemy swing on its way.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tele {
    /// Seconds until the swing is live.
    pub left: f32,
    pub committed: bool,
    /// Seconds the commit flash still shows.
    pub flash: f32,
}

/// Event counters (QA scenarios read them).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HudCounts {
    pub telegraphs: u32,
    pub commits: u32,
    pub feedbacks: u32,
    pub lock_changes: u32,
}

/// What the HUD shows, kept apart from the Bevy UI so it is testable.
#[derive(Resource, Default, Debug)]
pub struct SoulsHudModel {
    pub tele: BTreeMap<u32, Tele>,
    /// Last impact and the seconds it still shows.
    pub feedback: Option<(FeedbackCause, f32)>,
    pub counts: HudCounts,
    /// The lock and threat lines as drawn last frame (empty when hidden).
    pub shown_lock: String,
    pub shown_threat: String,
}

/// What the threat line says.
#[derive(Clone, Debug, PartialEq)]
pub struct Threat {
    pub foe: u32,
    pub text: String,
    /// The swing direction is fixed (the moment to roll).
    pub committed: bool,
    pub flashing: bool,
}

impl SoulsHudModel {
    pub fn on_event(&mut self, e: &Event) {
        match *e {
            Event::FoeTelegraph { foe, lead, .. } => {
                self.counts.telegraphs += 1;
                self.tele.insert(
                    foe,
                    Tele {
                        left: lead,
                        committed: false,
                        flash: 0.0,
                    },
                );
            }
            Event::FoeCommit { foe, .. } => {
                self.counts.commits += 1;
                if let Some(t) = self.tele.get_mut(&foe) {
                    t.committed = true;
                    t.flash = COMMIT_FLASH;
                }
            }
            Event::FoeStrike { foe, .. }
            | Event::FoeStaggered { foe, .. }
            | Event::FoeDied { foe } => {
                self.tele.remove(&foe);
            }
            Event::Feedback { cause, .. } => {
                self.counts.feedbacks += 1;
                self.feedback = Some((cause, FEEDBACK_SHOW));
            }
            Event::LockChanged { .. } => self.counts.lock_changes += 1,
            _ => {}
        }
    }

    pub fn tick(&mut self, dt: f32) {
        for t in self.tele.values_mut() {
            t.left -= dt;
            t.flash = (t.flash - dt).max(0.0);
        }
        self.tele.retain(|_, t| t.left > -TELEGRAPH_LINGER);
        if let Some((_, left)) = &mut self.feedback {
            *left -= dt;
        }
        self.feedback = self.feedback.filter(|f| f.1 > 0.0);
    }

    pub fn clear(&mut self) {
        self.tele.clear();
        self.feedback = None;
    }

    /// The most urgent swing (soonest to land).
    pub fn threat(&self) -> Option<Threat> {
        let (&foe, t) = self
            .tele
            .iter()
            .min_by(|a, b| a.1.left.total_cmp(&b.1.left))?;
        let secs = t.left.max(0.0);
        let text = if t.committed && t.flash > 0.0 {
            "COMMIT - ROLL".to_string()
        } else if t.committed {
            format!("COMMITTED  {secs:.1} s")
        } else {
            format!("WIND-UP  {secs:.1} s")
        };
        Some(Threat {
            foe,
            text,
            committed: t.committed,
            flashing: t.flash > 0.0,
        })
    }

    pub fn feedback_text(&self) -> Option<&'static str> {
        self.feedback.map(|(c, _)| match c {
            FeedbackCause::Parry => "PARRY",
            FeedbackCause::Block => "BLOCK",
            FeedbackCause::GuardBreak => "GUARD BREAK",
            FeedbackCause::HurtHeavy => "HEAVY HIT",
            FeedbackCause::Stagger => "STAGGER",
            FeedbackCause::Riposte => "RIPOSTE",
        })
    }
}

/// The lock line: target name, how much of the line-of-sight grace is used, camera lift.
pub fn lock_line(name: Option<&str>, unseen_share: f32, occluded: bool) -> String {
    let Some(name) = name else {
        return String::new();
    };
    let mut s = format!("LOCK  {name}");
    if unseen_share > 0.0 {
        let pct = (unseen_share.clamp(0.0, 1.0) * 100.0).round();
        s.push_str(&format!("  |  NO SIGHT {pct:.0}%"));
    }
    if occluded {
        s.push_str("  |  CAMERA LIFT");
    }
    s
}

#[derive(Component)]
struct HudPanel;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum HudLine {
    Lock,
    Threat,
    Feedback,
}

#[derive(Component)]
struct LockMarker;

pub fn register(app: &mut App) {
    app.init_resource::<SoulsHudModel>()
        .add_systems(Startup, spawn)
        .add_systems(Update, (model, draw).chain());
}

fn spawn(mut commands: Commands, assets: Option<Res<UiAssets>>) {
    let font: Handle<Font> = assets.map(|a| a.font.clone()).unwrap_or_default();
    let tf = |size: f32| TextFont {
        font: font.clone().into(),
        font_size: FontSize::Px(size),
        ..default()
    };
    commands
        .spawn((
            HudPanel,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                bottom: percent(16),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(2.0),
                ..default()
            },
            Visibility::Hidden,
        ))
        .with_children(|p| {
            for (line, size, color) in [
                (HudLine::Threat, 26.0, AMBER),
                (HudLine::Feedback, 20.0, WHITE),
                (HudLine::Lock, 18.0, DIM),
            ] {
                p.spawn((
                    line,
                    Text::new(""),
                    tf(size),
                    TextColor(color),
                    TextShadow::default(),
                ));
            }
        });
    commands.spawn((
        LockMarker,
        Node {
            position_type: PositionType::Absolute,
            width: px(MARKER_W),
            justify_content: JustifyContent::Center,
            ..default()
        },
        Text::new("V"),
        tf(22.0),
        TextColor(WHITE),
        TextShadow::default(),
        Visibility::Hidden,
    ));
}

fn model(
    time: Res<Time>,
    gate: Res<SoulsGate>,
    mut hud: ResMut<SoulsHudModel>,
    mut events: MessageReader<SoulsEvent>,
) {
    for SoulsEvent(e) in events.read() {
        hud.on_event(e);
    }
    if gate.active {
        hud.tick(time.delta_secs().min(0.1));
    } else {
        hud.clear();
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn draw(
    gate: Res<SoulsGate>,
    mut hud: ResMut<SoulsHudModel>,
    res: Res<SoulsRes>,
    view: Res<LockView>,
    core: Res<CoreRes>,
    roster: Res<Roster>,
    scale: Res<UiScale>,
    cam: Query<(&Camera, &GlobalTransform), With<PlayerCamera>>,
    mut panel: Query<&mut Visibility, (With<HudPanel>, Without<LockMarker>)>,
    mut lines: Query<(&HudLine, &mut Text, &mut TextColor), Without<LockMarker>>,
    mut marker: Query<
        (&mut Node, &mut Visibility, &mut TextColor),
        (With<LockMarker>, Without<HudPanel>, Without<HudLine>),
    >,
) {
    let show = |v: &mut Visibility, on: bool| {
        let want = if on {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *v != want {
            *v = want;
        }
    };
    if let Ok(mut v) = panel.single_mut() {
        show(&mut v, gate.active);
    }
    let Ok((mut node, mut mvis, mut mcolor)) = marker.single_mut() else {
        return;
    };
    if !gate.active {
        if !hud.shown_lock.is_empty() || !hud.shown_threat.is_empty() {
            hud.shown_lock.clear();
            hud.shown_threat.clear();
        }
        return show(&mut mvis, false);
    }
    let target = res.world.lock.target;
    let name = target.map(|id| {
        let actor = ActorId(id - 1);
        if (actor.0 as usize) < core.0.sim.actors.len() {
            let n = &core.0.sim.actor(actor).name;
            roster.display.get(n).unwrap_or(n).clone()
        } else {
            format!("#{id}")
        }
    });
    let t = &res.world.tuning;
    let unseen = if t.lock_los_grace > 0.0 {
        f32::from(res.world.lock.unseen_ticks) * DT / t.lock_los_grace
    } else {
        0.0
    };
    let threat = hud.threat();
    for (line, mut text, mut color) in &mut lines {
        let (s, c) = match line {
            HudLine::Lock => (lock_line(name.as_deref(), unseen, view.occluded), DIM),
            HudLine::Threat => match &threat {
                Some(th) => (th.text.clone(), if th.flashing { WHITE } else { AMBER }),
                None => (String::new(), AMBER),
            },
            HudLine::Feedback => (hud.feedback_text().unwrap_or("").to_string(), WHITE),
        };
        match line {
            HudLine::Lock if hud.shown_lock != s => hud.shown_lock.clone_from(&s),
            HudLine::Threat if hud.shown_threat != s => hud.shown_threat.clone_from(&s),
            _ => {}
        }
        if text.0 != s {
            text.0 = s;
        }
        if color.0 != c {
            color.0 = c;
        }
    }

    // ---- marker above the locked target ------------------------------------------------------
    let at = res.world.lock_pos();
    let (Some(at), Ok((camera, gt))) = (at, cam.single()) else {
        return show(&mut mvis, false);
    };
    let Ok(p) = camera.world_to_viewport(gt, (at + Vec3::Y * MARKER_HEIGHT) * 0.01) else {
        return show(&mut mvis, false);
    };
    let s = scale.0.max(0.01);
    node.left = px(p.x / s - MARKER_W * 0.5);
    node.top = px(p.y / s - 24.0);
    let c = match threat.filter(|th| Some(th.foe) == target) {
        Some(th) if th.flashing || th.committed => WHITE,
        Some(_) => AMBER,
        None => DIM,
    };
    if mcolor.0 != c {
        mcolor.0 = c;
    }
    show(&mut mvis, true);
}

#[cfg(test)]
mod tests {
    use super::*;
    use gothic_combat_souls::tuning::Shake;

    fn telegraph(foe: u32, lead: f32) -> Event {
        Event::FoeTelegraph {
            foe,
            attack: 0,
            lead,
            track: lead * 0.55,
            parryable: true,
        }
    }

    #[test]
    fn telegraph_counts_down_then_commits_and_expires() {
        let mut m = SoulsHudModel::default();
        m.on_event(&telegraph(5, 0.6));
        let th = m.threat().unwrap();
        assert_eq!((th.foe, th.committed), (5, false));
        assert_eq!(th.text, "WIND-UP  0.6 s");
        m.tick(0.3);
        m.on_event(&Event::FoeCommit { foe: 5, attack: 0 });
        let th = m.threat().unwrap();
        assert!(th.committed && th.flashing);
        assert_eq!(th.text, "COMMIT - ROLL");
        m.tick(COMMIT_FLASH + 0.01);
        let th = m.threat().unwrap();
        assert!(th.committed && !th.flashing);
        assert!(th.text.starts_with("COMMITTED"), "{}", th.text);
        m.tick(1.0);
        assert!(m.threat().is_none());
        assert_eq!(
            m.counts,
            HudCounts {
                telegraphs: 1,
                commits: 1,
                ..Default::default()
            }
        );
    }

    #[test]
    fn the_soonest_swing_is_the_threat_and_strikes_clear_it() {
        let mut m = SoulsHudModel::default();
        m.on_event(&telegraph(1, 1.0));
        m.on_event(&telegraph(2, 0.4));
        assert_eq!(m.threat().unwrap().foe, 2);
        m.on_event(&Event::FoeStrike { foe: 2, attack: 0 });
        assert_eq!(m.threat().unwrap().foe, 1);
        m.on_event(&Event::FoeDied { foe: 1 });
        assert!(m.threat().is_none());
        // A commit for a foe without a telegraph is counted but shows nothing.
        m.on_event(&Event::FoeCommit { foe: 9, attack: 0 });
        assert!(m.threat().is_none());
    }

    #[test]
    fn feedback_shows_for_a_moment() {
        let mut m = SoulsHudModel::default();
        m.on_event(&Event::Feedback {
            cause: FeedbackCause::GuardBreak,
            hit_stop: 0.15,
            shake: Shake::new(9.0, 0.4, 16.0),
        });
        assert_eq!(m.feedback_text(), Some("GUARD BREAK"));
        m.tick(FEEDBACK_SHOW + 0.01);
        assert_eq!(m.feedback_text(), None);
        assert_eq!(m.counts.feedbacks, 1);
        m.on_event(&Event::LockChanged { target: Some(3) });
        assert_eq!(m.counts.lock_changes, 1);
        m.on_event(&telegraph(3, 1.0));
        m.clear();
        assert!(m.threat().is_none());
    }

    #[test]
    fn lock_line_reports_sight_and_lift() {
        assert_eq!(lock_line(None, 0.5, true), "");
        assert_eq!(lock_line(Some("Orc"), 0.0, false), "LOCK  Orc");
        assert_eq!(
            lock_line(Some("Orc"), 0.426, true),
            "LOCK  Orc  |  NO SIGHT 43%  |  CAMERA LIFT"
        );
        assert_eq!(
            lock_line(Some("Orc"), 3.0, false),
            "LOCK  Orc  |  NO SIGHT 100%"
        );
    }
}
