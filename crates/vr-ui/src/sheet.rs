//! Sheets: the retained layout of the world-space *windows* of the game (trade, loot, work
//! station recipes, documents, quest log, character). A [`Sheet`] is a flat list of items
//! (labels, rows, cells, buttons, tabs, bars) in panel coordinates (`uv`, 0..1, top-left
//! origin) that the renderer draws and the pointer hit-tests; it carries no game logic. The
//! builders in [`crate::views`] turn the view models of `gothic-ui` into sheets.
use glam::Vec2;
use gothic_ui::PageEvent;
use gothic_ui::atlas::Pic;

/// Rectangle in panel coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn contains(&self, uv: Vec2) -> bool {
        uv.x >= self.x && uv.x <= self.x + self.w && uv.y >= self.y && uv.y <= self.y + self.h
    }

    pub fn center(&self) -> Vec2 {
        Vec2::new(self.x + self.w * 0.5, self.y + self.h * 0.5)
    }

    /// Row `i` of `n` equal rows stacked in this rectangle.
    pub fn row(&self, i: usize, n: usize) -> Rect {
        let h = self.h / n.max(1) as f32;
        Rect::new(self.x, self.y + h * i as f32, self.w, h)
    }

    /// Cell (`col`, `row`) of a `cols` x `rows` grid.
    pub fn cell(&self, col: usize, row: usize, cols: usize, rows: usize) -> Rect {
        let (w, h) = (self.w / cols.max(1) as f32, self.h / rows.max(1) as f32);
        Rect::new(self.x + w * col as f32, self.y + h * row as f32, w, h)
    }

    /// Shrunk by `m` on every side (relative units).
    pub fn inset(&self, m: f32) -> Rect {
        Rect::new(
            self.x + m,
            self.y + m,
            (self.w - 2.0 * m).max(0.0),
            (self.h - 2.0 * m).max(0.0),
        )
    }
}

/// What an item looks like.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ItemKind {
    /// Big heading.
    Title,
    /// Static text; `value` is right aligned.
    Label,
    /// Wrapped paragraph (documents, quest log).
    Paragraph,
    /// A list row (name left, `value` right, count).
    Row,
    /// An icon cell with a count (inventory style).
    Cell,
    Button,
    Tab,
    /// A bar filled to the fraction.
    Bar(f32),
    /// Horizontal rule.
    Rule,
    /// A picture of the `g3.igl` by name (map, time-of-day strip).
    Image(&'static str),
}

/// What pressing an item does. The window turns it into a [`crate::windows::WindowAction`]
/// for the host or handles it itself (tabs, row selection, scrolling).
#[derive(Debug, Clone, PartialEq)]
pub enum SheetAct {
    /// A page event of the flat UI (trade, loot, craft, close): the host applies it with the
    /// same code as on the desktop.
    Page(PageEvent),
    /// Use / equip / drink the inventory item with this template (`Command::UseItem`).
    UseItem(String),
    /// Switch tab `n` of the window.
    Tab(usize),
    /// Select list row `n` (recipes, quests).
    Select(usize),
    /// Turn to page `n` (documents).
    Turn(i32),
    Close,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SheetItem {
    pub rect: Rect,
    pub kind: ItemKind,
    pub text: String,
    pub value: String,
    /// Item template for the icon / name lookup.
    pub icon: String,
    pub count: i32,
    pub enabled: bool,
    pub selected: bool,
    pub act: Option<SheetAct>,
    /// Scroll region this item belongs to (index into [`Sheet::regions`]).
    pub region: Option<usize>,
    /// Text height in design pixels (original UI: 16 body, 22 heading).
    pub size: f32,
    /// Dark ink on parchment instead of light text on the dark panels.
    pub ink: bool,
}

impl SheetItem {
    pub fn new(rect: Rect, kind: ItemKind, text: impl Into<String>) -> Self {
        Self {
            rect,
            kind,
            text: text.into(),
            value: String::new(),
            icon: String::new(),
            count: 0,
            enabled: true,
            selected: false,
            act: None,
            region: None,
            size: 16.0,
            ink: false,
        }
    }

    pub fn ink(mut self) -> Self {
        self.ink = true;
        self
    }

    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    pub fn act(mut self, a: SheetAct) -> Self {
        self.act = Some(a);
        self
    }

    pub fn value(mut self, v: impl Into<String>) -> Self {
        self.value = v.into();
        self
    }

    pub fn icon(mut self, icon: impl Into<String>, count: i32) -> Self {
        self.icon = icon.into();
        self.count = count;
        self
    }

    pub fn enabled(mut self, on: bool) -> Self {
        self.enabled = on;
        self
    }

    pub fn selected(mut self, on: bool) -> Self {
        self.selected = on;
        self
    }

    pub fn interactive(&self) -> bool {
        self.enabled && self.act.is_some()
    }
}

/// A scrollable area: `total` rows of which `visible` show from `top`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Region {
    pub rect: Rect,
    pub total: usize,
    pub visible: usize,
    pub top: usize,
}

impl Region {
    pub fn max_top(&self) -> usize {
        self.total.saturating_sub(self.visible)
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Sheet {
    pub title: String,
    /// Layout space in pixels (the original UI's 1024x768 design space, cropped); item
    /// rectangles are `uv` of this.
    pub design: Vec2,
    /// Original view backgrounds, in design pixels.
    pub backdrops: Vec<(Pic, Rect)>,
    /// Where the hovered item's name and price are shown.
    pub info: Option<Rect>,
    /// Size in dmm (distance independent millimetres) at the intended distance.
    pub size_dmm: Vec2,
    /// Intended distance from the eyes, m.
    pub distance: f32,
    pub items: Vec<SheetItem>,
    pub regions: Vec<Region>,
}

impl Sheet {
    /// A sheet of `design` pixels shown `width_dmm` wide at `distance` metres.
    pub fn new(title: impl Into<String>, design: Vec2, width_dmm: f32, distance: f32) -> Self {
        Self {
            title: title.into(),
            design,
            size_dmm: Vec2::new(width_dmm, width_dmm * design.y / design.x),
            distance,
            ..Self::default()
        }
    }

    /// Rectangle of design pixels as panel coordinates.
    pub fn px(&self, x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::new(x / self.design.x, y / self.design.y, w / self.design.x, h / self.design.y)
    }

    pub fn backdrop(&mut self, pic: Pic, x: f32, y: f32, w: f32, h: f32) {
        let r = self.px(x, y, w, h);
        self.backdrops.push((pic, r));
    }

    /// Size of one design pixel in dmm.
    pub fn dmm_per_px(&self) -> f32 {
        self.size_dmm.x / self.design.x
    }

    pub fn push(&mut self, item: SheetItem) -> usize {
        self.items.push(item);
        self.items.len() - 1
    }

    /// Size in metres.
    pub fn size_m(&self) -> Vec2 {
        self.size_dmm * 0.001 * self.distance
    }

    /// The interactive item under `uv` (the last drawn wins).
    pub fn hit(&self, uv: Vec2) -> Option<usize> {
        self.items
            .iter()
            .enumerate()
            .rev()
            .find(|(_, i)| i.interactive() && i.rect.contains(uv))
            .map(|(k, _)| k)
    }

    /// The scroll region under `uv`.
    pub fn region_at(&self, uv: Vec2) -> Option<usize> {
        self.regions.iter().position(|r| r.rect.contains(uv))
    }

    /// Add a scroll region of `total` rows of `row_px` design pixels in `rect`; returns its
    /// index and the range of rows to emit (`top..top+visible`).
    pub fn region(
        &mut self,
        rect: Rect,
        total: usize,
        row_px: f32,
        top: usize,
    ) -> (usize, std::ops::Range<usize>) {
        let visible = ((rect.h * self.design.y / row_px).floor() as usize).max(1);
        let top = top.min(total.saturating_sub(visible));
        self.regions.push(Region {
            rect,
            total,
            visible,
            top,
        });
        (self.regions.len() - 1, top..(top + visible).min(total))
    }

    /// Smallest interactive item, in dmm (for the comfort tests).
    pub fn min_target_dmm(&self) -> f32 {
        self.items
            .iter()
            .filter(|i| i.interactive())
            .map(|i| (i.rect.w * self.size_dmm.x).min(i.rect.h * self.size_dmm.y))
            .fold(f32::INFINITY, f32::min)
    }
}
