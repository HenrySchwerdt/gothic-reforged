//! Display order of stacks: the `gCInventoryStack::Compare*` family.
//!
//! The comparison functions return -1/0/1; they are used both by the inventory sort and by
//! the HUD lists. Text comparisons need the localised names, which the host supplies through
//! [`OrderContext`].
use super::item::Category;
use super::slot::{Slot, use_type};
use super::stack::Stack;
use std::cmp::Ordering;

/// Localised text and the data-driven comparisons that live in other property sets.
pub trait OrderContext {
    /// Localised focus name of a template (string key `FO_<Template>`).
    fn focus_name(&self, template: &str) -> String {
        template.to_string()
    }
    /// Localised description (string key `ITEMDESC_<Template>`), the tie breaker.
    fn item_desc(&self, template: &str) -> String {
        template.to_string()
    }
    /// `gCRecipe_PS::CompareTo` for two recipe templates.
    fn recipe_cmp(&self, _a: &str, _b: &str) -> i32 {
        0
    }
    /// `gCMagic_PS::CompareTo` for two spell templates.
    fn spell_cmp(&self, _a: &str, _b: &str) -> i32 {
        0
    }
}

/// Context with no localisation: names are compared as template names.
#[derive(Debug, Clone, Copy, Default)]
pub struct PlainOrder;
impl OrderContext for PlainOrder {}

/// Sort rank per use type (table at Game 0x102dde38; negative entries sort last). Weapons
/// (101..106) precede armor and shields (201..204), jewellery (301..303) and consumables
/// (401..410).
pub fn use_type_rank(u: u32) -> i32 {
    match u {
        2 => 102,
        3 => 101,
        4 => 104,
        5 => 103,
        6 => 106,
        7 => 105,
        9 => 202,
        10 => 201,
        11 => 203,
        12 => 204,
        13 => 301,
        14 => 302,
        15 => 303,
        16 => 401,
        17 => 410,
        18 => 403,
        19 => 404,
        20 => 405,
        21 => 407,
        22 => 408,
        23 => 406,
        54 => 409,
        112 => 203,
        114 => 201,
        _ => -1,
    }
}

fn sign(o: Ordering) -> i32 {
    match o {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

/// Port of gCInventoryStack::CompareType (Game:0x100c3050): ascending rank of the use type;
/// out-of-range use types count as `None`, negative ranks as "last".
pub fn compare_type(a: &Stack, b: &Stack) -> i32 {
    gothic_portcov::hit!("compare_type");
    let clamp = |u: u32| if u > use_type::MAX { 0 } else { u };
    let rank = |s: &Stack| {
        let r = use_type_rank(clamp(s.use_type()));
        if r < 0 { i32::MAX } else { r }
    };
    sign(rank(a).cmp(&rank(b)))
}

/// Port of gCInventoryStack::CompareDamage (Game:0x100c2df0): higher damage first.
pub fn compare_damage(a: &Stack, b: &Stack) -> i32 {
    gothic_portcov::hit!("compare_damage");
    sign(b.item().damage_amount.cmp(&a.item().damage_amount))
}

/// Port of gCInventoryStack::CompareProtection (Game:0x100c2f10): higher `PROT_BLADE` first.
pub fn compare_protection(a: &Stack, b: &Stack) -> i32 {
    gothic_portcov::hit!("compare_protection");
    let (x, y) = (
        a.item().mod_value("PROT_BLADE"),
        b.item().mod_value("PROT_BLADE"),
    );
    sign(y.cmp(&x))
}

/// Port of gCInventoryStack::CompareTemplate (Game:0x100c2a10): case-insensitive name.
pub fn compare_template(a: &Stack, b: &Stack) -> i32 {
    gothic_portcov::hit!("compare_template");
    sign(
        a.template
            .to_ascii_lowercase()
            .cmp(&b.template.to_ascii_lowercase()),
    )
}

/// Port of gCInventoryStack::CompareValue (Game:0x100c2b00): more valuable first.
pub fn compare_value(a: &Stack, b: &Stack) -> i32 {
    gothic_portcov::hit!("compare_value");
    sign(b.item().gold_value.cmp(&a.item().gold_value))
}

/// Port of gCInventoryStack::CompareQuality (Game:0x100c28d0): compare the quality bits
/// without the damaged flag; at equal bits an undamaged item sorts before a damaged one.
pub fn compare_quality(a: &Stack, b: &Stack) -> i32 {
    gothic_portcov::hit!("compare_quality");
    let key = |s: &Stack| {
        let q = s.combined_quality();
        (q & !super::stack::QUALITY_DAMAGED).wrapping_mul(4)
            | if q & super::stack::QUALITY_DAMAGED != 0 {
                2
            } else {
                4
            }
    };
    sign(key(b).cmp(&key(a)))
}

/// Port of gCInventoryStack::CompareQuickSlot (Game:0x100c2980): assigned quick slots first,
/// slot 0 counts as 10, unassigned last.
pub fn compare_quick_slot(a: &Stack, b: &Stack) -> i32 {
    gothic_portcov::hit!("compare_quick_slot");
    let key = |q: i32| match q {
        q if q < 0 => i32::MAX,
        0 => 10,
        q => q,
    };
    sign(key(a.quick_slot).cmp(&key(b.quick_slot)))
}

/// Port of gCInventoryStack::CompareSlot (Game:0x100c29d0): linked slot order, unlinked
/// stacks after slot 18.
pub fn compare_slot(a: &Stack, b: &Stack) -> i32 {
    gothic_portcov::hit!("compare_slot");
    let key = |s: &Stack| {
        if s.slot == Slot::None {
            0x13
        } else {
            s.slot as i32
        }
    };
    sign(key(a).cmp(&key(b)))
}

/// Port of gCInventoryStack::CompareFocusName (Game:0x100c2c00): localised focus name, then
/// the localised description.
pub fn compare_focus_name(a: &Stack, b: &Stack, cx: &dyn OrderContext) -> i32 {
    gothic_portcov::hit!("compare_focus_name");
    let o = cx.focus_name(&a.template).cmp(&cx.focus_name(&b.template));
    if o != Ordering::Equal {
        return sign(o);
    }
    sign(cx.item_desc(&a.template).cmp(&cx.item_desc(&b.template)))
}

/// Port of gCInventoryStack::CompareName (Game:0x100c2b70): plain template name.
pub fn compare_name(a: &Stack, b: &Stack) -> i32 {
    gothic_portcov::hit!("compare_name");
    sign(a.template.cmp(&b.template))
}

/// Port of gCInventoryStack::CompareWeapon (Game:0x100c25c0).
fn compare_weapon(a: &Stack, b: &Stack) -> i32 {
    gothic_portcov::hit!("compare_weapon");
    first_nonzero([
        compare_type(a, b),
        compare_damage(a, b),
        compare_template(a, b),
        compare_quality(a, b),
        compare_quick_slot(a, b),
    ])
}

/// Port of gCInventoryStack::CompareArmor (Game:0x100c2610).
fn compare_armor(a: &Stack, b: &Stack) -> i32 {
    gothic_portcov::hit!("compare_armor");
    first_nonzero([
        compare_type(a, b),
        compare_protection(a, b),
        compare_template(a, b),
        compare_slot(a, b),
        compare_value(a, b),
    ])
}

/// Port of gCInventoryStack::CompareDefault (Game:0x100c2880).
fn compare_default(a: &Stack, b: &Stack) -> i32 {
    gothic_portcov::hit!("compare_default");
    first_nonzero([
        compare_template(a, b),
        compare_slot(a, b),
        compare_value(a, b),
    ])
}

/// Port of gCInventoryStack::CompareSpell (Game:0x100c2780): by the spell template, then name.
fn compare_spell(a: &Stack, b: &Stack, cx: &dyn OrderContext) -> i32 {
    gothic_portcov::hit!("compare_spell");
    let (sa, sb) = (&a.item().spell, &b.item().spell);
    match (sa, sb) {
        (None, None) => 0,
        (None, Some(_)) => -1,
        (Some(_), None) => 1,
        (Some(x), Some(y)) => match cx.spell_cmp(x, y) {
            0 => compare_focus_name(a, b, cx),
            v => v,
        },
    }
}

/// Port of gCInventoryStack::CompareRecipe (Game:0x100c26f0): recipe order, then name.
fn compare_recipe(a: &Stack, b: &Stack, cx: &dyn OrderContext) -> i32 {
    gothic_portcov::hit!("compare_recipe");
    match cx.recipe_cmp(&a.template, &b.template) {
        0 => compare_focus_name(a, b, cx),
        v => v,
    }
}

fn first_nonzero<const N: usize>(v: [i32; N]) -> i32 {
    v.into_iter().find(|x| *x != 0).unwrap_or(0)
}

/// Port of gCInventoryStack::Compare (Game:0x100c2460): categories in enum order, inside a
/// category the category-specific chain (weapons, armor, artefacts, alchemy, documents,
/// spells); everything else by localised name.
pub fn compare(a: &Stack, b: &Stack, cx: &dyn OrderContext) -> i32 {
    gothic_portcov::hit!("compare");
    let (ca, cb) = (a.category(), b.category());
    if ca != cb {
        return sign((ca as u8).cmp(&(cb as u8)));
    }
    match ca {
        Category::Weapon => compare_weapon(a, b),
        Category::Armor => compare_armor(a, b),
        Category::Artefact => first_nonzero([
            compare_type(a, b),
            compare_spell(a, b, cx),
            compare_default(a, b),
        ]),
        Category::Alchemy => first_nonzero([
            compare_type(a, b),
            compare_spell(a, b, cx),
            compare_name(a, b),
        ]),
        Category::Written => first_nonzero([
            compare_type(a, b),
            compare_recipe(a, b, cx),
            compare_focus_name(a, b, cx),
        ]),
        Category::Spellbook => compare_spell(a, b, cx),
        Category::Skill => 0,
        _ => compare_focus_name(a, b, cx),
    }
}

/// Stable sort of a stack list by [`compare`].
pub fn sort_stacks(stacks: &mut [Stack], cx: &dyn OrderContext) {
    stacks.sort_by(|a, b| compare(a, b, cx).cmp(&0));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::item::ItemInfo;
    use std::sync::Arc;

    fn s(name: &str, cat: Category, ut: u32, f: impl FnOnce(&mut ItemInfo)) -> Stack {
        let mut i = ItemInfo::new(name, cat, ut);
        f(&mut i);
        Stack::new(name, Some(Arc::new(i)))
    }

    #[test]
    fn categories_sort_in_enum_order() {
        let w = s("a", Category::Weapon, 2, |_| {});
        let ar = s("b", Category::Armor, 10, |_| {});
        let p = s("c", Category::Alchemy, 16, |_| {});
        assert_eq!(compare(&w, &ar, &PlainOrder), -1);
        assert_eq!(compare(&p, &ar, &PlainOrder), 1);
    }

    #[test]
    fn weapons_two_hander_before_one_hander_then_damage_desc() {
        let two = s("t", Category::Weapon, 3, |i| i.damage_amount = 20);
        let one = s("o", Category::Weapon, 2, |i| i.damage_amount = 50);
        assert_eq!(compare_type(&two, &one), -1);
        assert_eq!(compare(&two, &one, &PlainOrder), -1);
        let strong = s("x", Category::Weapon, 2, |i| i.damage_amount = 80);
        assert_eq!(compare(&strong, &one, &PlainOrder), -1);
        assert_eq!(compare(&one, &strong, &PlainOrder), 1);
    }

    #[test]
    fn armor_by_blade_protection_desc() {
        let a = s("a", Category::Armor, 10, |i| {
            i.mods.push(crate::inventory::item::StatMod {
                tag: "PROT_BLADE".into(),
                op: 0,
                value: 10,
            })
        });
        let b = s("b", Category::Armor, 10, |_| {});
        assert_eq!(compare_protection(&a, &b), -1);
        assert_eq!(compare(&a, &b, &PlainOrder), -1);
    }

    #[test]
    fn quality_key_ignores_damage_flag_and_prefers_undamaged() {
        let mut a = s("a", Category::Weapon, 2, |_| {});
        let mut b = s("a", Category::Weapon, 2, |_| {});
        a.quality = 0;
        b.quality = super::super::stack::QUALITY_DAMAGED;
        assert_eq!(compare_quality(&a, &b), -1);
        assert_eq!(compare_quality(&b, &a), 1);
        a.quality = 0x20;
        b.quality = 0x40;
        assert_eq!(compare_quality(&a, &b), 1);
    }

    #[test]
    fn quick_slots_sort_assigned_first_with_zero_as_ten() {
        let mut a = s("a", Category::Misc, 0, |_| {});
        let mut b = s("b", Category::Misc, 0, |_| {});
        a.quick_slot = 0;
        b.quick_slot = 3;
        assert_eq!(compare_quick_slot(&a, &b), 1);
        b.quick_slot = -1;
        assert_eq!(compare_quick_slot(&a, &b), -1);
    }

    #[test]
    fn type_rank_table() {
        assert_eq!(use_type_rank(3), 101);
        assert_eq!(use_type_rank(16), 401);
        assert_eq!(use_type_rank(54), 409);
        assert_eq!(use_type_rank(8), -1);
        let fist = s("f", Category::Weapon, 8, |_| {});
        let one = s("o", Category::Weapon, 2, |_| {});
        assert_eq!(compare_type(&fist, &one), 1);
    }

    #[test]
    fn sort_is_stable_and_total_for_equal_stacks() {
        let mut v = vec![
            s("b", Category::Misc, 0, |_| {}),
            s("a", Category::Misc, 0, |_| {}),
        ];
        sort_stacks(&mut v, &PlainOrder);
        assert_eq!(v[0].template, "a");
    }
}
