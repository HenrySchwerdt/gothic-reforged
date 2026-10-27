//! The world-space *window*: one [`WindowView`] floating in front of the player at laser
//! distance, world-locked once it has appeared (Meta: do not anchor menus to a moving wrist,
//! keep panels upright and facing the user), driven by the pointing hand:
//!
//! * laser hover + trigger presses an item; a fingertip poke works when the panel is near;
//! * thumbstick flicks scroll the list under the laser (no scroll bars);
//! * `B/Y` (`Secondary`) of either hand closes; documents turn pages with a stick flick left
//!   / right.
//!
//! Pure logic: the host feeds [`WindowView`]s built from the core and applies the returned
//! [`WindowAction`]s with the same code as the desktop UI (`PageEvent` handling, `UseItem`).
use crate::comfort;
use crate::hud::{Tracked, fingertip};
use crate::panel::Panel;
use crate::sheet::{Sheet, SheetAct};
use crate::views::{self, WindowUi, WindowView};
use glam::Vec3;
use gothic_ui::PageEvent;
use gothic_vr::{Hand, Pose, VrButton, VrInput};

/// What a window asks of the host.
#[derive(Debug, Clone, PartialEq)]
pub enum WindowAction {
    /// A page event of the flat UI (`windows.rs` of the desktop host applies them).
    Page(PageEvent),
    /// `Command::UseItem { template }`.
    UseItem(String),
    /// The wait page: wait this many hours (the host fades, advances the clock).
    Wait(u32),
    /// A main-menu entry.
    Menu(crate::sheet::MenuEntry),
    /// A comfort option stepped (the host applies it with `VrComfortOptions::apply` and sets
    /// the page's view again).
    Option(crate::comfort_opts::OptId, i32),
    /// A save-page button on the selected slot (the host carries out the file work).
    File(gothic_ui::FileCommand),
    /// The player closed the window (the host sends `Command::GuiOpen(false)` /
    /// `EndInteraction` as the desktop does).
    Close,
}

#[derive(Debug, Clone)]
pub struct VrWindow {
    pub view: WindowView,
    pub ui: WindowUi,
    pub panel: Panel,
    pub sheet: Sheet,
    /// Item under the pointer.
    pub hovered: Option<usize>,
    /// The pointing hand's fingertip is touching (poke mode) instead of the laser.
    pub poking: bool,
    poking_prev: bool,
}

/// Place a window of `size_m` in front of the head, on the horizontal view direction, a little
/// below the eye line, facing the eyes and upright; world-locked from then on.
pub fn place(head: &Pose, distance: f32) -> Pose {
    let mut fwd = head.forward();
    fwd.y = 0.0;
    let fwd = fwd.normalize_or(Vec3::NEG_Z);
    let pos = head.position + fwd * distance - Vec3::Y * 0.12;
    Pose::new(pos, comfort::facing(pos, head.position))
}

impl VrWindow {
    pub fn open(view: WindowView, head: &Pose, cx: &views::Ctx) -> Self {
        let ui = WindowUi::default();
        let sheet = views::build(&view, &ui, cx);
        let pose = place(head, sheet.distance);
        let panel = Panel::new(pose, sheet.size_m());
        Self {
            view,
            ui,
            panel,
            sheet,
            hovered: None,
            poking: false,
            poking_prev: false,
        }
    }

    /// New contents (same window kind): selection and scroll stay, the layout is rebuilt.
    pub fn set_view(&mut self, view: WindowView, cx: &views::Ctx) {
        if std::mem::discriminant(&view) != std::mem::discriminant(&self.view) {
            self.ui = WindowUi::default();
        }
        self.view = view;
        self.rebuild(cx);
        self.panel.size = self.sheet.size_m();
    }

    pub fn rebuild(&mut self, cx: &views::Ctx) {
        self.sheet = views::build(&self.view, &self.ui, cx);
        // Remember clamped scroll positions so a shrinking list does not leave a gap.
        self.ui.scroll = self.sheet.regions.iter().map(|r| r.top).collect();
        if let Some(h) = self.hovered
            && h >= self.sheet.items.len()
        {
            self.hovered = None;
        }
    }

    fn scroll(&mut self, region: usize, rows: i32) {
        let Some(r) = self.sheet.regions.get(region) else {
            return;
        };
        let top = (r.top as i32 + rows).clamp(0, r.max_top() as i32) as usize;
        if self.ui.scroll.len() <= region {
            self.ui.scroll.resize(region + 1, 0);
        }
        self.ui.scroll[region] = top;
    }

    /// Apply a pressed item; `None` when the window handled it itself.
    fn press(&mut self, act: SheetAct) -> Option<WindowAction> {
        match act {
            SheetAct::Page(e) => Some(WindowAction::Page(e)),
            SheetAct::UseItem(t) => Some(WindowAction::UseItem(t)),
            SheetAct::Close => Some(WindowAction::Close),
            SheetAct::Radio(main) => {
                self.ui.main_only = main;
                None
            }
            SheetAct::Step(d) => {
                let max = match &self.view {
                    WindowView::Wait(w) => w.max_hours as i32,
                    _ => 1,
                };
                self.ui.selected = Some((self.ui.selected.unwrap_or(1) as i32 + d).clamp(1, max) as usize);
                None
            }
            SheetAct::WaitDo => Some(WindowAction::Wait(self.ui.selected.unwrap_or(1) as u32)),
            SheetAct::Menu(e) => Some(WindowAction::Menu(e)),
            SheetAct::Opt(id, d) => Some(WindowAction::Option(id, d)),
            SheetAct::File(op) => {
                let WindowView::Files(f) = &self.view else {
                    return None;
                };
                let row = self.ui.selected.and_then(|i| f.rows.get(i));
                match (op, f.save_mode, row) {
                    (crate::sheet::FileOp::Do, true, r) => Some(WindowAction::File(gothic_ui::FileCommand::Save {
                        key: r.map(|r| r.key.clone()),
                    })),
                    (crate::sheet::FileOp::Do, false, Some(r)) => {
                        Some(WindowAction::File(gothic_ui::FileCommand::Load { key: r.key.clone() }))
                    }
                    (crate::sheet::FileOp::Delete, _, Some(r)) => {
                        Some(WindowAction::File(gothic_ui::FileCommand::Delete { key: r.key.clone() }))
                    }
                    _ => None,
                }
            }
            SheetAct::Tab(t) => {
                self.ui.tab = t;
                self.ui.selected = None;
                self.ui.scroll.clear();
                None
            }
            SheetAct::Select(i) => {
                self.ui.selected = Some(i);
                if let Some(s) = self.ui.scroll.get_mut(1) {
                    *s = 0;
                }
                None
            }
            SheetAct::Turn(d) => {
                self.ui.page = (self.ui.page as i32 + d).max(0) as usize;
                None
            }
        }
    }

    /// One frame of input. Call [`VrWindow::rebuild`] after the host changed the view or this
    /// returned (the layout follows the window state).
    pub fn update(
        &mut self,
        input: &VrInput,
        t: &Tracked,
        main: Hand,
        cx: &views::Ctx,
    ) -> Vec<WindowAction> {
        let mut out = Vec::new();
        if input.just_pressed(main, VrButton::Secondary)
            || input.just_pressed(main.other(), VrButton::Secondary)
        {
            out.push(WindowAction::Close);
            return out;
        }
        let Some(p) = t.hands[main.index()] else {
            self.hovered = None;
            return out;
        };
        let tip = fingertip(&p);
        let touched = self.panel.touch(tip, 0.015);
        let uv = touched.or_else(|| self.panel.ray(p.position, p.forward()));
        self.poking = touched.is_some();
        let was_touching = std::mem::replace(&mut self.poking_prev, self.poking);
        self.hovered = uv.and_then(|uv| self.sheet.hit(uv));

        // Scroll the list under the pointer, flick the stick.
        let (y, y0) = (
            input.current.hand(main).thumbstick.y,
            input.previous.hand(main).thumbstick.y,
        );
        let (x, x0) = (
            input.current.hand(main).thumbstick.x,
            input.previous.hand(main).thumbstick.x,
        );
        let mut dirty = false;
        if y.abs() > 0.8 && y0.abs() <= 0.8 {
            let region = uv
                .and_then(|uv| self.sheet.region_at(uv))
                .or_else(|| (!self.sheet.regions.is_empty()).then_some(0));
            if let Some(r) = region {
                self.scroll(r, if y < 0.0 { 1 } else { -1 });
                dirty = true;
            }
        }
        if x.abs() > 0.8 && x0.abs() <= 0.8 && matches!(self.view, WindowView::Document(_)) {
            self.press(SheetAct::Turn(if x > 0.0 { 1 } else { -1 }));
            dirty = true;
        }

        let poke = touched.is_some() && !was_touching;
        let pull = touched.is_none() && input.just_pressed(main, VrButton::Trigger);
        let pressed = if poke || pull { self.hovered } else { None };
        if let Some(i) = pressed
            && let Some(act) = self.sheet.items[i].act.clone()
        {
            if let Some(a) = self.press(act) {
                out.push(a);
            }
            dirty = true;
        }
        if dirty {
            self.rebuild(cx);
            self.hovered = uv.and_then(|uv| self.sheet.hit(uv));
        }
        out
    }
}
