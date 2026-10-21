//! The whole HUD with controller input: pages follow `GuiManager`, actions reach the host.
use bevy::prelude::*;
use gothic_ui::{PageId, PageRow, PlayerPanel, QuickSlot};
use gothic_vr::{Hand, PendingXrFrame, Pose, VrInput, VrPlugin, XrBackendKind, XrFrame};
use gothic_vr_ui::backpack::Backpack;
use gothic_vr_ui::hud::fingertip;
use gothic_vr_ui::*;

const RIGHT: Vec3 = Vec3::new(0.25, 1.3, -0.3);
const LEFT: Vec3 = Vec3::new(-0.25, 1.3, -0.3);

struct Rig {
    input: VrInput,
    frame: XrFrame,
    hud: VrHud,
    data: HudData,
}

impl Rig {
    fn new() -> Self {
        let mut frame = XrFrame::default();
        frame.hand_mut(Hand::Left).pose = Some(Pose::new(LEFT, Quat::IDENTITY));
        frame.hand_mut(Hand::Right).pose = Some(Pose::new(RIGHT, Quat::IDENTITY));
        let data = HudData {
            vitals: Vitals {
                health: (60.0, 100.0),
                mana: (10.0, 20.0),
                stamina: (100.0, 100.0),
            },
            quick_slots: vec![
                Some(QuickSlot {
                    item: "ItPo_Health".into(),
                    count: 3,
                }),
                None,
                Some(QuickSlot {
                    item: "ItFo_Bread".into(),
                    count: 1,
                }),
                None,
            ],
            inventory: PlayerPanel {
                title: String::new(),
                rows: (0..8)
                    .map(|i| PageRow {
                        item: format!("ItMi_Fake_{i}"),
                        count: 1,
                        key: 10 + i,
                        ..Default::default()
                    })
                    .collect(),
                gold: 12,
            },
            ..Default::default()
        };
        let mut r = Self {
            input: VrInput::new(),
            frame,
            hud: VrHud::default(),
            data,
        };
        r.step(|_| {});
        r
    }

    fn step(&mut self, f: impl FnOnce(&mut XrFrame)) -> Vec<HudAction> {
        f(&mut self.frame);
        self.input.push(self.frame);
        let t = Tracked::from_input(&self.input);
        self.hud.update(&self.input, &t, &self.data)
    }

    fn hand(&mut self, h: Hand, pos: Vec3, rot: Quat) -> Vec<HudAction> {
        self.step(|f| f.hand_mut(h).pose = Some(Pose::new(pos, rot)))
    }

    /// Put the right fingertip on `p` (identity grip).
    fn touch(&mut self, p: Vec3) -> Vec<HudAction> {
        let grip = Pose::new(Vec3::ZERO, Quat::IDENTITY);
        self.hand(Hand::Right, p - fingertip(&grip), Quat::IDENTITY)
    }

    /// Aim the right hand from its rest position at `p`.
    fn aim(&mut self, p: Vec3) -> Vec<HudAction> {
        let rot = Quat::from_rotation_arc(Vec3::NEG_Z, (p - RIGHT).normalize());
        self.hand(Hand::Right, RIGHT, rot)
    }
}

#[test]
fn backpack_over_the_shoulder_opens_the_inventory_page_and_a_touch_uses_an_item() {
    let mut r = Rig::new();
    let shoulder = Vec3::new(0.15, 1.65, 0.15);
    r.hand(Hand::Right, shoulder, Quat::IDENTITY);
    assert_eq!(
        r.step(|f| f.hand_mut(Hand::Right).grip = 1.0),
        [HudAction::Page(Some(PageId::Inventory))]
    );
    assert_eq!(r.hud.gui.page(), Some(PageId::Inventory));
    r.step(|f| f.hand_mut(Hand::Right).grip = 0.0);
    let bp: Backpack = r.hud.backpack.clone().expect("panel open");
    // The fingertip presses once on arrival, not every frame.
    assert_eq!(r.touch(bp.cell_center(3)), [HudAction::InventoryItem(13)]);
    assert!(r.touch(bp.cell_center(3)).is_empty());
    r.hand(Hand::Right, RIGHT, Quat::IDENTITY);
    assert_eq!(r.touch(bp.cell_center(3)), [HudAction::InventoryItem(13)]);
    // Laser + trigger works too.
    r.aim(bp.cell_center(5));
    assert_eq!(
        r.step(|f| f.hand_mut(Hand::Right).trigger = 1.0),
        [HudAction::InventoryItem(15)]
    );
    r.step(|f| f.hand_mut(Hand::Right).trigger = 0.0);
    // Over the shoulder again closes it.
    r.hand(Hand::Right, shoulder, Quat::IDENTITY);
    assert_eq!(
        r.step(|f| f.hand_mut(Hand::Right).grip = 1.0),
        [HudAction::Page(None)]
    );
    assert!(r.hud.backpack.is_none());
}

#[test]
fn dialog_page_follows_the_data_and_hides_the_vitals() {
    let mut r = Rig::new();
    // Wrist turned to the eyes: vitals on show without a page.
    let eye = Vec3::new(0.0, 1.7, 0.0);
    let towards = Quat::from_rotation_arc(Vec3::X, (eye - LEFT).normalize());
    r.hand(Hand::Left, LEFT, towards);
    assert!(r.hud.wrist.is_some());
    r.data.dialog = Some(DialogData {
        options: vec!["Who are you?".into(), "Goodbye.".into()],
        npc_head: Vec3::new(0.0, 1.65, -1.8),
    });
    assert_eq!(r.step(|_| {}), [HudAction::Page(Some(PageId::Dialog))]);
    assert!(r.hud.wrist.is_none(), "the dialog page hides the bars");
    let row = r.hud.dialog.as_ref().unwrap().row_center(1);
    r.aim(row);
    assert_eq!(
        r.step(|f| f.hand_mut(Hand::Right).trigger = 1.0),
        [HudAction::ChooseOption(1)]
    );
    r.step(|f| f.hand_mut(Hand::Right).trigger = 0.0);
    r.data.dialog = None;
    assert_eq!(r.step(|_| {}), [HudAction::Page(None)]);
    assert!(r.hud.wrist.is_some());
}

#[test]
fn dialog_scrolls_one_row_per_flick() {
    let mut r = Rig::new();
    r.data.dialog = Some(DialogData {
        options: (0..9).map(|i| format!("Option {i}")).collect(),
        npc_head: Vec3::new(0.0, 1.65, -1.8),
    });
    r.step(|_| {});
    for _ in 0..5 {
        r.step(|f| f.hand_mut(Hand::Right).thumbstick.y = -1.0);
    }
    assert_eq!(r.hud.dialog.as_ref().unwrap().rows(), 1..7);
    r.step(|f| f.hand_mut(Hand::Right).thumbstick.y = 0.0);
    r.step(|f| f.hand_mut(Hand::Right).thumbstick.y = -1.0);
    assert_eq!(r.hud.dialog.as_ref().unwrap().rows(), 2..8);
}

#[test]
fn quick_wheel_uses_filled_slots_only() {
    let mut r = Rig::new();
    r.step(|f| f.hand_mut(Hand::Left).primary = true);
    let w = r.hud.wheel.clone();
    assert_eq!(w.slots, 4);
    let at = |i: usize| w.slot_position(i).unwrap();
    r.hand(Hand::Right, at(2), Quat::IDENTITY);
    assert_eq!(r.hud.wheel.hovered, Some(2));
    assert_eq!(
        r.step(|f| f.hand_mut(Hand::Left).primary = false),
        [HudAction::UseQuickSlot(2)]
    );
    r.hand(Hand::Right, RIGHT, Quat::IDENTITY);
    r.step(|f| f.hand_mut(Hand::Left).primary = true);
    let w = r.hud.wheel.clone();
    r.hand(Hand::Right, w.slot_position(1).unwrap(), Quat::IDENTITY);
    assert!(
        r.step(|f| f.hand_mut(Hand::Left).primary = false)
            .is_empty(),
        "empty slot"
    );
}

#[test]
fn journal_page_puts_a_book_in_the_off_hand() {
    let mut r = Rig::new();
    r.data.quests = (0..3)
        .map(|i| gothic_ui::journal::QuestRow {
            name: format!("q{i}"),
            title: format!("Quest {i}"),
            updated: 0,
        })
        .collect();
    assert!(r.step(|_| {}).is_empty());
    assert!(r.hud.book.is_none());
    assert_eq!(
        r.hud.open_page(Some(PageId::Quests)),
        [HudAction::Page(Some(PageId::Quests))]
    );
    r.step(|_| {});
    let (book, pose) = r.hud.book.as_ref().expect("book");
    assert_eq!(book.lines.len(), 3);
    assert!(pose.position.distance(LEFT) < 0.3, "held in the left hand");
}

#[derive(Resource, Default)]
struct Got(Vec<HudAction>);

#[test]
fn plugin_sends_hud_actions() {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        VrPlugin {
            backend: XrBackendKind::External,
        },
        VrUiPlugin { gizmos: false },
    ))
    .init_resource::<Got>()
    .add_systems(
        Last,
        |mut got: ResMut<Got>, mut m: MessageReader<HudActionMsg>| {
            got.0.extend(m.read().map(|a| a.0));
        },
    );
    let mut frame = XrFrame::default();
    frame.hand_mut(Hand::Right).pose = Some(Pose::new(Vec3::new(0.15, 1.65, 0.15), Quat::IDENTITY));
    for g in [0.0, 1.0] {
        frame.hand_mut(Hand::Right).grip = g;
        app.world_mut().resource_mut::<PendingXrFrame>().0 = Some(frame);
        app.update();
    }
    assert_eq!(
        app.world().resource::<Got>().0,
        [HudAction::Page(Some(PageId::Inventory))]
    );
}
