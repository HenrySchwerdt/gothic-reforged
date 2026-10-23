//! Timed HUD text (`gCHUDTextEffect`, E33) and the centred screen output of the engine
//! (`gCScreenOutputCtrl`, E34): quest success / failure banners, the info-script "say" line and
//! script `PrintScreen` text.
//!
//! `gCHUDTextEffect` keeps a list of lines; each has a start time, a duration, a colour, a row
//! (drawn at `row * line height`) and flag bits: 1 right aligned, 2 centred, 4 fade in over the
//! window's fade time, 8 fade out over it, 0x10 shake. A line is dropped when its duration is
//! over. `gCScreenOutputCtrl::PrintScreen` replaces the whole text of one centred window, lays
//! the `\n` separated lines out under each other (a line may start with `[color=RRGGBBAA]`) and
//! hides the window when its timer (counted down by `Process`) runs out; a timer of 0 keeps it
//! until `PrintScreenEnd`.
use bevy::prelude::*;

/// Flag bits of a text effect line.
pub mod flags {
    pub const RIGHT: u32 = 1;
    pub const CENTER: u32 = 2;
    pub const FADE_IN: u32 = 4;
    pub const FADE_OUT: u32 = 8;
    pub const SHAKE: u32 = 0x10;
}

/// One line of the effect window.
#[derive(Clone, Debug, PartialEq)]
pub struct EffectLine {
    pub row: u32,
    pub flags: u32,
    pub color: Color,
    pub text: String,
    pub start: f32,
    pub duration: f32,
}

/// What to draw for a line this frame.
#[derive(Clone, Debug, PartialEq)]
pub struct EffectDraw {
    pub row: u32,
    pub text: String,
    pub color: Color,
    pub align: EffectAlign,
    /// Pixel offset of the shake (zero without the flag).
    pub shake: Vec2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectAlign {
    Left,
    Center,
    Right,
}

/// The effect window: floating level-up style texts.
#[derive(Resource, Clone, Debug)]
pub struct TextEffects {
    lines: Vec<EffectLine>,
    now: f32,
    /// Fade time of the window (`+0x20`), seconds.
    pub fade: f32,
    tick: u32,
}

impl Default for TextEffects {
    fn default() -> Self {
        Self {
            lines: Vec::new(),
            now: 0.0,
            fade: 0.5,
            tick: 0,
        }
    }
}

impl TextEffects {
    /// Pixel height of a row.
    pub const ROW_H: f32 = 26.0;
    /// Largest shake offset in pixels.
    pub const SHAKE_PX: f32 = 2.0;

    pub fn push(
        &mut self,
        row: u32,
        text: impl Into<String>,
        color: Color,
        flags: u32,
        duration: f32,
    ) {
        self.lines.push(EffectLine {
            row,
            flags,
            color,
            text: text.into(),
            start: self.now,
            duration,
        });
    }

    /// Advance the clock and drop finished lines (the loop of `vfn_44_100a77e0`).
    pub fn tick(&mut self, dt: f32) {
        self.now += dt;
        self.tick = self.tick.wrapping_add(1);
        let now = self.now;
        self.lines.retain(|l| now < l.start + l.duration);
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Fade factor of a line at `now`: in over `fade` after the start, out over `fade` before
    /// the end (the out fade wins where both apply).
    pub fn alpha(&self, l: &EffectLine) -> f32 {
        let end = l.start + l.duration;
        if l.flags & flags::FADE_OUT != 0 && self.now > end - self.fade {
            return (1.0 - (self.now - (end - self.fade)) / self.fade).clamp(0.0, 1.0);
        }
        if l.flags & flags::FADE_IN != 0 && self.now < l.start + self.fade {
            return ((self.now - l.start) / self.fade).clamp(0.0, 1.0);
        }
        1.0
    }

    /// The lines to draw this frame.
    pub fn draws(&self) -> Vec<EffectDraw> {
        self.lines
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let align = if l.flags & flags::RIGHT != 0 {
                    EffectAlign::Right
                } else if l.flags & flags::CENTER != 0 {
                    EffectAlign::Center
                } else {
                    EffectAlign::Left
                };
                let shake = if l.flags & flags::SHAKE != 0 {
                    shake_offset(self.tick, i as u32)
                } else {
                    Vec2::ZERO
                };
                EffectDraw {
                    row: l.row,
                    text: l.text.clone(),
                    color: l.color.with_alpha(l.color.alpha() * self.alpha(l)),
                    align,
                    shake,
                }
            })
            .collect()
    }
}

/// Deterministic jitter in `-SHAKE_PX..=SHAKE_PX` (the original asks its random generator).
fn shake_offset(tick: u32, salt: u32) -> Vec2 {
    let h = tick
        .wrapping_mul(2_654_435_761)
        .wrapping_add(salt.wrapping_mul(40_503));
    let f = |v: u32| ((v % 1000) as f32 / 999.0 * 2.0 - 1.0) * TextEffects::SHAKE_PX;
    Vec2::new(f(h >> 3), f(h.rotate_left(13) >> 5))
}

// ------------------------------------------------------------------------ screen output
/// A line of the screen output window.
#[derive(Clone, Debug, PartialEq)]
pub struct OutputLine {
    pub text: String,
    pub color: Color,
}

/// Parse `[color=RRGGBBAA]rest` at the start of a line (hex bytes, alpha last); other lines keep
/// `default`. A malformed tag is left in the text, as the original's `sscanf` leaves it.
pub fn parse_color_tag(line: &str, default: Color) -> (String, Color) {
    if let Some(rest) = line.strip_prefix("[color=")
        && let Some((hex, text)) = rest.split_once(']')
        && hex.len() == 8
        && let Ok(v) = u32::from_str_radix(hex, 16)
    {
        let [r, g, b, a] = v.to_be_bytes();
        return (text.to_string(), Color::srgba_u8(r, g, b, a));
    }
    (line.to_string(), default)
}

/// The window of `gCScreenOutputCtrl`.
#[derive(Resource, Clone, Debug, Default)]
pub struct ScreenOutput {
    lines: Vec<OutputLine>,
    /// Seconds left; 0 = until [`ScreenOutput::print_end`].
    timer: f32,
    visible: bool,
}

impl ScreenOutput {
    /// Port of gCScreenOutputCtrl::PrintScreen (Game:0x10077630): replace the text, show the
    /// window and start the timer.
    pub fn print(&mut self, text: &str, color: Color, seconds: f32) {
        gothic_portcov::hit!("print");
        self.lines = text
            .split('\n')
            .map(|l| {
                let (text, color) = parse_color_tag(l, color);
                OutputLine { text, color }
            })
            .collect();
        self.timer = seconds.max(0.0);
        self.visible = true;
    }

    /// Port of gCScreenOutputCtrl::PrintScreenEnd (Game:0x10077610).
    pub fn print_end(&mut self) {
        gothic_portcov::hit!("print_end");
        self.visible = false;
    }

    /// Port of gCScreenOutputCtrl::PrintScreen_Quest_Success (Game:0x10077c20): the quest
    /// title in white.
    pub fn quest_success(&mut self, title: &str, seconds: f32) {
        gothic_portcov::hit!("quest_success");
        self.print(title, Color::WHITE, seconds);
    }

    /// Port of gCScreenOutputCtrl::PrintScreen_Quest_Failed (Game:0x10077cc0).
    pub fn quest_failed(&mut self, title: &str, seconds: f32) {
        gothic_portcov::hit!("quest_failed");
        self.print(title, Color::WHITE, seconds);
    }

    /// Port of gCScreenOutputCtrl::PrintScreen_InfoScript_Say (Game:0x10077880): the spoken
    /// line, white for the player and yellow for everybody else.
    pub fn info_say(&mut self, speaker: &str, text: &str, by_player: bool, seconds: f32) {
        gothic_portcov::hit!("info_say");
        let color = if by_player {
            Color::WHITE
        } else {
            Color::srgb_u8(255, 255, 0)
        };
        self.print(&format!("{speaker}: {text}"), color, seconds);
    }

    /// Port of gCScreenOutputCtrl::Process (Game:0x10077f40): count the timer down and hide the
    /// window when it ends.
    pub fn tick(&mut self, dt: f32) {
        gothic_portcov::hit!("tick");
        if self.timer > 0.0 {
            self.timer -= dt;
            if self.timer <= 0.0 {
                self.visible = false;
            }
        }
    }

    pub fn lines(&self) -> &[OutputLine] {
        if self.visible { &self.lines } else { &[] }
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }
}

/// Host -> UI: show centred text (`PrintScreen`), `seconds` 0 keeps it up until [`PrintScreenEnd`].
#[derive(Message, Clone, Debug)]
pub struct PrintScreen {
    pub text: String,
    pub color: Color,
    pub seconds: f32,
}

/// Host -> UI: hide the centred text.
#[derive(Message, Clone, Copy, Debug)]
pub struct PrintScreenEnd;

/// Host -> UI: a floating text line (level up, XP gain).
#[derive(Message, Clone, Debug)]
pub struct FloatingText {
    pub row: u32,
    pub text: String,
    pub color: Color,
    pub flags: u32,
    pub seconds: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_live_their_duration() {
        let mut t = TextEffects::default();
        t.push(0, "Stufe 2", Color::WHITE, flags::CENTER, 2.0);
        t.tick(1.9);
        assert_eq!(t.draws().len(), 1);
        t.tick(0.2);
        assert!(t.is_empty());
    }

    #[test]
    fn fades_follow_the_window_fade_time() {
        let mut t = TextEffects {
            fade: 1.0,
            ..TextEffects::default()
        };
        t.push(0, "a", Color::WHITE, flags::FADE_IN | flags::FADE_OUT, 4.0);
        t.tick(0.5);
        assert!((t.draws()[0].color.alpha() - 0.5).abs() < 1e-5, "fading in");
        t.tick(1.5);
        assert_eq!(t.draws()[0].color.alpha(), 1.0, "fully visible in between");
        t.tick(1.5);
        assert!(
            (t.draws()[0].color.alpha() - 0.5).abs() < 1e-5,
            "fading out"
        );
    }

    #[test]
    fn alignment_and_shake_come_from_the_flags() {
        let mut t = TextEffects::default();
        t.push(1, "r", Color::WHITE, flags::RIGHT, 5.0);
        t.push(2, "c", Color::WHITE, flags::CENTER | flags::SHAKE, 5.0);
        t.tick(0.1);
        let d = t.draws();
        assert_eq!(d[0].align, EffectAlign::Right);
        assert_eq!(d[0].shake, Vec2::ZERO);
        assert_eq!(d[1].align, EffectAlign::Center);
        assert!(d[1].shake.abs().max_element() <= TextEffects::SHAKE_PX);
    }

    #[test]
    fn color_tags_are_parsed_per_line() {
        let (t, c) = parse_color_tag("[color=ff000080]Rot", Color::WHITE);
        assert_eq!(t, "Rot");
        assert_eq!(c, Color::srgba_u8(255, 0, 0, 128));
        let (t, c) = parse_color_tag("[color=zz]x", Color::BLACK);
        assert_eq!((t.as_str(), c), ("[color=zz]x", Color::BLACK));
        let (t, _) = parse_color_tag("plain", Color::BLACK);
        assert_eq!(t, "plain");
    }

    #[test]
    fn screen_output_shows_replaces_and_times_out() {
        let mut o = ScreenOutput::default();
        assert!(o.lines().is_empty());
        o.print("a\n[color=00ff00ff]b", Color::WHITE, 2.0);
        assert_eq!(o.lines().len(), 2);
        assert_eq!(o.lines()[1].color, Color::srgba_u8(0, 255, 0, 255));
        o.tick(1.0);
        assert!(o.is_visible());
        o.print("neu", Color::WHITE, 2.0);
        assert_eq!(o.lines().len(), 1, "a new text replaces the old");
        o.tick(2.1);
        assert!(!o.is_visible());
    }

    #[test]
    fn a_zero_timer_stays_until_print_end() {
        let mut o = ScreenOutput::default();
        o.print("bleibt", Color::WHITE, 0.0);
        o.tick(100.0);
        assert!(o.is_visible());
        o.print_end();
        assert!(o.lines().is_empty());
    }

    #[test]
    fn say_lines_are_yellow_unless_the_player_speaks() {
        let mut o = ScreenOutput::default();
        o.info_say("Diego", "Hallo", false, 3.0);
        assert_eq!(o.lines()[0].color, Color::srgb_u8(255, 255, 0));
        assert_eq!(o.lines()[0].text, "Diego: Hallo");
        o.info_say("Held", "Hi", true, 3.0);
        assert_eq!(o.lines()[0].color, Color::WHITE);
    }
}
