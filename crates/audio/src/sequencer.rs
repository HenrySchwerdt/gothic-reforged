//! The music sequencer: logical state, triggers, fragment choice, stinger transitions and
//! the due-time event queue (ENGINE_MAP C10; batches E35 / E36).
//!
//! The sequencer is pure: it owns no audio device. The host feeds it the frame time and
//! executes the returned [`Action`]s on three virtual channels (two fragment channels that swap
//! roles and one stinger channel), exactly the layout of `gCMusicSequencer`.
//!
//! Where the original reads data this installation does not ship, the gap is filled and marked:
//! the exit and entrance links of a fragment come from sync points embedded in the authoring
//! `.wav`, which are lost in the shipped Ogg files, so links are synthesised on a fixed grid
//! (see [`LinkModel`]).
use crate::music::{Fragment, MusicLibrary, Transition, Trigger as TableTrigger};
use crate::rng::Rng;

/// Length of the digital silence every fragment starts with (AUDIO_FORMATS section 3).
pub const SILENT_LEAD_MS: f64 = 10_000.0;

/// `gEMusicTriggerTime`: when the old music is left.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExitTime {
    Immediately,
    #[default]
    NextExit,
    NextPriorityExit,
    EndOfFragment,
}

impl ExitTime {
    pub fn from_index(i: i32) -> Self {
        match i {
            0 => Self::Immediately,
            2 => Self::NextPriorityExit,
            3 => Self::EndOfFragment,
            _ => Self::NextExit,
        }
    }
}

/// `gEMusicFragmentPosition`: where the new fragment is entered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Entrance {
    #[default]
    Begin,
    Current,
    RandomEntrance,
}

impl Entrance {
    pub fn from_index(i: i32) -> Self {
        match i {
            1 => Self::Current,
            2 => Self::RandomEntrance,
            _ => Self::Begin,
        }
    }
}

/// `gEMusicTransition`: how the stinger between two fragments is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransitionMode {
    /// The any-to-any stinger.
    Default,
    /// Connectors of the two links.
    #[default]
    MatchingConnector,
    /// Connectors given by the trigger.
    SpecifiedConnector,
    /// Stinger given by name.
    SpecifiedTransition,
}

impl TransitionMode {
    pub fn from_index(i: i32) -> Self {
        match i {
            0 => Self::Default,
            2 => Self::SpecifiedConnector,
            3 => Self::SpecifiedTransition,
            _ => Self::MatchingConnector,
        }
    }
}

/// `gEMusicSequence`: how one fragment is picked among equal matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SequenceMode {
    /// First fragment (list order) not yet played in this cycle.
    #[default]
    Sequential,
    /// Random among the fragments not yet played in this cycle.
    Shuffle,
}

/// A request to the sequencer (`gSMusicTrigger`). Empty strings leave that part of the state
/// unchanged.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Trigger {
    pub location: String,
    pub situation: String,
    pub day_time: String,
    pub variation: String,
    pub exit: ExitTime,
    pub entrance: Entrance,
    pub mode: TransitionMode,
    pub connect_in: i32,
    pub connect_out: i32,
    /// Stinger sample for [`TransitionMode::SpecifiedTransition`].
    pub transition: String,
    pub stop: bool,
    /// Executed even when the state already matches (follow triggers).
    pub force: bool,
}

impl Trigger {
    /// The partial trigger `gCMusicModule::SetLocation/SetSituation/SetDayTime` builds.
    pub fn partial(location: &str, situation: &str, day_time: &str) -> Self {
        Self {
            location: location.to_owned(),
            situation: situation.to_owned(),
            day_time: day_time.to_owned(),
            entrance: Entrance::RandomEntrance,
            ..Self::default()
        }
    }

    /// A `gCMusicTrigger` row of the music table.
    pub fn from_table(t: &TableTrigger) -> Self {
        Self {
            location: t.location.clone(),
            situation: t.situation.clone(),
            day_time: t.day_time.clone(),
            variation: t.variation.clone(),
            exit: ExitTime::from_index(t.exit),
            entrance: Entrance::from_index(t.entrance),
            mode: TransitionMode::from_index(t.transition_mode),
            connect_in: t.connect_in,
            connect_out: t.connect_out,
            transition: t.transition.clone(),
            stop: t.is_stop,
            force: false,
        }
    }
}

/// Logical state of the sequencer (`+0x10`, `+0x14`, `+0x18`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    pub location: String,
    pub situation: String,
    pub day_time: String,
}

/// Synthetic exit and entrance links (see the module note).
#[derive(Debug, Clone, Copy)]
pub struct LinkModel {
    /// Spacing of ordinary exit links, ms.
    pub exit_grid_ms: f64,
    /// Spacing of priority exit links, ms.
    pub priority_grid_ms: f64,
    /// Random entrances stay this far from the end of the fragment, ms.
    pub entrance_tail_ms: f64,
}

impl Default for LinkModel {
    fn default() -> Self {
        Self {
            exit_grid_ms: 4_000.0,
            priority_grid_ms: 16_000.0,
            entrance_tail_ms: 30_000.0,
        }
    }
}

/// Defaults of the fallback chain (the runtime globals `g_strDefaultMusic*`).
#[derive(Debug, Clone)]
pub struct Defaults {
    pub location: String,
    pub situation: String,
    pub day_time: String,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            location: "myrtana".into(),
            situation: "explore".into(),
            day_time: "day".into(),
        }
    }
}

/// Virtual channel numbers: two fragment channels and the stinger channel.
pub type Channel = u8;
pub const STINGER_CHANNEL: Channel = 2;

/// What the host has to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Start a fragment on `channel` at `start_ms` with `volume` (fragment volume times its
    /// current fade) and ramp the level in over `fade_in_secs` (0 = at once).
    /// `skip_silence`: nothing else is playing, so the host may skip the silent lead-in.
    Start {
        channel: Channel,
        fragment: usize,
        start_ms: f64,
        volume: f32,
        fade_in_secs: f32,
        skip_silence: bool,
    },
    /// Play the stinger sample on [`STINGER_CHANNEL`].
    StartStinger { transition: usize, volume: f32 },
    /// Set the level of a fragment channel (stinger envelope).
    Volume { channel: Channel, volume: f32 },
    /// Stop a channel, fading out over `fade_secs`.
    Stop { channel: Channel, fade_secs: f32 },
}

#[derive(Debug, Clone, PartialEq)]
enum Kind {
    Fragment {
        fragment: usize,
        start_ms: f64,
        stinger: bool,
    },
    Transition {
        transition: usize,
    },
    Stop,
}

#[derive(Debug, Clone)]
struct Event {
    due_ms: f64,
    kind: Kind,
    /// State the event was planned for; an event whose state no longer holds is dropped.
    state: State,
    /// State applied when the event runs (a story cue hands over to exploration).
    then: Option<State>,
}

#[derive(Debug, Clone, Copy)]
struct Chan {
    fragment: usize,
    pos_ms: f64,
}

#[derive(Debug, Clone, Copy)]
struct Stinger {
    transition: usize,
    pos_ms: f64,
    src: Option<Channel>,
    dst: Channel,
}

pub struct Sequencer {
    pub defaults: Defaults,
    pub links: LinkModel,
    pub sequence: SequenceMode,
    /// Plain cross-fade used when no stinger exists, seconds.
    pub crossfade_secs: f32,
    /// Fade used when music stops, seconds.
    pub stop_fade_secs: f32,
    state: State,
    accept: bool,
    chans: [Option<Chan>; 2],
    primary: usize,
    stinger: Option<Stinger>,
    events: Vec<Event>,
    played: Vec<bool>,
    waiting_for_begin: bool,
    rng: Rng,
    last_volume: [f32; 3],
    /// `[Trigger] old => new` lines, newest last (bounded).
    pub log: Vec<String>,
}

impl Sequencer {
    pub fn new(lib: &MusicLibrary, seed: u64) -> Self {
        Self {
            defaults: Defaults::default(),
            links: LinkModel::default(),
            sequence: SequenceMode::default(),
            crossfade_secs: 3.0,
            stop_fade_secs: 1.5,
            state: State {
                situation: "explore".into(),
                day_time: "day".into(),
                ..State::default()
            },
            accept: true,
            chans: [None, None],
            primary: 0,
            stinger: None,
            events: Vec::new(),
            played: vec![false; lib.fragments.len()],
            waiting_for_begin: false,
            rng: Rng::new(seed),
            last_volume: [f32::NAN; 3],
            log: Vec::new(),
        }
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    /// Fragment on the primary channel, if any.
    pub fn current_fragment(&self) -> Option<usize> {
        self.chans[self.primary].map(|c| c.fragment)
    }

    pub fn is_playing(&self) -> bool {
        self.chans[self.primary].is_some()
    }

    pub fn pending_events(&self) -> usize {
        self.events.len()
    }

    /// Moves the clock of a fragment channel (the host skipped the silent lead-in of a fragment
    /// it started from silence, so the sample is that far ahead of the planned position).
    pub fn seek(&mut self, channel: Channel, pos_ms: f64) {
        if let Some(Some(c)) = self.chans.get_mut(usize::from(channel)) {
            c.pos_ms = pos_ms;
        }
    }

    /// Port of gCMusicSequencer::SetAcceptTriggers: while off every trigger is ignored.
    pub fn set_accept_triggers(&mut self, accept: bool) {
        self.accept = accept;
    }

    /// Port of gCMusicSequencer::IsInState (Game:0x100fde20): every non-empty field of the
    /// trigger equals the state (and a requested variation equals the playing one).
    pub fn in_state(&self, lib: &MusicLibrary, t: &Trigger) -> bool {
        let same = |want: &str, have: &str| want.is_empty() || want.eq_ignore_ascii_case(have);
        let variation_ok = t.variation.is_empty()
            || self.current_fragment().is_some_and(|f| {
                t.variation
                    .eq_ignore_ascii_case(&lib.fragments[f].variation)
            });
        same(&t.location, &self.state.location)
            && same(&t.situation, &self.state.situation)
            && same(&t.day_time, &self.state.day_time)
            && variation_ok
    }

    /// Port of gCMusicSequencer::Trigger (Game:0x100febd0) and ExecuteTrigger (0x100fec60).
    pub fn trigger(&mut self, lib: &MusicLibrary, t: &Trigger) -> Vec<Action> {
        let mut out = Vec::new();
        if !self.accept {
            return out;
        }
        if t.stop {
            if !self.is_playing() {
                return out;
            }
            self.events.clear();
            let due = self.exit_time(lib, t.exit, 0.0).unwrap_or(0.0);
            self.push_event(due, Kind::Stop, None);
            self.log_line(format!("[Trigger] stop at {due:.0} ms"));
            return out;
        }
        if !t.force && self.is_playing() && self.in_state(lib, t) {
            return out;
        }
        if self.waiting_for_begin {
            return out;
        }
        let Some(fragment) = self.find_fragment(lib, t) else {
            self.log_line("[Trigger] Could not find any fallback fragment. Giving up".into());
            return out;
        };
        let old = self.state.clone();
        self.apply_state(t);
        self.log_line(format!(
            "[Trigger] {}/{}/{} => {}/{}/{}",
            old.location,
            old.situation,
            old.day_time,
            self.state.location,
            self.state.situation,
            self.state.day_time
        ));
        if !t.force && self.current_fragment() == Some(fragment) {
            // Same music under a new name (a fallback hit): keep the queued follow events.
            for e in &mut self.events {
                e.state.clone_from(&self.state);
            }
            return out;
        }
        self.events.clear();
        self.plan(lib, t, fragment, None);
        // A fragment due at once (nothing playing) starts in this very call.
        self.start_due_events(lib, &mut out);
        out
    }

    /// Advances the channel clocks by `dt` seconds: runs due events, applies the stinger
    /// envelope and stops everything when the primary fragment ran out.
    pub fn advance(&mut self, lib: &MusicLibrary, dt: f32) -> Vec<Action> {
        let mut out = Vec::new();
        let dt_ms = f64::from(dt) * 1000.0;
        for c in self.chans.iter_mut().flatten() {
            c.pos_ms += dt_ms;
        }
        if let Some(s) = &mut self.stinger {
            s.pos_ms += dt_ms;
        }
        self.start_due_events(lib, &mut out);
        self.update_envelope(lib, &mut out);
        self.update_channels(lib, &mut out);
        out
    }

    // --- state ------------------------------------------------------------------------------

    fn apply_state(&mut self, t: &Trigger) {
        let set = |dst: &mut String, src: &str| {
            if !src.is_empty() {
                *dst = src.to_ascii_lowercase();
            }
        };
        set(&mut self.state.location, &t.location);
        set(&mut self.state.situation, &t.situation);
        set(&mut self.state.day_time, &t.day_time);
    }

    fn log_line(&mut self, line: String) {
        if self.log.len() >= 64 {
            self.log.remove(0);
        }
        self.log.push(line);
    }

    // --- fragment choice (C10.3) --------------------------------------------------------------

    /// Port of gCMusicSequencer::FindFragment (Game:0x100fdfd0) with the fallback chain of
    /// `FUN_100fda80`: empty fields come from the state; when nothing matches the variation, then
    /// the daytime, the location and finally the situation are replaced by their defaults.
    pub fn find_fragment(&mut self, lib: &MusicLibrary, t: &Trigger) -> Option<usize> {
        let fill = |want: &str, have: &str| {
            if want.is_empty() {
                have.to_owned()
            } else {
                want.to_owned()
            }
        };
        let mut q = State {
            location: fill(&t.location, &self.state.location),
            situation: fill(&t.situation, &self.state.situation),
            day_time: fill(&t.day_time, &self.state.day_time),
        };
        let mut variation = t.variation.clone();
        loop {
            let matches: Vec<usize> = lib
                .fragments
                .iter()
                .enumerate()
                .filter(|(_, f)| matches_query(f, &q, &variation))
                .map(|(i, _)| i)
                .collect();
            if let Some(i) = self.select(&matches) {
                return Some(i);
            }
            let d = &self.defaults;
            if !variation.is_empty() {
                variation.clear();
            } else if !q.day_time.eq_ignore_ascii_case(&d.day_time) {
                q.day_time = d.day_time.clone();
            } else if !q.location.eq_ignore_ascii_case(&d.location) {
                q.location = d.location.clone();
            } else if !q.situation.eq_ignore_ascii_case(&d.situation) {
                q.situation = d.situation.clone();
            } else {
                return None;
            }
        }
    }

    /// Port of gCMusicFragmentList::SelectFragment (Game:0x100f98d0): take an unmarked match,
    /// mark it, reset the marks of the group once all were played.
    fn select(&mut self, matches: &[usize]) -> Option<usize> {
        if matches.is_empty() {
            return None;
        }
        if matches.iter().all(|&i| self.played[i]) {
            for &i in matches {
                self.played[i] = false;
            }
        }
        let open: Vec<usize> = matches
            .iter()
            .copied()
            .filter(|&i| !self.played[i])
            .collect();
        let pick = match self.sequence {
            SequenceMode::Sequential => open[0],
            SequenceMode::Shuffle => open[self.rng.below(open.len())],
        };
        self.played[pick] = true;
        Some(pick)
    }

    // --- planning (C10.4) -------------------------------------------------------------------

    fn plan(&mut self, lib: &MusicLibrary, t: &Trigger, fragment: usize, then: Option<State>) {
        let new = &lib.fragments[fragment];
        let state = self.state.clone();
        let Some(cur) = self.chans[self.primary] else {
            let start = self.entrance_start(t.entrance, new, 0.0);
            self.waiting_for_begin = true;
            self.events.push(Event {
                due_ms: 0.0,
                kind: Kind::Fragment {
                    fragment,
                    start_ms: start,
                    stinger: false,
                },
                state,
                then,
            });
            return;
        };
        // Synthetic links carry connector 0 (see module note).
        let stinger = self.find_transition(lib, t, 0, 0);
        let trans_ms = stinger.map_or(0.0, |i| f64::from(lib.transitions[i].duration_ms));
        let exit_ms = self.exit_time(lib, t.exit, trans_ms).unwrap_or(cur.pos_ms);
        let start = self.entrance_start(t.entrance, new, exit_ms);
        if let Some(i) = stinger {
            self.push_event(exit_ms, Kind::Transition { transition: i }, None);
        }
        self.push_event(
            exit_ms,
            Kind::Fragment {
                fragment,
                start_ms: start,
                stinger: stinger.is_some(),
            },
            then,
        );
    }

    fn push_event(&mut self, due_ms: f64, kind: Kind, then: Option<State>) {
        let state = self.state.clone();
        // Stable insertion keeps the stinger ahead of the fragment queued for the same time.
        let at = self.events.partition_point(|e| e.due_ms <= due_ms);
        self.events.insert(
            at,
            Event {
                due_ms,
                kind,
                state,
                then,
            },
        );
    }

    /// Port of gCMusicFragment::FindLink / FindNextExit on the synthetic link grid: due time (ms
    /// on the primary channel) of the exit the trigger asks for.
    fn exit_time(&self, lib: &MusicLibrary, time: ExitTime, stinger_ms: f64) -> Option<f64> {
        let cur = self.chans[self.primary]?;
        let frag = &lib.fragments[cur.fragment];
        let end = (f64::from(frag.duration_ms) - stinger_ms).max(cur.pos_ms);
        let on_grid = |grid: f64| ((cur.pos_ms + 100.0) / grid).ceil() * grid;
        Some(match time {
            ExitTime::Immediately => cur.pos_ms,
            ExitTime::NextExit => on_grid(self.links.exit_grid_ms).min(end),
            ExitTime::NextPriorityExit => on_grid(self.links.priority_grid_ms).min(end),
            ExitTime::EndOfFragment => end,
        })
    }

    /// Where the new fragment is entered (gEMusicFragmentPosition).
    fn entrance_start(&mut self, kind: Entrance, new: &Fragment, at_exit_ms: f64) -> f64 {
        let duration = f64::from(new.duration_ms);
        match kind {
            Entrance::Begin => 0.0,
            Entrance::Current => at_exit_ms.min((duration - 1000.0).max(0.0)),
            Entrance::RandomEntrance => {
                let grid = self.links.exit_grid_ms;
                let first = SILENT_LEAD_MS;
                let last = duration - self.links.entrance_tail_ms;
                if last <= first + grid {
                    return 0.0;
                }
                let slots = ((last - first) / grid) as usize;
                first + grid * self.rng.below(slots + 1) as f64
            }
        }
    }

    /// Port of gCMusicSequencer::FindTransition (Game:0x100fe150/0x100fe230).
    fn find_transition(
        &mut self,
        lib: &MusicLibrary,
        t: &Trigger,
        exit_connector: i32,
        entrance_connector: i32,
    ) -> Option<usize> {
        let by = |connect_in: i32, connect_out: i32| -> Vec<usize> {
            lib.transitions
                .iter()
                .enumerate()
                .filter(|(_, x)| x.connect_in == connect_in && x.connect_out == connect_out)
                .map(|(i, _)| i)
                .collect()
        };
        let named = |name: &str| -> Vec<usize> {
            let stem = name.trim_end_matches(".wav").to_ascii_lowercase();
            lib.transitions
                .iter()
                .enumerate()
                .filter(|(_, x)| {
                    x.sample
                        .trim_end_matches(".wav")
                        .eq_ignore_ascii_case(&stem)
                })
                .map(|(i, _)| i)
                .collect()
        };
        let mut found = match t.mode {
            TransitionMode::Default => Vec::new(),
            TransitionMode::MatchingConnector => by(entrance_connector, exit_connector),
            TransitionMode::SpecifiedConnector => by(t.connect_in, t.connect_out),
            TransitionMode::SpecifiedTransition => named(&t.transition),
        };
        if found.is_empty() {
            found = by(Transition::ANY_TO_ANY, Transition::ANY_TO_ANY);
        }
        (!found.is_empty()).then(|| found[self.rng.below(found.len())])
    }

    // --- running events ---------------------------------------------------------------------

    /// Port of gCMusicSequencer::StartDueEvents (Game:0x100fe920).
    fn start_due_events(&mut self, lib: &MusicLibrary, out: &mut Vec<Action>) {
        loop {
            let now = self.chans[self.primary].map_or(f64::INFINITY, |c| c.pos_ms);
            let Some(first) = self.events.first() else {
                return;
            };
            if first.due_ms > now && self.chans[self.primary].is_some() {
                return;
            }
            let ev = self.events.remove(0);
            if ev.state != self.state {
                // Obsolete: the logical state moved on after the event was queued.
                continue;
            }
            if let Some(after) = ev.then {
                self.state = after;
            }
            match ev.kind {
                Kind::Fragment {
                    fragment,
                    start_ms,
                    stinger,
                } => self.execute_fragment(lib, fragment, start_ms, stinger, out),
                Kind::Transition { transition } => self.execute_transition(lib, transition, out),
                Kind::Stop => self.execute_stop(out),
            }
        }
    }

    /// Port of Execute(fragment event) (Game:0x100fea10): the fragment starts on the free
    /// channel which becomes the primary one, pending events are dropped and the fragment's own
    /// follow trigger is issued.
    fn execute_fragment(
        &mut self,
        lib: &MusicLibrary,
        fragment: usize,
        start_ms: f64,
        stinger: bool,
        out: &mut Vec<Action>,
    ) {
        self.waiting_for_begin = false;
        let free = 1 - self.primary;
        let had_music = self.chans[self.primary].is_some();
        if self.chans[free].take().is_some() {
            out.push(Action::Stop {
                channel: free as Channel,
                fade_secs: 0.0,
            });
        }
        let frag = &lib.fragments[fragment];
        let dst_factor = if stinger {
            self.stinger.as_mut().map(|s| {
                s.dst = free as Channel;
                lib.transitions[s.transition].dst_volume(s.pos_ms)
            })
        } else {
            None
        };
        let (fade_in, volume) = match (dst_factor, had_music) {
            (Some(f), _) => (0.0, frag.volume * f),
            (None, true) => (self.crossfade_secs, frag.volume),
            (None, false) => (1.0, frag.volume),
        };
        if had_music && dst_factor.is_none() {
            out.push(Action::Stop {
                channel: self.primary as Channel,
                fade_secs: self.crossfade_secs,
            });
        }
        self.chans[free] = Some(Chan {
            fragment,
            pos_ms: start_ms,
        });
        self.primary = free;
        self.last_volume[free] = volume;
        out.push(Action::Start {
            channel: free as Channel,
            fragment,
            start_ms,
            volume,
            fade_in_secs: fade_in,
            skip_silence: !had_music,
        });
        self.events.clear();
        self.trigger_follow(lib, fragment);
    }

    /// Port of Execute(transition event) (Game:0x100feb10): the stinger plays on its own
    /// channel and fades the old fragment (and later the new one).
    fn execute_transition(&mut self, lib: &MusicLibrary, transition: usize, out: &mut Vec<Action>) {
        let src = self.chans[self.primary].map(|_| self.primary as Channel);
        self.stinger = Some(Stinger {
            transition,
            pos_ms: 0.0,
            src,
            dst: (1 - self.primary) as Channel,
        });
        self.last_volume[usize::from(STINGER_CHANNEL)] = f32::NAN;
        out.push(Action::StartStinger {
            transition,
            volume: lib.transitions[transition].volume,
        });
    }

    /// Port of Execute(stop event): all channels stop, events are cleared.
    fn execute_stop(&mut self, out: &mut Vec<Action>) {
        for (i, c) in self.chans.iter_mut().enumerate() {
            if c.take().is_some() {
                out.push(Action::Stop {
                    channel: i as Channel,
                    fade_secs: self.stop_fade_secs,
                });
            }
        }
        if self.stinger.take().is_some() {
            out.push(Action::Stop {
                channel: STINGER_CHANNEL,
                fade_secs: self.stop_fade_secs,
            });
        }
        self.events.clear();
        self.waiting_for_begin = false;
    }

    /// Port of gCMusicSequencer::TriggerFollow (Game:0x100ff040) with the rule of
    /// gCMusicFragment::GetFollowTrigger (0x100f2420): the location, situation and daytime stay
    /// as they are, the variation becomes `NextVariation` (the fragment's own one when empty:
    /// it repeats) and the old music is left at the end of the fragment. A cue
    /// (`NextVariation == -1`) hands over to the exploration music instead.
    fn trigger_follow(&mut self, lib: &MusicLibrary, fragment: usize) {
        let f = &lib.fragments[fragment];
        let next = f.next_variation.trim();
        let mut t = Trigger {
            exit: ExitTime::EndOfFragment,
            entrance: Entrance::Begin,
            mode: TransitionMode::Default,
            force: true,
            ..Trigger::default()
        };
        if next == "-1" {
            t.situation = self.defaults.situation.clone();
        } else if next.is_empty() {
            t.variation.clone_from(&f.variation);
        } else {
            t.variation = next.to_owned();
        }
        let frag_state = State {
            location: f.location.to_ascii_lowercase(),
            situation: f.situation.to_ascii_lowercase(),
            day_time: f.day_time.to_ascii_lowercase(),
        };
        // The follow fragment is searched inside the group that is playing (the state may name
        // a location without music, which fell back to another group).
        let saved = std::mem::replace(&mut self.state, frag_state.clone());
        let found = self.find_fragment(lib, &t);
        self.state = saved;
        if let Some(target) = found {
            let then = (next == "-1").then(|| {
                let mut after = self.state.clone();
                if !t.situation.is_empty() {
                    after.situation = t.situation.to_ascii_lowercase();
                }
                after
            });
            self.plan(lib, &t, target, then);
        }
    }

    // --- running channels (C10.5) -----------------------------------------------------------

    /// Port of gCMusicChannel::UpdateEnvelope / ApplyFadeVolume (Game:0x100fc4f0/0x100fc560).
    fn update_envelope(&mut self, lib: &MusicLibrary, out: &mut Vec<Action>) {
        let Some(s) = self.stinger else { return };
        let tr = &lib.transitions[s.transition];
        if s.pos_ms >= f64::from(tr.duration_ms) {
            if let Some(src) = s.src
                && self.chans[usize::from(src)].take().is_some()
            {
                out.push(Action::Stop {
                    channel: src,
                    fade_secs: 0.0,
                });
            }
            if let Some(c) = self.chans[usize::from(s.dst)] {
                self.set_volume(s.dst, lib.fragments[c.fragment].volume, out);
            }
            self.stinger = None;
            out.push(Action::Stop {
                channel: STINGER_CHANNEL,
                fade_secs: 0.0,
            });
            return;
        }
        if let Some(src) = s.src
            && let Some(c) = self.chans[usize::from(src)]
        {
            let v = lib.fragments[c.fragment].volume * tr.src_volume(s.pos_ms);
            self.set_volume(src, v, out);
        }
        if let Some(c) = self.chans[usize::from(s.dst)] {
            let v = lib.fragments[c.fragment].volume * tr.dst_volume(s.pos_ms);
            self.set_volume(s.dst, v, out);
        }
    }

    fn set_volume(&mut self, channel: Channel, volume: f32, out: &mut Vec<Action>) {
        let last = &mut self.last_volume[usize::from(channel)];
        if last.is_nan() || (*last - volume).abs() > 0.002 {
            *last = volume;
            out.push(Action::Volume { channel, volume });
        }
    }

    /// Port of gCMusicSequencer::UpdateChannels (Game:0x100fdf40): when the primary channel ran
    /// out nothing is playing any more and everything stops.
    fn update_channels(&mut self, lib: &MusicLibrary, out: &mut Vec<Action>) {
        let Some(c) = self.chans[self.primary] else {
            return;
        };
        if c.pos_ms < f64::from(lib.fragments[c.fragment].duration_ms) {
            return;
        }
        self.execute_stop(out);
    }
}

fn matches_query(f: &Fragment, q: &State, variation: &str) -> bool {
    let same = |want: &str, have: &str| want.is_empty() || want.eq_ignore_ascii_case(have);
    same(&q.location, &f.location)
        && same(&q.situation, &f.situation)
        && same(&q.day_time, &f.day_time)
        && same(variation, &f.variation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::music::test_library;

    fn lib() -> MusicLibrary {
        let mut l = test_library();
        for f in &mut l.fragments {
            f.duration_ms = 100_000;
            f.lead_out_begin_ms = 100_000;
        }
        let t = |cin: i32, cout: i32, name: &str, dur: i32| Transition {
            sample: name.into(),
            connect_in: cin,
            connect_out: cout,
            fade_out_begin_ms: 800,
            fade_out_end_ms: 3600,
            fade_in_begin_ms: 0,
            fade_in_end_ms: 800,
            offset_ms: 800,
            duration_ms: dur,
            volume: 1.0,
        };
        l.transitions = vec![
            t(666, 666, "transition_00.wav", 7000),
            t(0, 30, "transition_30.wav", 5000),
        ];
        l
    }

    fn begin(loc: &str, sit: &str, day: &str) -> Trigger {
        Trigger {
            entrance: Entrance::Begin,
            ..Trigger::partial(loc, sit, day)
        }
    }

    fn started(a: &[Action]) -> Vec<(Channel, usize)> {
        a.iter()
            .filter_map(|x| match x {
                Action::Start {
                    channel, fragment, ..
                } => Some((*channel, *fragment)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn first_trigger_starts_at_once_with_the_fallback_chain() {
        let l = lib();
        let mut s = Sequencer::new(&l, 1);
        // nothing for "nowhere": location falls back to myrtana, situation stays explore
        let a = s.trigger(&l, &Trigger::partial("nowhere", "explore", "day"));
        let st = started(&a);
        assert_eq!(st.len(), 1);
        let f = &l.fragments[st[0].1];
        assert_eq!(
            (f.location.as_str(), f.situation.as_str()),
            ("myrtana", "explore")
        );
        // the logical state keeps the requested location, so repeating the trigger is a no-op
        assert_eq!(s.state().location, "nowhere");
        assert!(
            s.trigger(&l, &Trigger::partial("nowhere", "", ""))
                .is_empty()
        );
        assert!(matches!(
            a[0],
            Action::Start {
                skip_silence: true,
                ..
            }
        ));
    }

    #[test]
    fn night_falls_back_to_day_and_variation_chain_round_robins() {
        let l = lib();
        let mut s = Sequencer::new(&l, 1);
        let f = s
            .find_fragment(&l, &Trigger::partial("braga", "explore", "night"))
            .unwrap();
        assert_eq!(l.fragments[f].location, "braga");
        // varant has variations 0 and 1: an unspecified variation takes them in list order
        let t = Trigger::partial("varant", "explore", "day");
        let a = s.find_fragment(&l, &t).unwrap();
        let b = s.find_fragment(&l, &t).unwrap();
        let c = s.find_fragment(&l, &t).unwrap();
        assert_eq!(
            [
                &l.fragments[a].variation,
                &l.fragments[b].variation,
                &l.fragments[c].variation
            ],
            ["0", "1", "0"]
        );
    }

    #[test]
    fn fight_waits_for_the_next_exit_then_stinger_covers_the_change() {
        let l = lib();
        let mut s = Sequencer::new(&l, 3);
        let first = s.trigger(&l, &begin("myrtana", "explore", "day"));
        let old = started(&first)[0].0;
        s.advance(&l, 1.0);
        let mut fight = Trigger::partial("", "fight", "");
        fight.entrance = Entrance::Begin;
        assert!(s.trigger(&l, &fight).is_empty());
        // queued: stinger first, then the fragment, both at the next 4 s grid point
        assert_eq!(s.pending_events(), 2);
        let early = s.advance(&l, 2.0);
        assert!(early.is_empty(), "{early:?} {:?}", s.log);
        let a = s.advance(&l, 1.2); // 4.2 s: due
        assert!(matches!(a[0], Action::StartStinger { transition: 0, .. }));
        let st = started(&a);
        assert_eq!(st.len(), 1);
        assert_eq!(l.fragments[st[0].1].situation, "fight");
        assert!(
            matches!(a[1], Action::Start { skip_silence: false, fade_in_secs, .. } if fade_in_secs == 0.0)
        );
        // while the stinger plays the old fragment fades linearly; afterwards it stops
        let a = s.advance(&l, 2.2); // stinger at 2.2 s: halfway through the fade-out
        let v = a
            .iter()
            .find_map(|x| match x {
                Action::Volume { channel, volume } if *channel == old => Some(*volume),
                _ => None,
            })
            .unwrap();
        assert!((v - (1.0 - (2200.0 - 800.0) / 2800.0)).abs() < 0.02, "{v}");
        let a = s.advance(&l, 5.0);
        assert!(a.contains(&Action::Stop {
            channel: old,
            fade_secs: 0.0
        }));
        assert_eq!(
            s.current_fragment()
                .map(|f| l.fragments[f].situation.clone()),
            Some("fight".into())
        );
    }

    #[test]
    fn stop_trigger_and_stop_gating() {
        let l = lib();
        let mut s = Sequencer::new(&l, 1);
        let stop = Trigger {
            stop: true,
            exit: ExitTime::Immediately,
            ..Trigger::default()
        };
        assert!(s.trigger(&l, &stop).is_empty() && s.pending_events() == 0);
        s.trigger(&l, &begin("myrtana", "explore", "day"));
        s.trigger(&l, &stop);
        let a = s.advance(&l, 0.1);
        assert!(matches!(a[0], Action::Stop { .. }));
        assert!(!s.is_playing());
    }

    #[test]
    fn single_fragment_loops_through_its_follow_trigger() {
        let l = lib();
        let mut s = Sequencer::new(&l, 5);
        s.trigger(&l, &begin("braga", "explore", "day"));
        // follow: end of fragment minus the 7 s stinger
        assert_eq!(s.pending_events(), 2);
        let a = s.advance(&l, 92.9);
        assert!(a.iter().all(|x| !matches!(x, Action::StartStinger { .. })));
        let a = s.advance(&l, 0.2);
        assert!(matches!(a[0], Action::StartStinger { .. }));
        assert_eq!(started(&a).len(), 1);
        assert!(s.log.iter().any(|l| l.contains("[Trigger]")));
    }

    #[test]
    fn story_cue_hands_over_to_exploration() {
        let l = lib();
        let mut s = Sequencer::new(&l, 5);
        s.trigger(&l, &begin("myrtana", "innos", "day"));
        assert_eq!(s.state().situation, "innos");
        let a = s.advance(&l, 93.1);
        let st = started(&a);
        assert_eq!(st.len(), 1);
        assert_eq!(l.fragments[st[0].1].situation, "explore");
        assert_eq!(s.state().situation, "explore");
    }

    #[test]
    fn ignored_while_triggers_are_disabled() {
        let l = lib();
        let mut s = Sequencer::new(&l, 1);
        s.set_accept_triggers(false);
        assert!(
            s.trigger(&l, &Trigger::partial("myrtana", "", ""))
                .is_empty()
        );
        assert!(!s.is_playing());
    }

    #[test]
    fn explicit_stinger_modes() {
        let l = lib();
        let mut s = Sequencer::new(&l, 9);
        let mut t = Trigger::partial("myrtana", "explore", "day");
        t.mode = TransitionMode::SpecifiedConnector;
        t.connect_in = 0;
        t.connect_out = 30;
        assert_eq!(s.find_transition(&l, &t, 0, 0), Some(1));
        t.mode = TransitionMode::SpecifiedTransition;
        t.transition = "transition_30".into();
        assert_eq!(s.find_transition(&l, &t, 0, 0), Some(1));
        t.mode = TransitionMode::MatchingConnector; // synthetic links: connector 0 -> default
        assert_eq!(s.find_transition(&l, &t, 0, 0), Some(0));
    }
}
