//! VR controller input -> the game core's [`Command`]s.
//!
//! Physical interactions (grabbing, swinging, drawing a bow) are handled by the VR mechanic
//! plugins; this module maps the remaining *buttons* to the same logical commands the
//! keyboard host sends, so the classic game logic (jump, sneak, draw weapon, menus, ranged
//! mode) stays the single authority.
use crate::input::{Hand, VrButton, VrInput};
use gothic_runtime::Command;
use serde::{Deserialize, Serialize};

/// Logical action a button can be bound to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BoundAction {
    Jump,
    ToggleSneak,
    ToggleDrawn,
    ToggleMenu,
}

/// One binding: a button on a hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    pub hand: Hand,
    pub button: VrButton,
    pub action: BoundAction,
}

/// Button bindings plus the toggle state the commands need.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandMapper {
    pub bindings: Vec<Binding>,
    pub drawn: bool,
    pub menu_open: bool,
}

impl Default for CommandMapper {
    fn default() -> Self {
        let b = |hand, button, action| Binding {
            hand,
            button,
            action,
        };
        Self {
            bindings: vec![
                b(Hand::Right, VrButton::Primary, BoundAction::Jump),
                b(Hand::Right, VrButton::StickClick, BoundAction::ToggleSneak),
                b(Hand::Right, VrButton::Secondary, BoundAction::ToggleDrawn),
                b(Hand::Left, VrButton::Menu, BoundAction::ToggleMenu),
            ],
            drawn: false,
            menu_open: false,
        }
    }
}

impl CommandMapper {
    /// Commands triggered by this frame's button edges.
    pub fn commands(&mut self, input: &VrInput) -> Vec<Command> {
        let mut out = Vec::new();
        for b in &self.bindings {
            if !input.just_pressed(b.hand, b.button) {
                continue;
            }
            match b.action {
                BoundAction::Jump if !self.menu_open => out.push(Command::Jump),
                BoundAction::ToggleSneak if !self.menu_open => out.push(Command::ToggleSneak),
                BoundAction::ToggleDrawn if !self.menu_open => {
                    self.drawn = !self.drawn;
                    out.push(Command::SetDrawn(self.drawn));
                }
                BoundAction::ToggleMenu => {
                    self.menu_open = !self.menu_open;
                    out.push(Command::GuiOpen(self.menu_open));
                }
                _ => {}
            }
        }
        out
    }
}

/// Hero transform for the core: feet position in Gothic centimetres and the sim's yaw
/// convention (see `apps/game/src/play/mod.rs::bevy_to_sim_yaw`) from a Bevy-convention yaw.
pub fn hero_transform(feet_cm: glam::Vec3, bevy_yaw: f32) -> Command {
    Command::HeroTransform {
        pos: gothic_sim::world::Pos::new(feet_cm.x, feet_cm.y, feet_cm.z),
        yaw: (-bevy_yaw.cos()).atan2(-bevy_yaw.sin()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::XrFrame;

    fn press(f: impl Fn(&mut XrFrame)) -> XrFrame {
        let mut fr = XrFrame::default();
        f(&mut fr);
        fr
    }

    #[test]
    fn edges_map_to_commands_once() {
        let mut m = CommandMapper::default();
        let mut i = VrInput::new();
        let a = press(|f| f.hand_mut(Hand::Right).primary = true);
        i.push(a);
        assert!(matches!(m.commands(&i).as_slice(), [Command::Jump]));
        i.push(a);
        assert!(m.commands(&i).is_empty(), "held is not a new press");
    }

    #[test]
    fn toggles_alternate_and_menu_blocks_gameplay() {
        let mut m = CommandMapper::default();
        let mut i = VrInput::new();
        let b = press(|f| f.hand_mut(Hand::Right).secondary = true);
        for want in [true, false] {
            i.push(b);
            assert!(matches!(m.commands(&i).as_slice(), [Command::SetDrawn(d)] if *d == want));
            i.push(XrFrame::default());
        }
        i.push(press(|f| f.hand_mut(Hand::Left).menu = true));
        assert!(matches!(
            m.commands(&i).as_slice(),
            [Command::GuiOpen(true)]
        ));
        i.push(press(|f| f.hand_mut(Hand::Right).primary = true));
        assert!(
            m.commands(&i).is_empty(),
            "no jumping while the menu is open"
        );
    }

    #[test]
    fn hero_yaw_matches_the_host_convention() {
        let Command::HeroTransform { pos, yaw } =
            hero_transform(glam::Vec3::new(1.0, 2.0, 3.0), 0.0)
        else {
            panic!()
        };
        assert_eq!((pos.x, pos.y, pos.z), (1.0, 2.0, 3.0));
        // Bevy yaw 0 looks down -Z; the sim measures atan2(dz, dx) = atan2(-1, 0).
        assert!((yaw + std::f32::consts::FRAC_PI_2).abs() < 1e-6);
    }
}
