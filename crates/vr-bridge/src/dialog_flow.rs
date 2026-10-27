//! The original dialogue flow for the VR dialogue panel, driven only by the core's events.
//!
//! What the original (and the desktop host, `apps/game/src/play/events.rs` + `voice_flow`)
//! does: a `Talk` starts the dialogue; the NPC's (and the hero's) lines arrive as
//! `LineSpoken` and are *spoken one after the other*, each with its voice-over and subtitle;
//! only when the last line has finished does the info menu (`MenuChanged`) appear; picking an
//! option sends `Choose { index }`, which makes the core speak the hero's line, run the info
//! and produce the next menu. `DialogEnded` closes the panel (after pending lines).
//!
//! VR adds nothing to the rules: the subtitle hangs at the speaker's head (`Subtitle`), the voice
//! is a 3D sound at the speaker, the options float beside the NPC (`DialogPanel`), and a trigger
//! press during a line skips it (the original's confirm key).
use gothic_gameplay::dialog::Choice;
use gothic_runtime::{Command, Event};
use gothic_sim::types::ActorId;
use gothic_ui::dialogmenu::Subtitle;
use gothic_vr_ui::{DialogData, HudData};
use std::collections::VecDeque;

/// One spoken line.
#[derive(Debug, Clone, PartialEq)]
pub struct SpokenLine {
    pub speaker: String,
    pub text: String,
    /// The hero speaks (the subtitle hangs at the hero's head).
    pub player: bool,
    /// The NPC that speaks.
    pub actor: Option<ActorId>,
    /// Voice sample id (`SVM_<voice>_<key>` family / dialog line key) for 3D playback.
    pub voice: Option<String>,
}

/// What the host has to do next.
#[derive(Debug, Clone, PartialEq)]
pub enum FlowOut {
    /// Show this subtitle for `secs` at the speaker and play its voice (3D at `actor`).
    Say { line: SpokenLine, secs: f32 },
    /// The options are due.
    ShowOptions(Vec<String>),
    /// Hide the options (a choice was made).
    HideOptions,
    /// The dialogue is over: close the panel.
    Closed,
}

#[derive(Debug, Default)]
pub struct DialogFlow {
    pub npc: Option<ActorId>,
    queue: VecDeque<SpokenLine>,
    current: Option<(SpokenLine, f32)>,
    menu: Vec<Choice>,
    menu_due: bool,
    ended: bool,
    /// Options currently shown.
    pub options: Vec<String>,
    /// The line being spoken.
    pub speaking: Option<SpokenLine>,
}

/// Seconds a line stays up: the reading time of the flat UI's subtitle (`Subtitle::reading_secs`);
/// a voice-over that is longer keeps it (the host calls [`DialogFlow::finish_line`] when the
/// audio ends instead).
pub fn line_secs(text: &str) -> f32 {
    Subtitle::reading_secs(text)
}

impl DialogFlow {
    pub fn is_open(&self) -> bool {
        self.npc.is_some() || self.ended
    }

    pub fn on_started(&mut self, npc: ActorId) {
        *self = Self {
            npc: Some(npc),
            ..Self::default()
        };
    }

    pub fn on_line(&mut self, speaker: &str, key: &str, text: Option<&str>, player: bool, actor: Option<ActorId>) {
        let shown = text.map_or_else(|| format!("[{key}]"), String::from);
        self.queue.push_back(SpokenLine {
            speaker: speaker.to_string(),
            text: shown,
            player,
            actor,
            voice: (!key.is_empty()).then(|| key.to_string()),
        });
    }

    pub fn on_menu(&mut self, menu: &[Choice]) {
        self.menu = menu.to_vec();
        self.menu_due = true;
    }

    pub fn on_ended(&mut self) {
        self.ended = true;
    }

    /// Feed a core event (anything else is ignored).
    pub fn on_event(&mut self, e: &Event) {
        match e {
            Event::DialogStarted { npc, .. } => self.on_started(*npc),
            Event::LineSpoken { speaker, key, text, player, actor } => {
                self.on_line(speaker, key, text.as_deref(), *player, *actor)
            }
            Event::MenuChanged { menu, .. } => self.on_menu(menu),
            Event::DialogEnded { .. } => self.on_ended(),
            _ => {}
        }
    }

    /// The current line is over (audio ended or the player skipped it).
    pub fn finish_line(&mut self) {
        if let Some((_, t)) = self.current.as_mut() {
            *t = 0.0;
        }
    }

    /// Advance by `dt` seconds; returns what the host must do now.
    pub fn tick(&mut self, dt: f32) -> Vec<FlowOut> {
        let mut out = Vec::new();
        if let Some((_, t)) = self.current.as_mut() {
            *t -= dt;
            if *t <= 0.0 {
                self.current = None;
                self.speaking = None;
            }
        }
        if self.current.is_none()
            && let Some(line) = self.queue.pop_front()
        {
            let secs = line_secs(&line.text);
            self.speaking = Some(line.clone());
            self.current = Some((line.clone(), secs));
            out.push(FlowOut::Say { line, secs });
        }
        let idle = self.current.is_none() && self.queue.is_empty();
        if idle && self.menu_due {
            self.menu_due = false;
            self.options = self.menu.iter().map(|c| c.label.clone()).collect();
            out.push(FlowOut::ShowOptions(self.options.clone()));
        }
        if idle && self.ended {
            *self = Self::default();
            out.push(FlowOut::Closed);
        }
        out
    }

    /// The options were chosen: the command for the core. The options hide until the core answers
    /// with the next menu. `None` while lines are still being spoken or for a stale index.
    pub fn choose(&mut self, index: usize) -> Option<(Command, FlowOut)> {
        if self.options.get(index).is_none() || self.current.is_some() || !self.queue.is_empty() {
            return None;
        }
        self.options.clear();
        self.menu.clear();
        Some((Command::Choose { index }, FlowOut::HideOptions))
    }

    /// The command that leaves the dialogue (the original's escape).
    pub fn leave(&self) -> Command {
        Command::EndDialog
    }
}

/// Put a flow output into the HUD data: the subtitle at the speaker, the options beside the NPC.
/// `head_of` gives the world head position (metres) of an actor (`None` = the hero's head).
pub fn apply_to_hud(out: &FlowOut, data: &mut HudData, npc_head: glam::Vec3, head_of: &dyn Fn(Option<ActorId>) -> glam::Vec3) {
    match out {
        FlowOut::Say { line, secs: _ } => {
            data.speaker_head = Some(if line.player { head_of(None) } else { head_of(line.actor) });
            let measure = gothic_ui::dialogmenu::approx_text_width(&line.text, 14.0);
            data.subtitle.start(&line.text, data.comfort.subtitles, false, measure);
        }
        FlowOut::ShowOptions(options) => {
            data.dialog = Some(DialogData {
                options: options.clone(),
                npc_head,
            });
        }
        FlowOut::HideOptions => data.dialog = None,
        FlowOut::Closed => {
            data.dialog = None;
            data.subtitle.stop();
            data.speaker_head = None;
        }
    }
}
