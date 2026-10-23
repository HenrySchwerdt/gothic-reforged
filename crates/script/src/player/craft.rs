//! Crafting from the HUD (B19): `OnPlayerSynthRecipe`, `OnPlayerFryStack`,
//! `OnPlayerTransformStack` and the recipe checks.
//!
//! A recipe turns ingredients into a result item at a work station (anvil, alchemy table,
//! cauldron). The engine's `PSInventory::Synthesize` does the bookkeeping; the script handler
//! only reports: success prints the item and amount, missing ingredients make the hero say
//! "MissingIngredients", a missing skill "NotEnoughLP" with the skill's name. [`synthesize`] does
//! both halves.

use crate::host::{EntityId, ScriptHost};
use crate::player::api::{PlayerEvent, Refusal};
use crate::speech::say;

/// Outcome of a synthesis attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Synth {
    /// The recipe was made.
    Crafted {
        /// Result template.
        template: String,
        /// Amount produced.
        amount: u32,
    },
    /// An ingredient is missing; nothing was consumed.
    MissingIngredients,
    /// The skill the recipe needs is missing (the perk name).
    MissingSkill(String),
    /// No such recipe.
    Unknown,
}

/// Port of `PSInventory::Synthesize` + `OnPlayerSynthRecipe` (S:6a9f0): make recipe `id`.
///
/// The perk the recipe requires is checked first [I], then every ingredient; when both are there
/// the ingredients are consumed and the result (`ResultAmount`, one when zero) is added. The
/// hero's voice line and the HUD event follow the outcome.
pub fn synthesize(host: &mut dyn ScriptHost, p: EntityId, id: i32) -> Synth {
    gothic_portcov::hit!("synthesize");
    let Some(recipe) = host.recipe(id) else {
        return Synth::Unknown;
    };
    if !recipe.required_perk.is_empty() && !host.is_skill_active(p, &recipe.required_perk) {
        say(host, p, "NotEnoughLP");
        host.emit(PlayerEvent::Refused(Refusal::NotEnoughLp(
            recipe.required_perk.clone(),
        )));
        return Synth::MissingSkill(recipe.required_perk);
    }
    if recipe
        .ingredients
        .iter()
        .any(|(template, n)| host.item_count(p, template) < *n)
    {
        say(host, p, "MissingIngredients");
        host.emit(PlayerEvent::Refused(Refusal::MissingIngredients));
        return Synth::MissingIngredients;
    }
    for (template, n) in &recipe.ingredients {
        host.remove_item(p, template, *n);
    }
    let amount = recipe.result_amount.max(1);
    host.add_item(p, &recipe.result, amount);
    host.emit(PlayerEvent::Crafted {
        template: recipe.result.clone(),
        amount,
    });
    Synth::Crafted {
        template: recipe.result,
        amount,
    }
}

/// What frying turns an item into (`OnPlayerFryStack`): `It_Meatbug` into `It_Meatbug_Fried`,
/// `It_Meat` into `It_Meat_Fried` [I: the source names are read from a comparison against a
/// stack-held string].
pub fn fried_form(template: &str) -> Option<&'static str> {
    match template {
        "It_Meatbug" => Some("It_Meatbug_Fried"),
        "It_Meat" => Some("It_Meat_Fried"),
        _ => None,
    }
}

/// Port of `OnPlayerFryStack` (S:6a740): fry `amount` pieces of raw meat. Items that cannot be
/// fried are left alone; returns the number fried.
pub fn fry_stack(host: &mut dyn ScriptHost, p: EntityId, template: &str, amount: u32) -> u32 {
    gothic_portcov::hit!("fry_stack");
    let Some(fried) = fried_form(template) else {
        return 0;
    };
    let n = amount.min(host.item_count(p, template));
    if n == 0 {
        return 0;
    }
    host.remove_item(p, template, n);
    host.add_item(p, fried, n);
    host.emit(PlayerEvent::Crafted {
        template: fried.to_owned(),
        amount: n,
    });
    n
}

/// Port of `OnPlayerTransformStack` (S:6a8d0): a transform request on the barbecue is frying
/// (the only transformation the hero's HUD offers); the stacks are rearranged afterwards by the
/// host.
pub fn transform_stack(host: &mut dyn ScriptHost, p: EntityId, template: &str, amount: u32) -> u32 {
    gothic_portcov::hit!("transform_stack");
    fry_stack(host, p, template, amount)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{EntityApi, NpcApi};
    use crate::player::api::{PlayerApi, Recipe, SkillCategory};
    use crate::testhost::TestWorld;

    fn world_with_recipe() -> (TestWorld, EntityId) {
        let mut w = TestWorld::new();
        let p = w.player();
        w.add_recipe(
            7,
            Recipe {
                category: SkillCategory::Alchemy,
                ingredients: vec![("It_Plant_Healer".into(), 2), ("It_Bottle".into(), 1)],
                result: "It_Potion_Health".into(),
                result_amount: 0,
                required_perk: "Perk_Alchemy_1".into(),
            },
        );
        (w, p)
    }

    #[test]
    fn missing_skill_comes_before_missing_ingredients() {
        let (mut w, p) = world_with_recipe();
        assert_eq!(
            synthesize(&mut w, p, 7),
            Synth::MissingSkill("Perk_Alchemy_1".into())
        );
        w.grant_skill(p, "Perk_Alchemy_1");
        assert_eq!(synthesize(&mut w, p, 7), Synth::MissingIngredients);
        assert_eq!(w.item_count(p, "It_Potion_Health"), 0);
    }

    #[test]
    fn crafting_consumes_ingredients_and_adds_the_result() {
        let (mut w, p) = world_with_recipe();
        w.grant_skill(p, "Perk_Alchemy_1");
        w.give_item(p, "It_Plant_Healer");
        w.give_item(p, "It_Plant_Healer");
        w.give_item(p, "It_Plant_Healer");
        w.give_item(p, "It_Bottle");
        assert_eq!(
            synthesize(&mut w, p, 7),
            Synth::Crafted {
                template: "It_Potion_Health".into(),
                amount: 1
            }
        );
        assert_eq!(w.item_count(p, "It_Plant_Healer"), 1);
        assert_eq!(w.item_count(p, "It_Bottle"), 0);
        assert_eq!(w.item_count(p, "It_Potion_Health"), 1);
        assert_eq!(synthesize(&mut w, p, 99), Synth::Unknown);
        assert!(
            w.events()
                .iter()
                .any(|e| matches!(e, PlayerEvent::Crafted { .. }))
        );
        let _ = w.ai_mode(p);
    }

    #[test]
    fn frying_turns_raw_meat_into_fried() {
        let mut w = TestWorld::new();
        let p = w.player();
        for _ in 0..3 {
            w.give_item(p, "It_Meat");
        }
        assert_eq!(fry_stack(&mut w, p, "It_Meat", 5), 3);
        assert_eq!(w.item_count(p, "It_Meat_Fried"), 3);
        assert_eq!(fry_stack(&mut w, p, "It_Apple", 1), 0);
        assert_eq!(transform_stack(&mut w, p, "It_Meatbug", 1), 0);
    }
}
