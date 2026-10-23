//! Behaviour of the dialog page: the option list (selection, scrolling), the subtitle line
//! (single line centred, longer text left aligned, 200 ms fades) and the game-message list.
//!
//! Pure state, no Bevy types. Ported from `gCHUDPageDialog2` / `gCHUDListCtrlDialog` /
//! `gCGUIManager::StartSay` / `PrintGameMessage`; see ENGINE_PORT_PLAN section 7 (E28/E29).
use std::collections::VecDeque;

/// Option list of the dialog page (`gCHUDListCtrlDialog`): one row per choice the player
/// may pick, one selected row, a window of `visible` rows over the list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OptionList {
    len: usize,
    selected: usize,
    top: usize,
    visible: usize,
}

impl OptionList {
    pub fn new(visible: usize) -> Self {
        Self {
            visible: visible.max(1),
            ..Self::default()
        }
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn selected(&self) -> Option<usize> {
        (self.len > 0).then_some(self.selected)
    }
    /// First row of the visible window.
    pub fn top(&self) -> usize {
        self.top
    }
    pub fn visible_rows(&self) -> std::ops::Range<usize> {
        self.top..(self.top + self.visible.max(1)).min(self.len)
    }

    /// Replace the rows (after every dialog step): the selection starts on row 0.
    /// Port of gCHUDPageDialog2 refresh (Game:0x1009ec90) "select row 0 and focus the list".
    /// (The original's variant that prefers an "end" row is not applied: the row kinds it
    /// tests are not known to be the same as our `Choice` kinds.)
    pub fn reset(&mut self, len: usize) {
        gothic_portcov::hit!("reset");
        self.len = len;
        self.selected = 0;
        self.top = 0;
    }

    /// Move the selection by `delta` rows (clamped, no wrap) and scroll it into view.
    pub fn step(&mut self, delta: i32) {
        if self.len == 0 {
            return;
        }
        let max = self.len - 1;
        let to = (self.selected as i64 + i64::from(delta)).clamp(0, max as i64);
        self.select(to as usize);
    }

    /// Select a row (mouse hover or digit key); out of range is ignored.
    pub fn select(&mut self, i: usize) {
        if i >= self.len {
            return;
        }
        self.selected = i;
        if i < self.top {
            self.top = i;
        } else if i >= self.top + self.visible.max(1) {
            self.top = i + 1 - self.visible.max(1);
        }
    }

    /// Scroll the window by mouse wheel rows without moving the selection.
    pub fn scroll(&mut self, rows: i32) {
        let max_top = self.len.saturating_sub(self.visible.max(1));
        self.top = (self.top as i64 + i64::from(rows)).clamp(0, max_top as i64) as usize;
    }
}

/// How the subtitle text is laid out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubtitleAlign {
    /// The text fits on one line: centred.
    Center,
    /// Longer text: wrapped and left aligned.
    Left,
}

/// Fade time of the talk line (`StartSay` / `StopSay`).
pub const SUBTITLE_FADE_SECS: f32 = 0.2;
/// Width of the talk region (227,100)-(797,181) minus the frame padding.
pub const SUBTITLE_TEXT_WIDTH: f32 = 532.0;

/// Rough width of `text` in a bold serif face of `px` pixels. Only used to decide between
/// centred and left aligned layout; the real measurement belongs to the text renderer.
pub fn approx_text_width(text: &str, px: f32) -> f32 {
    text.chars().count() as f32 * px * 0.5
}

/// Port of gCHUDStaticTalk layout (`FUN_1009eed0`): measure the text as a single line, keep
/// it centred when it fits, otherwise left aligned.
pub fn subtitle_align(text_width: f32, region_width: f32) -> SubtitleAlign {
    gothic_portcov::hit!("subtitle_align");
    if text_width <= region_width {
        SubtitleAlign::Center
    } else {
        SubtitleAlign::Left
    }
}

/// The talk line: text, layout and fade state.
#[derive(Clone, Debug, Default)]
pub struct Subtitle {
    pub text: String,
    pub align: Option<SubtitleAlign>,
    /// 0..=1.
    pub alpha: f32,
    on: bool,
    /// Seconds left until the line is taken down by itself (0 = until `stop`).
    remaining: f32,
}

impl Subtitle {
    /// Reading time used when no voice-over drives `stop` (the original stops the line when
    /// the audio ends; lines without audio use this estimate).
    pub fn reading_secs(text: &str) -> f32 {
        (1.5 + 0.055 * text.chars().count() as f32).clamp(2.0, 12.0)
    }

    /// Port of gCGUIManager::StartSay (Game:0x1007c500): shown only when the subtitle option
    /// is on or `force` is set. Returns whether the line was taken.
    pub fn start(&mut self, text: &str, subtitles_on: bool, force: bool, measure: f32) -> bool {
        gothic_portcov::hit!("start");
        if !(subtitles_on || force) || text.is_empty() {
            return false;
        }
        self.text = text.to_string();
        self.align = Some(subtitle_align(measure, SUBTITLE_TEXT_WIDTH));
        self.on = true;
        self.remaining = Self::reading_secs(text);
        true
    }

    /// Port of gCGUIManager::StopSay (Game:0x1007c520): fade out.
    pub fn stop(&mut self) {
        gothic_portcov::hit!("stop");
        self.on = false;
    }

    pub fn tick(&mut self, dt: f32) {
        if self.on && self.remaining > 0.0 {
            self.remaining -= dt;
            if self.remaining <= 0.0 {
                self.on = false;
            }
        }
        let step = dt / SUBTITLE_FADE_SECS;
        self.alpha = if self.on {
            (self.alpha + step).min(1.0)
        } else {
            (self.alpha - step).max(0.0)
        };
        if !self.on && self.alpha == 0.0 {
            self.text.clear();
            self.align = None;
        }
    }

    pub fn visible(&self) -> bool {
        self.alpha > 0.0
    }
}

/// Message types of `gEGameMessageType` seen in the code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageKind {
    Info,
    SaveOk,
    Error,
}

/// One line of the game message area.
#[derive(Clone, Debug, PartialEq)]
pub struct GameMessage {
    pub text: String,
    pub kind: MessageKind,
    pub age: f32,
}

/// The "Game Messages" area (87,308)-(937,524): a FIFO whose lines live `TTL` seconds and
/// fade during the last second.
#[derive(Clone, Debug, Default)]
pub struct GameMessages {
    lines: VecDeque<GameMessage>,
    /// Engine setup "hide GUI": messages are dropped.
    pub hide_gui: bool,
}

impl GameMessages {
    /// Lifetime per message (the table value 10 in `PrintGameMessage`).
    pub const TTL: f32 = 10.0;
    /// Lines that fit the area (216 px, 18 px per line, minus one for spacing).
    pub const MAX_LINES: usize = 11;
    const FADE: f32 = 1.0;

    /// Port of gCGUIManager::PrintGameMessage (Game:0x1007c5c0).
    pub fn print(&mut self, text: impl Into<String>, kind: MessageKind) {
        gothic_portcov::hit!("print");
        if self.hide_gui {
            return;
        }
        if self.lines.len() >= Self::MAX_LINES {
            self.lines.pop_front();
        }
        self.lines.push_back(GameMessage {
            text: text.into(),
            kind,
            age: 0.0,
        });
    }

    pub fn tick(&mut self, dt: f32) {
        for l in &mut self.lines {
            l.age += dt;
        }
        while self.lines.front().is_some_and(|l| l.age >= Self::TTL) {
            self.lines.pop_front();
        }
    }

    pub fn lines(&self) -> impl Iterator<Item = &GameMessage> {
        self.lines.iter()
    }

    /// Opacity of a line: 1 until the last second, then fading out.
    pub fn alpha(m: &GameMessage) -> f32 {
        ((Self::TTL - m.age) / Self::FADE).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn option_list_clamps_and_scrolls_selection_into_view() {
        let mut l = OptionList::new(3);
        l.reset(8);
        assert_eq!(l.selected(), Some(0));
        l.step(-1);
        assert_eq!(l.selected(), Some(0));
        for _ in 0..4 {
            l.step(1);
        }
        assert_eq!(l.selected(), Some(4));
        assert_eq!(l.visible_rows(), 2..5);
        l.step(100);
        assert_eq!(l.selected(), Some(7));
        assert_eq!(l.top(), 5);
        l.step(-100);
        assert_eq!((l.selected(), l.top()), (Some(0), 0));
    }

    #[test]
    fn option_list_reset_restarts_on_row_zero_and_handles_empty() {
        let mut l = OptionList::new(2);
        l.reset(5);
        l.select(4);
        l.reset(3);
        assert_eq!((l.selected(), l.top()), (Some(0), 0));
        l.reset(0);
        assert_eq!(l.selected(), None);
        l.step(1);
        l.select(9);
        assert!(l.visible_rows().is_empty());
    }

    #[test]
    fn wheel_scroll_keeps_selection_and_clamps() {
        let mut l = OptionList::new(3);
        l.reset(5);
        l.scroll(10);
        assert_eq!(l.top(), 2);
        assert_eq!(l.selected(), Some(0));
        l.scroll(-10);
        assert_eq!(l.top(), 0);
    }

    #[test]
    fn short_subtitle_is_centred_long_one_left() {
        let short = "Willkommen.";
        let long = "x".repeat(200);
        assert_eq!(
            subtitle_align(approx_text_width(short, 14.0), SUBTITLE_TEXT_WIDTH),
            SubtitleAlign::Center
        );
        assert_eq!(
            subtitle_align(approx_text_width(&long, 14.0), SUBTITLE_TEXT_WIDTH),
            SubtitleAlign::Left
        );
    }

    #[test]
    fn subtitle_respects_option_and_force_and_fades() {
        let mut s = Subtitle::default();
        assert!(!s.start("Hallo", false, false, 10.0));
        assert!(!s.visible());
        assert!(s.start("Hallo", false, true, 10.0));
        s.tick(0.1);
        assert!((s.alpha - 0.5).abs() < 1e-5);
        s.tick(0.1);
        assert_eq!(s.alpha, 1.0);
        s.stop();
        s.tick(0.2);
        assert!(!s.visible());
        assert!(s.text.is_empty());
    }

    #[test]
    fn subtitle_times_out_without_voice_over() {
        let mut s = Subtitle::default();
        s.start("Kurz", true, false, 10.0);
        for _ in 0..15 {
            s.tick(0.1);
        }
        assert!(s.visible());
        for _ in 0..30 {
            s.tick(0.1);
        }
        assert!(!s.visible());
    }

    #[test]
    fn game_messages_fifo_ttl_and_hide() {
        let mut m = GameMessages::default();
        for i in 0..(GameMessages::MAX_LINES + 2) {
            m.print(format!("m{i}"), MessageKind::Info);
        }
        assert_eq!(m.lines().count(), GameMessages::MAX_LINES);
        assert_eq!(m.lines().next().unwrap().text, "m2");
        m.tick(9.5);
        let a = GameMessages::alpha(m.lines().next().unwrap());
        assert!((a - 0.5).abs() < 1e-5);
        m.tick(0.6);
        assert_eq!(m.lines().count(), 0);
        m.hide_gui = true;
        m.print("x", MessageKind::Error);
        assert_eq!(m.lines().count(), 0);
    }
}
