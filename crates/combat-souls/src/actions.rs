//! The action table: durations, input and cancel windows, i-frames, hit windows and root motion
//! of every player action, in frames of 30 fps animation (`tuning::DF` per tick).
//!
//! All numbers are our own estimates for feel; none are taken from another game's data. The
//! per-action animation is looked up separately ([`crate::animmap`]) so the Gothic 3 motions can
//! be replaced without touching timing.

/// Direction relative to the facing.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Dir {
    Front,
    Back,
    Left,
    Right,
}

/// How hard a hit throws the victim about.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Hurt {
    Small,
    Middle,
    Large,
    Knockdown,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum ActionId {
    Roll(Dir),
    Backstep,
    /// Light chain, 1..=3.
    Light(u8),
    Heavy,
    /// Light attack out of a sprint.
    RunLight,
    /// Light attack straight out of a roll.
    RollLight,
    /// Heavy attack right after a block.
    GuardCounter,
    /// Critical strike on a staggered or parried enemy.
    Riposte,
    GuardHit,
    GuardBreak,
    /// Hit reaction.
    Hurt(Hurt),
    /// Poise-break stagger of the player.
    Stagger,
}

impl ActionId {
    pub fn is_attack(self) -> bool {
        matches!(
            self,
            Self::Light(_)
                | Self::Heavy
                | Self::RunLight
                | Self::RollLight
                | Self::GuardCounter
                | Self::Riposte
        )
    }
    pub fn is_roll(self) -> bool {
        matches!(self, Self::Roll(_) | Self::Backstep)
    }
    pub fn is_reaction(self) -> bool {
        matches!(
            self,
            Self::GuardHit | Self::GuardBreak | Self::Hurt(_) | Self::Stagger
        )
    }
}

/// Which request the pipeline sees; the Gothic move class follows from it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum StrikeKind {
    Light,
    Heavy,
    Charged,
    Riposte,
    Counter,
    /// An enemy swing.
    Foe,
}

#[derive(Clone, Copy, Debug)]
pub struct HitDef {
    pub from: f32,
    pub to: f32,
    /// Poise damage dealt to the victim.
    pub poise: f32,
    /// Seconds both sides freeze for on contact.
    pub stop: f32,
    /// Half-angle of the cone in front that the swing covers.
    pub arc_deg: f32,
    pub kind: StrikeKind,
}

#[derive(Clone, Copy, Debug)]
pub struct Travel {
    pub from: f32,
    pub to: f32,
    /// Total ground distance over the window (cm), smoothly eased.
    pub dist: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct ActionDef {
    pub name: &'static str,
    pub total: f32,
    /// Presses before this frame are ignored rather than buffered.
    pub input_from: f32,
    /// Earliest frame a buffered request may interrupt.
    pub cancel_light: f32,
    pub cancel_heavy: f32,
    pub cancel_dodge: f32,
    pub cancel_guard: f32,
    pub cancel_move: f32,
    /// Invincibility window `from <= f < to`.
    pub iframes: (f32, f32),
    pub hit: Option<HitDef>,
    pub travel: Option<Travel>,
    /// Heavy attacks wait here while the button is held (charge).
    pub hold: Option<f32>,
    /// Frames during which the character can still turn.
    pub turn_until: f32,
    /// Super armour: light hits do not flinch the attacker inside this window.
    pub armour: Option<(f32, f32)>,
}

pub const NEVER: f32 = 9999.0;

const BASE: ActionDef = ActionDef {
    name: "",
    total: 20.0,
    input_from: 0.0,
    cancel_light: NEVER,
    cancel_heavy: NEVER,
    cancel_dodge: NEVER,
    cancel_guard: NEVER,
    cancel_move: NEVER,
    iframes: (0.0, 0.0),
    hit: None,
    travel: None,
    hold: None,
    turn_until: 0.0,
    armour: None,
};

const fn hit(from: f32, to: f32, poise: f32, stop: f32, kind: StrikeKind) -> Option<HitDef> {
    Some(HitDef {
        from,
        to,
        poise,
        stop,
        arc_deg: 65.0,
        kind,
    })
}

const fn travel(from: f32, to: f32, dist: f32) -> Option<Travel> {
    Some(Travel { from, to, dist })
}

static ROLL: ActionDef = ActionDef {
    name: "roll",
    total: 28.0,
    input_from: 8.0,
    cancel_light: 14.0,
    cancel_heavy: 24.0,
    cancel_dodge: 22.0,
    cancel_guard: 20.0,
    cancel_move: 22.0,
    iframes: (3.0, 14.0),
    travel: travel(0.0, 21.0, 340.0),
    ..BASE
};
static BACKSTEP: ActionDef = ActionDef {
    name: "backstep",
    total: 20.0,
    input_from: 6.0,
    cancel_light: 12.0,
    cancel_heavy: 16.0,
    cancel_dodge: 15.0,
    cancel_guard: 14.0,
    cancel_move: 15.0,
    iframes: (1.0, 6.0),
    travel: travel(0.0, 14.0, 230.0),
    ..BASE
};
static LIGHT1: ActionDef = ActionDef {
    name: "light1",
    total: 24.0,
    input_from: 6.0,
    cancel_light: 15.0,
    cancel_heavy: 15.0,
    cancel_dodge: 9.0,
    cancel_guard: 18.0,
    cancel_move: 21.0,
    hit: hit(9.0, 13.0, 14.0, 0.07, StrikeKind::Light),
    travel: travel(5.0, 11.0, 45.0),
    turn_until: 8.0,
    ..BASE
};
static LIGHT2: ActionDef = ActionDef {
    name: "light2",
    total: 26.0,
    input_from: 6.0,
    cancel_light: 16.0,
    cancel_heavy: 16.0,
    cancel_dodge: 10.0,
    cancel_guard: 19.0,
    cancel_move: 23.0,
    hit: hit(9.0, 13.0, 16.0, 0.07, StrikeKind::Light),
    travel: travel(5.0, 11.0, 40.0),
    turn_until: 8.0,
    ..BASE
};
static LIGHT3: ActionDef = ActionDef {
    name: "light3",
    total: 34.0,
    input_from: 8.0,
    cancel_light: 24.0,
    cancel_heavy: 24.0,
    cancel_dodge: 14.0,
    cancel_guard: 26.0,
    cancel_move: 30.0,
    hit: hit(12.0, 17.0, 24.0, 0.1, StrikeKind::Light),
    travel: travel(7.0, 15.0, 65.0),
    turn_until: 10.0,
    ..BASE
};
static HEAVY: ActionDef = ActionDef {
    name: "heavy",
    total: 46.0,
    input_from: 10.0,
    cancel_light: 34.0,
    cancel_heavy: 34.0,
    cancel_dodge: 30.0,
    cancel_guard: 36.0,
    cancel_move: 40.0,
    hit: hit(22.0, 27.0, 40.0, 0.14, StrikeKind::Heavy),
    travel: travel(16.0, 24.0, 80.0),
    hold: Some(12.0),
    turn_until: 14.0,
    armour: Some((12.0, 30.0)),
    ..BASE
};
static RUN_LIGHT: ActionDef = ActionDef {
    name: "run_light",
    total: 32.0,
    input_from: 10.0,
    cancel_light: 22.0,
    cancel_heavy: 22.0,
    cancel_dodge: 14.0,
    cancel_guard: 24.0,
    cancel_move: 26.0,
    hit: hit(10.0, 15.0, 22.0, 0.08, StrikeKind::Light),
    travel: travel(0.0, 14.0, 170.0),
    turn_until: 6.0,
    ..BASE
};
static ROLL_LIGHT: ActionDef = ActionDef {
    name: "roll_light",
    total: 28.0,
    input_from: 8.0,
    cancel_light: 20.0,
    cancel_heavy: 20.0,
    cancel_dodge: 12.0,
    cancel_guard: 22.0,
    cancel_move: 24.0,
    hit: hit(8.0, 12.0, 14.0, 0.07, StrikeKind::Light),
    travel: travel(3.0, 9.0, 55.0),
    turn_until: 6.0,
    ..BASE
};
static GUARD_COUNTER: ActionDef = ActionDef {
    name: "guard_counter",
    total: 40.0,
    input_from: 10.0,
    cancel_light: 32.0,
    cancel_heavy: 32.0,
    cancel_dodge: 24.0,
    cancel_guard: 32.0,
    cancel_move: 36.0,
    hit: hit(14.0, 19.0, 45.0, 0.12, StrikeKind::Counter),
    travel: travel(8.0, 16.0, 70.0),
    turn_until: 6.0,
    ..BASE
};
static RIPOSTE: ActionDef = ActionDef {
    name: "riposte",
    total: 62.0,
    input_from: NEVER,
    cancel_dodge: 52.0,
    cancel_move: 56.0,
    iframes: (0.0, 56.0),
    hit: hit(30.0, 33.0, 100.0, 0.25, StrikeKind::Riposte),
    ..BASE
};
static GUARD_HIT: ActionDef = ActionDef {
    name: "guard_hit",
    total: 14.0,
    input_from: 4.0,
    cancel_light: 11.0,
    cancel_heavy: 11.0,
    cancel_dodge: 8.0,
    cancel_guard: 6.0,
    cancel_move: 10.0,
    ..BASE
};
static GUARD_BREAK: ActionDef = ActionDef {
    name: "guard_break",
    total: 56.0,
    input_from: 40.0,
    cancel_move: 52.0,
    ..BASE
};
static STAGGER: ActionDef = ActionDef {
    name: "stagger",
    total: 40.0,
    input_from: 28.0,
    cancel_dodge: 34.0,
    cancel_move: 38.0,
    ..BASE
};
static HURT_SMALL: ActionDef = ActionDef {
    name: "hurt_small",
    total: 14.0,
    input_from: 6.0,
    cancel_dodge: 9.0,
    cancel_guard: 10.0,
    cancel_move: 12.0,
    ..BASE
};
static HURT_MIDDLE: ActionDef = ActionDef {
    name: "hurt_middle",
    total: 22.0,
    input_from: 10.0,
    cancel_dodge: 16.0,
    cancel_guard: 18.0,
    cancel_move: 19.0,
    ..BASE
};
static HURT_LARGE: ActionDef = ActionDef {
    name: "hurt_large",
    total: 34.0,
    input_from: 20.0,
    cancel_dodge: 28.0,
    cancel_move: 31.0,
    ..BASE
};
static KNOCKDOWN: ActionDef = ActionDef {
    name: "knockdown",
    total: 80.0,
    input_from: NEVER,
    cancel_move: 70.0,
    iframes: (30.0, 66.0),
    travel: travel(0.0, 16.0, -180.0),
    ..BASE
};

/// Timing read from a locally generated clip pack (frames at 30 fps; see
/// [`install_timing`]). Every field is optional: what is `None` keeps the built-in value.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Timing {
    pub total: f32,
    pub input_from: Option<f32>,
    pub cancel_light: Option<f32>,
    pub cancel_heavy: Option<f32>,
    pub cancel_dodge: Option<f32>,
    pub cancel_guard: Option<f32>,
    pub cancel_move: Option<f32>,
    pub iframes: Option<(f32, f32)>,
    /// First hit window `(from, to)`; keeps the built-in poise, hit-stop and arc.
    pub hit: Option<(f32, f32)>,
    pub travel: Option<(f32, f32, f32)>,
    /// Frame the heavy attack waits at while the button is held.
    pub hold: Option<f32>,
}

/// Number of distinct timing slots (see [`ActionId::slot`]).
const SLOTS: usize = 16;

static OVERRIDES: std::sync::RwLock<[Option<&'static ActionDef>; SLOTS]> =
    std::sync::RwLock::new([None; SLOTS]);

impl ActionId {
    /// Index of the action's built-in timing entry; actions sharing one definition (all four
    /// rolls, the later chain links) share the slot.
    pub fn slot(self) -> usize {
        match self {
            Self::Roll(_) => 0,
            Self::Backstep => 1,
            Self::Light(1) => 2,
            Self::Light(2) => 3,
            Self::Light(_) => 4,
            Self::Heavy => 5,
            Self::RunLight => 6,
            Self::RollLight => 7,
            Self::GuardCounter => 8,
            Self::Riposte => 9,
            Self::GuardHit => 10,
            Self::GuardBreak => 11,
            Self::Stagger => 12,
            Self::Hurt(Hurt::Small) => 13,
            Self::Hurt(Hurt::Middle) => 14,
            Self::Hurt(Hurt::Large) | Self::Hurt(Hurt::Knockdown) => 15,
        }
    }

    fn builtin(self) -> &'static ActionDef {
        match self {
            Self::Roll(_) => &ROLL,
            Self::Backstep => &BACKSTEP,
            Self::Light(1) => &LIGHT1,
            Self::Light(2) => &LIGHT2,
            Self::Light(_) => &LIGHT3,
            Self::Heavy => &HEAVY,
            Self::RunLight => &RUN_LIGHT,
            Self::RollLight => &ROLL_LIGHT,
            Self::GuardCounter => &GUARD_COUNTER,
            Self::Riposte => &RIPOSTE,
            Self::GuardHit => &GUARD_HIT,
            Self::GuardBreak => &GUARD_BREAK,
            Self::Stagger => &STAGGER,
            Self::Hurt(Hurt::Small) => &HURT_SMALL,
            Self::Hurt(Hurt::Middle) => &HURT_MIDDLE,
            Self::Hurt(Hurt::Large) => &HURT_LARGE,
            Self::Hurt(Hurt::Knockdown) => &KNOCKDOWN,
        }
    }

    /// The action's timing: the installed override when there is one, else the built-in table.
    pub fn def(self) -> &'static ActionDef {
        if let Ok(o) = OVERRIDES.read()
            && let Some(d) = o[self.slot()]
        {
            return d;
        }
        self.builtin()
    }

    /// Stamina the action costs to start.
    pub fn cost(self, t: &crate::tuning::Tuning) -> f32 {
        match self {
            Self::Roll(_) => t.roll_cost,
            Self::Backstep => t.backstep_cost,
            Self::Light(1) => 10.0,
            Self::Light(2) => 11.0,
            Self::Light(_) => 14.0,
            Self::RunLight => 14.0,
            Self::RollLight => 11.0,
            Self::Heavy => 22.0,
            Self::GuardCounter => 18.0,
            _ => 0.0,
        }
    }
}

/// Replaces the timing of the action's slot (for example with windows read from a clip pack).
/// Idempotent; the previous override of the slot is leaked (a handful of small records per
/// process, installs happen once at startup).
pub fn install_timing(id: ActionId, t: &Timing) {
    let leaked: &'static ActionDef = Box::leak(Box::new(apply_timing(id.builtin(), t)));
    if let Ok(mut o) = OVERRIDES.write() {
        o[id.slot()] = Some(leaked);
    }
}

/// `base` with the pack timing applied and every window clamped into the new length.
pub fn apply_timing(base: &ActionDef, t: &Timing) -> ActionDef {
    let mut d = *base;
    d.total = t.total.max(1.0);
    macro_rules! set {
        ($f:ident) => {
            if let Some(v) = t.$f {
                d.$f = v;
            }
        };
    }
    set!(input_from);
    set!(cancel_light);
    set!(cancel_heavy);
    set!(cancel_dodge);
    set!(cancel_guard);
    set!(cancel_move);
    set!(iframes);
    if let Some((from, to)) = t.hit
        && let Some(h) = d.hit.as_mut()
    {
        h.from = from;
        h.to = to.max(from + 1.0);
    }
    if let Some((from, to, dist)) = t.travel {
        d.travel = Some(Travel { from, to, dist });
    }
    if let Some(h) = t.hold {
        d.hold = Some(h);
    }
    // Windows that were authored for another length must still lie inside the action.
    let total = d.total;
    let clamp = |v: &mut f32| {
        if *v != NEVER && *v > total {
            *v = total;
        }
    };
    for v in [
        &mut d.cancel_light,
        &mut d.cancel_heavy,
        &mut d.cancel_dodge,
        &mut d.cancel_guard,
        &mut d.cancel_move,
        &mut d.input_from,
    ] {
        clamp(v);
    }
    d.iframes.1 = d.iframes.1.min(total);
    d.iframes.0 = d.iframes.0.min(d.iframes.1);
    if let Some(h) = d.hit.as_mut() {
        h.to = h.to.min(total);
        h.from = h.from.min(h.to - 1.0).max(0.0);
    }
    if let Some(tr) = d.travel.as_mut() {
        tr.to = tr.to.min(total);
        tr.from = tr.from.min(tr.to - 1.0).max(0.0);
    }
    d
}

/// Removes every installed timing override (back to the built-in table).
pub fn clear_timing() {
    if let Ok(mut o) = OVERRIDES.write() {
        *o = [None; SLOTS];
    }
}

impl ActionDef {
    /// Cumulative ground travel (cm) after `frame`.
    pub fn travelled(&self, frame: f32) -> f32 {
        match self.travel {
            Some(t) if t.to > t.from => {
                t.dist * crate::math::smoothstep((frame - t.from) / (t.to - t.from))
            }
            _ => 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applied_timing_replaces_and_clamps() {
        let d = apply_timing(
            ActionId::Light(1).def(),
            &Timing {
                total: 33.0,
                cancel_move: Some(50.0),
                hit: Some((10.0, 12.0)),
                iframes: Some((2.0, 40.0)),
                ..Default::default()
            },
        );
        assert_eq!(d.total, 33.0);
        assert_eq!(d.cancel_move, 33.0, "clamped into the action");
        assert_eq!(d.iframes, (2.0, 33.0));
        let h = d.hit.unwrap();
        assert_eq!((h.from, h.to), (10.0, 12.0));
        assert!(h.poise > 0.0, "built-in poise kept");
    }

    #[test]
    fn every_window_lies_inside_the_animation() {
        let ids = [
            ActionId::Roll(Dir::Front),
            ActionId::Backstep,
            ActionId::Light(1),
            ActionId::Light(2),
            ActionId::Light(3),
            ActionId::Heavy,
            ActionId::RunLight,
            ActionId::RollLight,
            ActionId::GuardCounter,
            ActionId::Riposte,
            ActionId::GuardHit,
            ActionId::GuardBreak,
            ActionId::Stagger,
            ActionId::Hurt(Hurt::Small),
            ActionId::Hurt(Hurt::Middle),
            ActionId::Hurt(Hurt::Large),
            ActionId::Hurt(Hurt::Knockdown),
        ];
        for id in ids {
            let d = id.def();
            assert!(
                d.iframes.0 <= d.iframes.1 && d.iframes.1 <= d.total,
                "{}",
                d.name
            );
            if let Some(h) = d.hit {
                assert!(h.from < h.to && h.to <= d.total, "{}", d.name);
            }
            if let Some(t) = d.travel {
                assert!(t.from < t.to && t.to <= d.total, "{}", d.name);
            }
            assert!(
                d.cancel_move <= d.total || d.cancel_move == NEVER,
                "{}",
                d.name
            );
        }
    }
}
