//! `OnPlayerUseStack` (B19): what using an inventory stack does, by the item's use type.
//!
//! Weapons, shields, torches, armor and jewelry toggle between hand/body and bag; scrolls start
//! the casting task and potions the drinking one; food and drink are consumed; books and
//! letters open their window; the "modify" items (blessing and poison) open the matching window
//! when the hero has the spell or perk.

use crate::enums::UseType;
use crate::host::{EntityId, ScriptHost};
use crate::player::api::{PlayerEvent, Refusal, UiWindow};
use crate::routine::state::State;
use crate::speech::say;

/// What using a stack did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UseOutcome {
    /// The item went on or into a hand.
    Equipped,
    /// The item came off again.
    Unequipped,
    /// The item was used up.
    Consumed,
    /// The hero switched to a magic task (`PS_Magic` or `PS_MagicPotion`).
    Magic(State),
    /// A HUD window opened.
    Window(UiWindow),
    /// The item cannot be used (requirements, missing skill).
    Refused,
    /// Nothing happens for this use type.
    Ignored,
}

/// Does the item toggle between bag and body?
fn is_wearable(use_type: UseType) -> bool {
    matches!(
        use_type,
        UseType::OneH
            | UseType::TwoH
            | UseType::Arrow
            | UseType::Bow
            | UseType::CrossBow
            | UseType::Bolt
            | UseType::Shield
            | UseType::Staff
            | UseType::Torch
            | UseType::Halberd
            | UseType::Axe
            | UseType::Armor
            | UseType::Helmet
            | UseType::Amulet
            | UseType::Ring
            | UseType::Body
    )
}

/// Port of `OnPlayerUseStack` (S:69d60): use one item of the hero's inventory.
pub fn use_stack(host: &mut dyn ScriptHost, p: EntityId, template: &str) -> UseOutcome {
    if host.item_count(p, template) == 0 {
        return UseOutcome::Ignored;
    }
    let use_type = host.item_use_type(template);
    match use_type {
        u if is_wearable(u) => {
            if host.is_equipped(p, template) {
                host.unequip_item(p, template);
                UseOutcome::Unequipped
            } else if host.equip_item(p, template) {
                UseOutcome::Equipped
            } else {
                UseOutcome::Refused
            }
        }
        UseType::Cast => {
            host.set_task(p, State::PsMagic);
            UseOutcome::Magic(State::PsMagic)
        }
        UseType::Potion => {
            if host.consume_item(p, template) {
                UseOutcome::Consumed
            } else {
                UseOutcome::Refused
            }
        }
        UseType::Plant
        | UseType::Meat
        | UseType::Fruit
        | UseType::Bread
        | UseType::Bottle
        | UseType::Cup
        | UseType::Bowl
        | UseType::Smoke => {
            if host.consume_item(p, template) {
                UseOutcome::Consumed
            } else {
                UseOutcome::Refused
            }
        }
        UseType::Book => open(host, UiWindow::Book),
        UseType::Letter | UseType::Map => open(host, UiWindow::Letter),
        UseType::Modify => modify(host, p, template),
        _ => UseOutcome::Ignored,
    }
}

fn open(host: &mut dyn ScriptHost, window: UiWindow) -> UseOutcome {
    host.emit(PlayerEvent::Window(window));
    UseOutcome::Window(window)
}

/// The "modify" items: a blessing scroll or the learned spell opens the bless window, the poison
/// potion needs `Perk_PoisonBlade` (otherwise "NotEnoughLP" and the missing-skill message).
fn modify(host: &mut dyn ScriptHost, p: EntityId, template: &str) -> UseOutcome {
    match template {
        "It_Spell_BlessWeapon" | "It_Scroll_BlessWeapon" => open(host, UiWindow::Bless),
        "It_Potion_Poison" => {
            if host.is_skill_active(p, "Perk_PoisonBlade") {
                open(host, UiWindow::Poison)
            } else {
                say(host, p, "NotEnoughLP");
                host.emit(PlayerEvent::Refused(Refusal::MissingPerk(
                    "Perk_PoisonBlade",
                )));
                UseOutcome::Refused
            }
        }
        _ => UseOutcome::Ignored,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::EntityApi;
    use crate::testhost::TestWorld;

    #[test]
    fn weapons_toggle_and_food_is_consumed() {
        let mut w = TestWorld::new();
        let p = w.player();
        w.set_item_use_type("It_Sword", UseType::OneH);
        w.set_item_use_type("It_Apple", UseType::Fruit);
        w.give_item(p, "It_Sword");
        w.give_item(p, "It_Apple");
        assert_eq!(use_stack(&mut w, p, "It_Sword"), UseOutcome::Equipped);
        assert_eq!(use_stack(&mut w, p, "It_Sword"), UseOutcome::Unequipped);
        assert_eq!(use_stack(&mut w, p, "It_Apple"), UseOutcome::Consumed);
        assert_eq!(use_stack(&mut w, p, "It_Apple"), UseOutcome::Ignored);
    }

    #[test]
    fn poison_needs_the_perk() {
        let mut w = TestWorld::new();
        let p = w.player();
        w.set_item_use_type("It_Potion_Poison", UseType::Modify);
        w.give_item(p, "It_Potion_Poison");
        assert_eq!(
            use_stack(&mut w, p, "It_Potion_Poison"),
            UseOutcome::Refused
        );
        w.grant_skill(p, "Perk_PoisonBlade");
        assert_eq!(
            use_stack(&mut w, p, "It_Potion_Poison"),
            UseOutcome::Window(UiWindow::Poison)
        );
    }

    #[test]
    fn scrolls_start_the_magic_task() {
        let mut w = TestWorld::new();
        let p = w.player();
        w.set_item_use_type("It_Scroll_Fireball", UseType::Cast);
        w.give_item(p, "It_Scroll_Fireball");
        assert_eq!(
            use_stack(&mut w, p, "It_Scroll_Fireball"),
            UseOutcome::Magic(State::PsMagic)
        );
    }
}
