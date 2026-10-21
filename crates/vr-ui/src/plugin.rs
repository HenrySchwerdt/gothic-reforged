//! Bevy integration: runs [`VrHud`] on `VrInputRes` (world poses through `VrPlayer`), sends
//! [`HudActionMsg`]s, and draws the greybox HUD with gizmos (panels, bars, rows, slots,
//! markers). Text and icons are the renderer's job once the real UI art is wired in; the
//! gizmo view shows layout and state only.
use crate::backpack::{COLS, ROWS};
use crate::hud::{HudAction, HudData, Tracked, VrHud};
use crate::panel::Panel;
use crate::wrist::health_pulse;
use bevy::prelude::*;
use gothic_vr::{Hand, VrInputRes, VrPlayer, VrSet};

#[derive(Resource, Debug, Default, Clone)]
pub struct VrHudRes(pub VrHud);

#[derive(Resource, Debug, Default, Clone)]
pub struct HudDataRes(pub HudData);

#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct HudActionMsg(pub HudAction);

/// Add after `gothic_vr::VrPlugin`; `gizmos: false` for headless hosts that only need the
/// logic.
pub struct VrUiPlugin {
    pub gizmos: bool,
}

impl Default for VrUiPlugin {
    fn default() -> Self {
        Self { gizmos: true }
    }
}

impl Plugin for VrUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<VrHudRes>()
            .init_resource::<HudDataRes>()
            .add_message::<HudActionMsg>()
            .add_systems(
                Update,
                update_hud.after(VrSet::Locomotion).before(VrSet::Sync),
            );
        if self.gizmos {
            app.add_systems(Update, draw_hud.after(update_hud));
        }
    }
}

fn update_hud(
    time: Res<Time>,
    input: Res<VrInputRes>,
    player: Option<Res<VrPlayer>>,
    mut data: ResMut<HudDataRes>,
    mut hud: ResMut<VrHudRes>,
    mut out: MessageWriter<HudActionMsg>,
) {
    let input = &input.0;
    let world = |p: gothic_vr::Pose| player.as_ref().map_or(p, |pl| pl.world(&p));
    let tracked = Tracked {
        head: world(input.head()),
        hands: Hand::BOTH.map(|h| input.hand_pose(h).map(world)),
    };
    data.0.subtitle.tick(time.delta_secs());
    for a in hud.0.update(input, &tracked, &data.0) {
        out.write(HudActionMsg(a));
    }
}

fn iso(p: &Panel) -> Isometry3d {
    Isometry3d::new(p.pose.position, p.pose.rotation)
}

const FRAME: Color = Color::srgb(0.75, 0.7, 0.55);
const HOVER: Color = Color::srgb(1.0, 0.85, 0.2);
const DIM: Color = Color::srgb(0.35, 0.33, 0.3);

fn draw_hud(time: Res<Time>, hud: Res<VrHudRes>, data: Res<HudDataRes>, mut g: Gizmos) {
    let hud = &hud.0;
    let data = &data.0;
    if let Some(w) = &hud.wrist {
        g.rect(iso(w), w.size, FRAME);
        let f = data.vitals.fractions();
        let colors = [
            Color::srgb(0.85, 0.1, 0.1).with_alpha(health_pulse(f[0], time.elapsed_secs())),
            Color::srgb(0.2, 0.35, 0.95),
            Color::srgb(0.2, 0.8, 0.3),
        ];
        for (i, (frac, c)) in f.iter().zip(colors).enumerate() {
            let v = (i as f32 + 0.5) / 3.0;
            g.line(
                w.point(Vec2::new(0.05, v)),
                w.point(Vec2::new(0.05 + 0.9 * frac, v)),
                c,
            );
        }
    }
    if hud.wheel.open.is_some() {
        for i in 0..hud.wheel.slots {
            if let Some(p) = hud.wheel.slot_position(i) {
                let filled = data.quick_slots.get(i).is_some_and(Option::is_some);
                let c = if hud.wheel.hovered == Some(i) {
                    HOVER
                } else if filled {
                    FRAME
                } else {
                    DIM
                };
                g.circle(Isometry3d::new(p, hud.wheel.rotation()), 0.02, c);
            }
        }
    }
    if let Some(bp) = &hud.backpack {
        g.rect(iso(&bp.panel), bp.panel.size, FRAME);
        for i in 0..COLS * ROWS {
            let filled = bp.visible(&data.inventory).nth(i).is_some();
            let c = if filled { FRAME } else { DIM };
            g.circle(
                Isometry3d::new(bp.cell_center(i), bp.panel.pose.rotation),
                0.025,
                c,
            );
        }
    }
    if let Some(dp) = &hud.dialog {
        g.rect(iso(&dp.panel), dp.panel.size, FRAME);
        for (k, i) in dp.rows().enumerate() {
            let c = if dp.list.selected() == Some(i) {
                HOVER
            } else {
                FRAME
            };
            let half = dp.panel.pose.rotation * Vec3::X * dp.panel.size.x * 0.45;
            let m = dp.row_center(k);
            g.line(m - half, m + half, c);
        }
    }
    if let Some((book, pose)) = &hud.book {
        for (p, page) in crate::handheld::Book::panels(pose)
            .iter()
            .zip([book.spread, book.spread + 1])
        {
            g.rect(iso(p), p.size, FRAME);
            let n = book.page(page).len();
            for k in 0..n {
                let v = (k as f32 + 0.5) / crate::handheld::LINES_PER_PAGE as f32;
                g.line(p.point(Vec2::new(0.1, v)), p.point(Vec2::new(0.9, v)), DIM);
            }
        }
    }
    if let Some((map, panel)) = &hud.map {
        g.rect(iso(panel), panel.size, FRAME);
        for (_, uv) in &map.markers {
            g.circle(
                Isometry3d::new(panel.point(*uv), panel.pose.rotation),
                0.006,
                FRAME,
            );
        }
        if let Some(uv) = map.player {
            g.circle(
                Isometry3d::new(panel.point(uv), panel.pose.rotation),
                0.01,
                HOVER,
            );
        }
    }
    if let Some(s) = &hud.subtitle {
        let w = s.lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) as f32
            * s.text_height
            * 0.5;
        let h = s.lines.len() as f32 * s.text_height * 1.3;
        g.rect(
            Isometry3d::new(s.pose.position, s.pose.rotation),
            Vec2::new(w, h),
            Color::WHITE.with_alpha(s.alpha),
        );
    }
}
