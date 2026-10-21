//! The HUD message log (`gCHUDLog`, E33) and its settings (`Ini/log.ini`, read by
//! `gCGUIManager::ImportLogSettings`).
//!
//! The log is the "Log" static of the main page, `(15,219)-(884,471)`: a short scrolling list of
//! coloured lines (system, battle). A new line scrolls in from below at `ScrollLinesPerSecond`
//! (faster while several wait), the list is capped at `MaxLines`, and lines fade out once they
//! are older than `DecayDelaySeconds` (never below `MinLines` while those are still young).
//! Categories can be switched off individually; the shipped file leaves everything but system
//! errors off.
use bevy::prelude::*;
use std::collections::VecDeque;

/// Log categories of `log.ini`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LogCategory {
    SystemInfo,
    SystemWarning,
    SystemError,
    Battle0,
    Battle1,
    Battle2,
}

impl LogCategory {
    pub const ALL: [LogCategory; 6] = [
        LogCategory::SystemInfo,
        LogCategory::SystemWarning,
        LogCategory::SystemError,
        LogCategory::Battle0,
        LogCategory::Battle1,
        LogCategory::Battle2,
    ];

    /// The key prefix in `log.ini` (`System.Info`, `Battle.L0`, ...).
    pub fn ini_prefix(self) -> &'static str {
        match self {
            LogCategory::SystemInfo => "System.Info",
            LogCategory::SystemWarning => "System.Warning",
            LogCategory::SystemError => "System.Error",
            LogCategory::Battle0 => "Battle.L0",
            LogCategory::Battle1 => "Battle.L1",
            LogCategory::Battle2 => "Battle.L2",
        }
    }

    fn index(self) -> usize {
        self as usize
    }

    fn default_color(self) -> [f32; 4] {
        match self {
            LogCategory::SystemInfo | LogCategory::Battle0 => [1.0, 1.0, 1.0, 1.0],
            LogCategory::SystemWarning | LogCategory::Battle1 => [1.0, 1.0, 0.2, 1.0],
            LogCategory::SystemError | LogCategory::Battle2 => [1.0, 0.2, 0.2, 1.0],
        }
    }
}

/// `[Log]` of `log.ini`.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct LogSettings {
    /// Master switch (`Enabled`).
    pub enabled: bool,
    pub category_enabled: [bool; 6],
    pub category_color: [[f32; 4]; 6],
    pub max_lines: usize,
    pub min_lines: usize,
    pub decay_delay: f32,
    pub scroll_lines_per_second: f32,
}

impl Default for LogSettings {
    /// The values of the shipped file.
    fn default() -> Self {
        let mut enabled = [false; 6];
        enabled[LogCategory::SystemError.index()] = true;
        Self {
            enabled: false,
            category_enabled: enabled,
            category_color: LogCategory::ALL.map(LogCategory::default_color),
            max_lines: 10,
            min_lines: 0,
            decay_delay: 3.0,
            scroll_lines_per_second: 2.0,
        }
    }
}

impl LogSettings {
    /// Port of gCGUIManager::ImportLogSettings (Game:0x1007cf10): read the `[Log]` keys.
    pub fn from_ini(get: impl Fn(&str) -> Option<String>) -> Self {
        let mut s = Self::default();
        let flag = |k: &str| get(k).map(|v| v.trim().eq_ignore_ascii_case("true"));
        let num = |k: &str| get(k).and_then(|v| v.trim().parse::<f32>().ok());
        if let Some(v) = flag("Enabled") {
            s.enabled = v;
        }
        for c in LogCategory::ALL {
            let p = c.ini_prefix();
            if let Some(v) = flag(&format!("{p}.Enabled")) {
                s.category_enabled[c.index()] = v;
            }
            for (i, ch) in ["R", "G", "B", "A"].into_iter().enumerate() {
                if let Some(v) = num(&format!("{p}.Color.{ch}")) {
                    s.category_color[c.index()][i] = v.clamp(0.0, 1.0);
                }
            }
        }
        if let Some(v) = num("MaxLines") {
            s.max_lines = v.max(1.0) as usize;
        }
        if let Some(v) = num("MinLines") {
            s.min_lines = v.max(0.0) as usize;
        }
        if let Some(v) = num("DecayDelaySeconds") {
            s.decay_delay = v.max(0.0);
        }
        if let Some(v) = num("ScrollLinesPerSecond") {
            s.scroll_lines_per_second = v.max(0.1);
        }
        s
    }

    pub fn color(&self, c: LogCategory) -> Color {
        let [r, g, b, a] = self.category_color[c.index()];
        Color::srgba(r, g, b, a)
    }

    /// Whether a line of this category is logged at all.
    pub fn accepts(&self, c: LogCategory) -> bool {
        self.enabled && self.category_enabled[c.index()]
    }
}

/// One line of the log.
#[derive(Clone, Debug, PartialEq)]
pub struct LogLine {
    pub text: String,
    pub category: LogCategory,
    pub age: f32,
}

/// Fade-out time once a line is past the decay delay.
const FADE: f32 = 1.0;

/// The scrolling log list.
#[derive(Resource, Clone, Debug, Default)]
pub struct HudLog {
    lines: VecDeque<LogLine>,
    /// Lines that still have to scroll in.
    waiting: u32,
    /// Scroll progress of the waiting lines in pixels.
    offset: f32,
}

impl HudLog {
    /// Pixel height of one log line.
    pub const LINE_H: f32 = 18.0;

    /// Add a line if the category is enabled. Returns whether it was logged.
    pub fn push(
        &mut self,
        settings: &LogSettings,
        category: LogCategory,
        text: impl Into<String>,
    ) -> bool {
        if !settings.accepts(category) {
            return false;
        }
        self.lines.push_back(LogLine {
            text: text.into(),
            category,
            age: 0.0,
        });
        self.waiting += 1;
        while self.lines.len() > settings.max_lines {
            self.lines.pop_front();
            self.waiting = self.waiting.min(self.lines.len() as u32);
        }
        true
    }

    /// Advance the scroll and the decay.
    pub fn tick(&mut self, settings: &LogSettings, dt: f32) {
        // waiting lines scroll in faster the more of them wait
        if self.waiting > 0 {
            self.offset +=
                Self::LINE_H * settings.scroll_lines_per_second * dt * self.waiting as f32;
            while self.offset >= Self::LINE_H && self.waiting > 0 {
                self.waiting -= 1;
                self.offset -= Self::LINE_H;
            }
            if self.waiting == 0 {
                self.offset = 0.0;
            }
        }
        for l in &mut self.lines {
            l.age += dt;
        }
        let gone = settings.decay_delay + FADE;
        while self.lines.len() > settings.min_lines
            && self.lines.front().is_some_and(|l| l.age >= gone)
        {
            self.lines.pop_front();
            self.waiting = self.waiting.min(self.lines.len() as u32);
        }
    }

    /// Opacity of a line: 1 until the decay delay, then fading out.
    pub fn alpha(settings: &LogSettings, l: &LogLine) -> f32 {
        (1.0 - (l.age - settings.decay_delay) / FADE).clamp(0.0, 1.0)
    }

    /// The lines on show, oldest first, and the upward shift in pixels that animates the line
    /// scrolling in (the waiting lines are not drawn yet; the one being scrolled in is, shifted
    /// down by the part of its height still missing).
    pub fn visible(&self, rows: usize) -> (Vec<&LogLine>, f32) {
        let settled = self.lines.len().saturating_sub(self.waiting as usize);
        let drawn = if self.waiting > 0 {
            settled + 1
        } else {
            settled
        }
        .min(self.lines.len());
        let start = drawn.saturating_sub(rows + usize::from(self.waiting > 0));
        let shown: Vec<&LogLine> = self.lines.range(start..drawn).collect();
        let shift = if self.waiting > 0 {
            Self::LINE_H - self.offset
        } else {
            0.0
        };
        (shown, shift)
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::read_section;

    const SHIPPED: &str = "[Log]\nEnabled=false\nSystem.Error.Enabled=true\nSystem.Error.Color.G=0.200000\nBattle.L1.Enabled=true\nBattle.L1.Color.B=0.5\nMaxLines=10\nMinLines=1\nDecayDelaySeconds=3.000000\nScrollLinesPerSecond=2.000000\n";

    fn on() -> LogSettings {
        LogSettings {
            enabled: true,
            category_enabled: [true; 6],
            ..LogSettings::default()
        }
    }

    #[test]
    fn the_shipped_file_reads_and_switches_categories() {
        let s = LogSettings::from_ini(read_section(SHIPPED, "Log"));
        assert!(!s.enabled && s.min_lines == 1 && s.max_lines == 10);
        assert!(!s.accepts(LogCategory::SystemError), "master switch is off");
        let mut s = s;
        s.enabled = true;
        assert!(s.accepts(LogCategory::SystemError) && s.accepts(LogCategory::Battle1));
        assert!(!s.accepts(LogCategory::SystemInfo));
        assert_eq!(s.category_color[LogCategory::Battle1.index()][2], 0.5);
    }

    #[test]
    fn disabled_categories_log_nothing() {
        let mut log = HudLog::default();
        assert!(!log.push(&LogSettings::default(), LogCategory::SystemError, "x"));
        assert!(log.is_empty());
        assert!(log.push(&on(), LogCategory::SystemError, "x"));
    }

    #[test]
    fn new_lines_scroll_in_and_the_list_is_capped() {
        let s = on();
        let mut log = HudLog::default();
        log.push(&s, LogCategory::SystemInfo, "a");
        let (v, shift) = log.visible(10);
        assert_eq!(v.len(), 1);
        assert_eq!(shift, HudLog::LINE_H, "fully below at first");
        log.tick(&s, 0.25);
        let (_, shift) = log.visible(10);
        assert!((shift - HudLog::LINE_H * 0.5).abs() < 1e-3);
        log.tick(&s, 1.0);
        assert_eq!(log.visible(10).1, 0.0);
        for i in 0..20 {
            log.push(&s, LogCategory::SystemInfo, format!("l{i}"));
        }
        assert_eq!(log.len(), 10);
    }

    #[test]
    fn old_lines_fade_and_leave_but_min_lines_stay() {
        let mut s = on();
        s.min_lines = 1;
        let mut log = HudLog::default();
        log.push(&s, LogCategory::SystemInfo, "a");
        log.push(&s, LogCategory::SystemInfo, "b");
        log.tick(&s, 2.0);
        assert_eq!(HudLog::alpha(&s, &log.lines[0]), 1.0);
        log.tick(&s, 2.5);
        assert!(HudLog::alpha(&s, &log.lines[0]) < 1.0);
        log.tick(&s, 1.0);
        assert_eq!(log.len(), 1, "one line is kept");
    }
}
