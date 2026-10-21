//! Item statistics tables (`gCHUDTableItemStats` and `gCHUDTable`, E33): the label / value rows
//! of the item tooltip of the inventory, trade and loot pages, with an optional comparison
//! against the item worn in the same slot.
//!
//! The original builds the rows from the stack's template: one damage row per damage type
//! (`HUD_Damage_<Type>`, the amount comes from the script `OnPlayerGetDamage`), attribute
//! bonuses, requirement rows (`HUD_Req_<Attribute>`, shown in the warning colour while the
//! player lacks the value), the gold value (`HUD_GoldValue`, from `OnPlayerGetGoldCost`) and,
//! on the trade pages, the count in the player's inventory (`HUD_In_Inventory`). The script
//! numbers are the template values here; the host can install better ones through
//! [`StatContext::damage`] / [`StatContext::price`].
use crate::items::ItemInfo;
use bevy::prelude::*;
use std::collections::HashMap;

/// How a row is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Normal,
    /// Requirement the player does not meet.
    Bad,
    Dim,
}

/// One row of the table.
#[derive(Clone, Debug, PartialEq)]
pub struct StatRow {
    pub label: String,
    pub value: String,
    pub tone: Tone,
    /// Difference to the compared item (positive = better); `None` without a comparison.
    pub delta: Option<i32>,
}

impl StatRow {
    fn new(label: String, value: impl ToString, tone: Tone) -> Self {
        Self {
            label,
            value: value.to_string(),
            tone,
            delta: None,
        }
    }
}

/// Items the hero currently wears, by category id (`1` weapon, `2` armor): the host keeps it up
/// to date; the tooltip compares a selected item against the one in the same slot.
#[derive(Resource, Clone, Debug, Default)]
pub struct EquippedItems(pub HashMap<i32, String>);

impl EquippedItems {
    pub fn for_category(&self, category: i32) -> Option<&str> {
        self.0.get(&category).map(String::as_str)
    }
}

/// `OnPlayerGetDamage` stand-in.
pub type DamageHook = dyn Fn(&ItemInfo) -> Option<u32>;
/// `OnPlayerGetGoldCost` stand-in.
pub type PriceHook = dyn Fn(&ItemInfo) -> Option<i64>;

/// What the table needs from the game: texts, the player's attribute values and (optionally)
/// the script numbers.
pub struct StatContext<'a> {
    /// Loc string by key (`None` when the table has no entry).
    pub text: &'a dyn Fn(&str) -> Option<String>,
    /// Localized attribute name by tag.
    pub attribute_name: &'a dyn Fn(&str) -> String,
    /// The player's current attribute value by lower-case tag.
    pub attribute: &'a dyn Fn(&str) -> i32,
    /// `OnPlayerGetDamage`: damage shown for an item, `None` keeps the template value.
    pub damage: Option<&'a DamageHook>,
    /// `OnPlayerGetGoldCost`: price shown, `None` keeps the template value.
    pub price: Option<&'a PriceHook>,
}

fn damage_label(ctx: &StatContext, ty: &str) -> String {
    (ctx.text)(&format!("HUD_Damage_{ty}")).unwrap_or_else(|| {
        match ty {
            "Blade" => "Klinge",
            "Impact" => "Zerschmettern",
            "Missile" => "Geschoss",
            "Fire" => "Feuer",
            "Ice" => "Eis",
            "Lightning" => "Energie",
            other => other,
        }
        .to_string()
    })
}

fn damage_of(ctx: &StatContext, info: &ItemInfo) -> Option<(u32, String)> {
    let (amount, ty) = info.damage.as_ref()?;
    let amount = ctx.damage.and_then(|f| f(info)).unwrap_or(*amount);
    Some((amount, ty.clone()))
}

fn price_of(ctx: &StatContext, info: &ItemInfo) -> i64 {
    ctx.price
        .and_then(|f| f(info))
        .unwrap_or(i64::from(info.value))
}

/// Port of gCHUDTableItemStats (Game:0x100a3bb0 ff.) row building: the table of `info`. With
/// `against` (the worn item) each damage / bonus / value row carries the difference.
/// `in_inventory` adds the "in inventory" row of the trade pages.
pub fn item_rows(
    ctx: &StatContext,
    info: &ItemInfo,
    against: Option<&ItemInfo>,
    in_inventory: Option<u32>,
) -> Vec<StatRow> {
    let mut rows = Vec::new();
    if let Some((amount, ty)) = damage_of(ctx, info) {
        let mut r = StatRow::new(damage_label(ctx, &ty), amount, Tone::Normal);
        if let Some(other) = against {
            // the worn item's damage of the same type (0 if it has none of it)
            let base = damage_of(ctx, other)
                .filter(|(_, t)| *t == ty)
                .map_or(0, |(a, _)| a);
            r.delta = Some(amount as i32 - base as i32);
        }
        rows.push(r);
    }
    for (tag, v) in &info.modifiers {
        let mut r = StatRow::new((ctx.attribute_name)(tag), format!("{v:+}"), Tone::Normal);
        if let Some(other) = against {
            let base = other
                .modifiers
                .iter()
                .find(|(t, _)| t == tag)
                .map_or(0, |(_, b)| *b);
            r.delta = Some(v - base);
        }
        rows.push(r);
    }
    for (tag, need) in &info.requirements {
        let have = (ctx.attribute)(&tag.to_ascii_lowercase());
        let label = (ctx.text)(&format!("HUD_Req_{tag}"))
            .unwrap_or_else(|| format!("Benötigt {}", (ctx.attribute_name)(tag)));
        rows.push(StatRow::new(
            label,
            need,
            if have >= *need {
                Tone::Normal
            } else {
                Tone::Bad
            },
        ));
    }
    let price = price_of(ctx, info);
    let mut r = StatRow::new(
        (ctx.text)("HUD_GoldValue").unwrap_or_else(|| "Wert".into()),
        price,
        Tone::Normal,
    );
    if let Some(other) = against {
        r.delta = Some((price - price_of(ctx, other)) as i32);
    }
    rows.push(r);
    if let Some(n) = in_inventory {
        rows.push(StatRow::new(
            (ctx.text)("HUD_In_Inventory").unwrap_or_else(|| "Im Inventar".into()),
            n,
            Tone::Dim,
        ));
    }
    rows
}

/// The delta cell: `+3` / `-2`, nothing for 0 or no comparison.
pub fn delta_text(delta: Option<i32>) -> String {
    match delta {
        Some(d) if d != 0 => format!("{d:+}"),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx<'a>(
        text: &'a dyn Fn(&str) -> Option<String>,
        name: &'a dyn Fn(&str) -> String,
        attr: &'a dyn Fn(&str) -> i32,
    ) -> StatContext<'a> {
        StatContext {
            text,
            attribute_name: name,
            attribute: attr,
            damage: None,
            price: None,
        }
    }

    fn sword(dmg: u32, value: u32) -> ItemInfo {
        ItemInfo {
            name: "It_Sword".into(),
            category: 1,
            value,
            damage: Some((dmg, "Blade".into())),
            requirements: vec![("STR".into(), 30)],
            modifiers: vec![("DEX".into(), 2)],
            ..ItemInfo::default()
        }
    }

    #[test]
    fn rows_follow_the_original_order() {
        let (t, n, a) = (|_: &str| None, |t: &str| t.to_string(), |_: &str| 40);
        let rows = item_rows(&ctx(&t, &n, &a), &sword(25, 300), None, Some(2));
        let labels: Vec<_> = rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(
            labels,
            ["Klinge", "DEX", "Benötigt STR", "Wert", "Im Inventar"]
        );
        assert_eq!(rows[0].value, "25");
        assert_eq!(rows[1].value, "+2");
        assert!(rows.iter().all(|r| r.delta.is_none()));
        assert_eq!(rows[4].tone, Tone::Dim);
    }

    #[test]
    fn unmet_requirements_are_flagged() {
        let (t, n, a) = (|_: &str| None, |t: &str| t.to_string(), |_: &str| 10);
        let rows = item_rows(&ctx(&t, &n, &a), &sword(25, 300), None, None);
        assert_eq!(rows[2].tone, Tone::Bad);
        let a = |_: &str| 30;
        let rows = item_rows(&ctx(&t, &n, &a), &sword(25, 300), None, None);
        assert_eq!(rows[2].tone, Tone::Normal, "equal meets the requirement");
    }

    #[test]
    fn loc_keys_win_over_the_fallbacks() {
        let t = |k: &str| (k == "HUD_Req_STR").then(|| "Stärke".to_string());
        let (n, a) = (|t: &str| t.to_string(), |_: &str| 0);
        let rows = item_rows(&ctx(&t, &n, &a), &sword(25, 300), None, None);
        assert_eq!(rows[2].label, "Stärke");
    }

    #[test]
    fn comparison_reports_differences() {
        let (t, n, a) = (|_: &str| None, |t: &str| t.to_string(), |_: &str| 99);
        let worn = sword(20, 500);
        let mut better = sword(26, 450);
        better.modifiers = vec![("DEX".into(), 5), ("STR".into(), 1)];
        let rows = item_rows(&ctx(&t, &n, &a), &better, Some(&worn), None);
        assert_eq!(rows[0].delta, Some(6));
        assert_eq!(rows[1].delta, Some(3));
        assert_eq!(rows[2].delta, Some(1), "a bonus the worn item lacks");
        let value = rows.iter().find(|r| r.label == "Wert").unwrap();
        assert_eq!(value.delta, Some(-50));
        assert_eq!(delta_text(Some(6)), "+6");
        assert_eq!(delta_text(Some(-50)), "-50");
        assert_eq!(delta_text(Some(0)), "");
        assert_eq!(delta_text(None), "");
    }

    #[test]
    fn script_numbers_replace_template_values() {
        let (t, n, a) = (|_: &str| None, |t: &str| t.to_string(), |_: &str| 99);
        let dmg = |i: &ItemInfo| Some(i.damage.as_ref().map_or(0, |d| d.0) * 2);
        let price = |_: &ItemInfo| Some(7_i64);
        let mut c = ctx(&t, &n, &a);
        c.damage = Some(&dmg);
        c.price = Some(&price);
        let rows = item_rows(&c, &sword(10, 300), None, None);
        assert_eq!(rows[0].value, "20");
        assert_eq!(rows.last().unwrap().value, "7");
    }

    #[test]
    fn equipped_lookup_by_category() {
        let mut e = EquippedItems::default();
        e.0.insert(1, "It_Sword".into());
        assert_eq!(e.for_category(1), Some("It_Sword"));
        assert_eq!(e.for_category(2), None);
    }
}
