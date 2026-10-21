//! The player attribute store: `gCAttribute`, `gCStat` and the `gCPlayerMemory_PS` entry
//! points `ApplyMod`, `UnapplyMod` and `Consume` that equipment and consumables call with
//! `(tag, value, EAttribModOperation)`.
//!
//! An attribute (`STR`, `DEX`, `INT`, `THF`, `ALC`, the protections ...) is a base value plus a
//! temporary modifier; its maximum is its value. A pool (`HP`, `MP`, `SP`) is the same plus a
//! base maximum and a maximum modifier. Equipment applies *temporary* modifiers and takes them
//! back when it is removed; consumables apply *permanent* changes.
//!
//! `EAttribModOperation` values (raw item property): 0 add, 1 add to the maximum, 2 add a
//! percentage of the maximum to the value, 3 add a percentage to value and maximum, 4 set,
//! 5 .. 8 see [`Stat::apply_perm`]. Not ported: the engine's generic `gCStats` / `gCTrait` system
//! (a second, config-file driven attribute list that the shipped game does not use) and the
//! rounding ops 2, 3, 6, 7 of `gCAttribModDesc`, whose operand the decompiler dropped.
use std::collections::BTreeMap;

/// The pools; every other tag is a plain attribute.
pub fn is_pool(tag: &str) -> bool {
    matches!(tag.to_ascii_uppercase().as_str(), "HP" | "MP" | "SP")
}

/// One attribute or pool. For an attribute `base_max` and `max_modifier` are unused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stat {
    /// `BaseValue` (+0x14).
    pub base: i32,
    /// `Modifier` (+0x10): the temporary part of the value.
    pub modifier: i32,
    pub pool: bool,
    /// `BaseMaximum` (+0x18).
    pub base_max: i32,
    /// `MaximumModifier` (+0x1c).
    pub max_modifier: i32,
}

impl Stat {
    /// Port of gCAttribute::ApplyDefaults (Game:0x183850): base 100, no modifier.
    pub fn attribute(base: i32) -> Self {
        Self {
            base,
            ..Self::default()
        }
    }

    /// Port of gCStat::ApplyDefaults (Game:0x1845f0): value and maximum 100.
    pub fn pool(value: i32, max: i32) -> Self {
        Self {
            base: value,
            pool: true,
            base_max: max,
            ..Self::default()
        }
    }

    /// Port of gCAttribute::GetValue (Game:0x183880): base plus modifier.
    pub fn value(&self) -> i32 {
        self.base + self.modifier
    }

    /// Port of gCAttribute::GetMaximum / gCStat::GetMaximum (Game:0x183890 / 0x184630).
    pub fn maximum(&self) -> i32 {
        if self.pool {
            self.max_modifier + self.base_max
        } else {
            self.value()
        }
    }

    /// Port of gCAttribute::SetValue (Game:0x1838a0): the base follows so that the value
    /// is as asked, then the stat is capped.
    pub fn set_value(&mut self, v: i32) {
        self.base = v - self.modifier;
        self.cap();
    }

    /// Port of gCStat::SetMaximum (Game:0x184640).
    pub fn set_maximum(&mut self, m: i32) {
        if self.pool {
            self.base_max = m - self.max_modifier;
            self.cap();
        }
    }

    /// Port of gCAttribute::Cap / gCStat::Cap (Game:0x183a10 / 0x184970): a pool's maximum
    /// is at least 0; the value stays within `0..=maximum`.
    pub fn cap(&mut self) {
        if self.pool && self.maximum() < 0 {
            self.set_maximum(0);
        }
        let (max, val) = (self.maximum(), self.value());
        if max < val {
            self.set_value(max);
        } else if val < 0 {
            self.set_value(0);
        }
    }

    /// Port of gCAttribute::ApplyPerm / gCStat::ApplyPerm (Game:0x183980 / 0x184670), the
    /// permanent change of a consumed item. Only the add (0) and set (4) operations report
    /// success, as in the original.
    pub fn apply_perm(&mut self, op: i32, p: i32) -> bool {
        if !self.pool {
            return match op {
                0 => {
                    self.set_value(self.value() + p);
                    true
                }
                4 => {
                    self.set_value(p);
                    true
                }
                _ => false,
            };
        }
        match op {
            0 => {
                self.set_value(self.value() + p);
                true
            }
            1 => {
                self.set_maximum(self.maximum() + p);
                self.set_value(self.value() + p);
                false
            }
            2 => {
                let d = self.maximum() * p / 100;
                self.set_value(self.value() + d);
                false
            }
            3 => {
                let d = self.maximum() * p / 100;
                self.set_maximum(self.maximum() + d);
                self.set_value(self.value() + d);
                false
            }
            4 => {
                self.set_value(p);
                true
            }
            5 => {
                let m = self.maximum();
                self.set_maximum(m);
                if p - m > 0 {
                    self.set_value(self.value() + (p - m));
                }
                false
            }
            6 => {
                self.set_value(self.maximum() * p / 100);
                false
            }
            7 => {
                let target = self.maximum() * p / 100;
                let gain = target - self.maximum();
                self.set_maximum(target);
                if gain > 0 {
                    self.set_value(self.value() + gain);
                }
                false
            }
            8 => {
                self.set_value(self.maximum());
                false
            }
            _ => false,
        }
    }

    /// Port of gCAttribute::ApplyTemp / gCStat::ApplyTemp (Game:0x1839d0 / 0x184830), what
    /// worn equipment does: operation 0 raises the value modifier, operation 1 (pools only) the
    /// maximum modifier.
    pub fn apply_temp(&mut self, op: i32, p: i32) -> bool {
        match op {
            0 => {
                self.modifier += p;
                self.cap();
                true
            }
            1 if self.pool => {
                self.max_modifier += p;
                self.cap();
                false
            }
            _ => false,
        }
    }

    /// Port of gCAttribute::UnapplyTemp / gCStat::UnapplyTemp (Game:0x1839f0 / 0x1848d0).
    pub fn unapply_temp(&mut self, op: i32, p: i32) -> bool {
        match op {
            0 => {
                self.modifier -= p;
                self.cap();
                true
            }
            1 if self.pool => {
                self.max_modifier -= p;
                self.cap();
                false
            }
            _ => false,
        }
    }

    /// Port of gCAttrib::CanConsume (Game:0x186ea0): would the permanent change leave the
    /// value non-negative (checked before the cap)?
    pub fn can_consume(&self, op: i32, p: i32) -> bool {
        match op {
            0 => self.value() + p >= 0,
            4 => p >= 0,
            _ => true,
        }
    }
}

/// Port of gCPlayerMemory_PS::ApplyMod (Game:0x14efa0): equipment raises a temporary
/// modifier. A raised maximum of `HP` / `MP` takes the current value along in proportion
/// (`value + value * p / max`, rounded).
pub fn apply_mod(stat: &mut Stat, tag: &str, p: i32, op: i32) -> bool {
    let tag = tag.to_ascii_uppercase();
    if (tag == "HP" || tag == "MP") && op == 1 && stat.pool {
        let (v, m) = (f64::from(stat.value()), f64::from(stat.maximum()));
        let scaled = (v + 0.5 + v * f64::from(p) / m.max(1.0)) as i32;
        let r = stat.apply_temp(1, p);
        stat.set_value(scaled);
        return r;
    }
    stat.apply_temp(op, p)
}

/// Port of gCPlayerMemory_PS::UnapplyMod (Game:0x14f0a0): the opposite; the value shrinks
/// with the maximum (`value * (max - p) / max`, rounded).
pub fn unapply_mod(stat: &mut Stat, tag: &str, p: i32, op: i32) -> bool {
    let tag = tag.to_ascii_uppercase();
    if (tag == "HP" || tag == "MP") && op == 1 && stat.pool {
        let (v, m) = (f64::from(stat.value()), f64::from(stat.maximum()));
        let scaled = (v * (m - f64::from(p)) / m.max(1.0) + 0.5) as i32;
        stat.set_value(scaled);
    }
    stat.unapply_temp(op, p)
}

/// Port of gCPlayerMemory_PS::Consume (Game:0x14ed60): the permanent change.
pub fn consume(stat: &mut Stat, p: i32, op: i32) -> bool {
    stat.apply_perm(op, p)
}

/// The temporary parts of a character's stats by (upper-case) tag: the `Modifier` and the
/// pool `MaximumModifier` that worn equipment contributes. The base values stay where the
/// owner keeps them (`sim::Actor`); [`StatMods::apply`] works on a [`Stat`] built from both.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatMods {
    modifier: BTreeMap<String, i32>,
    max_modifier: BTreeMap<String, i32>,
}

impl StatMods {
    pub fn modifier(&self, tag: &str) -> i32 {
        self.modifier
            .get(&tag.to_ascii_uppercase())
            .copied()
            .unwrap_or(0)
    }

    pub fn max_modifier(&self, tag: &str) -> i32 {
        self.max_modifier
            .get(&tag.to_ascii_uppercase())
            .copied()
            .unwrap_or(0)
    }

    /// Fill `stat` (whose `base` / `base_max` the caller has set) with the stored modifiers.
    pub fn load(&self, tag: &str, stat: &mut Stat) {
        stat.modifier = self.modifier(tag);
        stat.max_modifier = self.max_modifier(tag);
    }

    /// Store the modifiers of `stat` back.
    pub fn store(&mut self, tag: &str, stat: &Stat) {
        let tag = tag.to_ascii_uppercase();
        for (map, v) in [
            (&mut self.modifier, stat.modifier),
            (&mut self.max_modifier, stat.max_modifier),
        ] {
            if v == 0 {
                map.remove(&tag);
            } else {
                map.insert(tag.clone(), v);
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.modifier.is_empty() && self.max_modifier.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attribute_value_is_base_plus_modifier_and_set_value_keeps_the_modifier() {
        let mut s = Stat::attribute(100);
        assert!(s.apply_temp(0, 8));
        assert_eq!((s.value(), s.maximum()), (108, 108));
        s.set_value(150);
        assert_eq!((s.base, s.modifier), (142, 8));
        assert!(s.unapply_temp(0, 8));
        assert_eq!(s.value(), 142);
        // temporary ops other than 0 do nothing on an attribute
        assert!(!s.apply_temp(1, 5));
        assert_eq!(s.value(), 142);
    }

    #[test]
    fn attribute_perm_ops_add_and_set_only() {
        let mut s = Stat::attribute(10);
        assert!(s.apply_perm(0, 5));
        assert_eq!(s.value(), 15);
        assert!(s.apply_perm(4, 40));
        assert_eq!(s.value(), 40);
        assert!(!s.apply_perm(2, 10));
        assert_eq!(s.value(), 40);
        // the value never drops below 0
        s.apply_perm(0, -100);
        assert_eq!(s.value(), 0);
    }

    #[test]
    fn health_potion_ops() {
        // 200 maximum, 80 now
        let mut hp = Stat::pool(80, 200);
        assert!(hp.apply_perm(0, 50)); // +50 flat
        assert_eq!(hp.value(), 130);
        hp.apply_perm(2, 30); // + 30 % of the maximum = 60
        assert_eq!(hp.value(), 190);
        hp.apply_perm(2, 30); // capped at the maximum
        assert_eq!(hp.value(), 200);
        let mut hp = Stat::pool(100, 200);
        hp.apply_perm(1, 13); // permanent: maximum and value +13
        assert_eq!((hp.value(), hp.maximum()), (113, 213));
        hp.apply_perm(3, 10); // + 10 % of 213 = 21 to both
        assert_eq!((hp.value(), hp.maximum()), (134, 234));
        hp.apply_perm(8, 0);
        assert_eq!(hp.value(), 234);
        hp.apply_perm(6, 50);
        assert_eq!(hp.value(), 117);
        assert!(hp.apply_perm(4, 1));
        assert_eq!(hp.value(), 1);
    }

    #[test]
    fn op7_raises_the_maximum_and_the_value_by_the_same_gain() {
        let mut hp = Stat::pool(100, 200);
        hp.apply_perm(7, 150); // maximum 300, gain 100
        assert_eq!((hp.value(), hp.maximum()), (200, 300));
        hp.apply_perm(7, 50); // shrinking only lowers the maximum (and caps)
        assert_eq!((hp.value(), hp.maximum()), (150, 150));
    }

    #[test]
    fn op5_adds_the_difference_to_the_maximum_to_the_value() {
        let mut hp = Stat::pool(50, 100);
        hp.apply_perm(5, 130); // 30 above the maximum: value + 30
        assert_eq!(hp.value(), 80);
        hp.apply_perm(5, 90); // below the maximum: nothing
        assert_eq!(hp.value(), 80);
    }

    #[test]
    fn worn_max_bonus_scales_the_value_and_comes_back_off() {
        // a ring of +50 maximum health on a hero at 100 of 200 hit points
        let mut hp = Stat::pool(100, 200);
        apply_mod(&mut hp, "HP", 50, 1);
        assert_eq!(hp.maximum(), 250);
        // 100 + 0.5 + 100 * 50 / 200 = 125.5 -> 125
        assert_eq!(hp.value(), 125);
        unapply_mod(&mut hp, "HP", 50, 1);
        assert_eq!(hp.maximum(), 200);
        // 125 * (250 - 50) / 250 + 0.5 = 100.5 -> 100
        assert_eq!(hp.value(), 100);
        assert_eq!(hp.max_modifier, 0);
    }

    #[test]
    fn stamina_has_no_proportional_scaling() {
        let mut sp = Stat::pool(100, 150);
        apply_mod(&mut sp, "SP", 20, 1);
        assert_eq!((sp.value(), sp.maximum()), (100, 170));
        unapply_mod(&mut sp, "SP", 20, 1);
        assert_eq!((sp.value(), sp.maximum()), (100, 150));
    }

    #[test]
    fn lowering_the_maximum_caps_the_value() {
        let mut mp = Stat::pool(150, 150);
        mp.apply_temp(1, 50);
        mp.set_value(200);
        assert_eq!(mp.value(), 200);
        mp.unapply_temp(1, 50);
        assert_eq!((mp.value(), mp.maximum()), (150, 150));
    }

    #[test]
    fn mods_round_trip_through_the_store() {
        let mut m = StatMods::default();
        let mut s = Stat::attribute(100);
        m.load("str", &mut s);
        apply_mod(&mut s, "STR", 8, 0);
        m.store("str", &s);
        assert_eq!(m.modifier("STR"), 8);
        let mut s2 = Stat::attribute(100);
        m.load("STR", &mut s2);
        assert_eq!(s2.value(), 108);
        unapply_mod(&mut s2, "STR", 8, 0);
        m.store("STR", &s2);
        assert!(m.is_empty());
    }

    #[test]
    fn can_consume_rejects_a_negative_result() {
        let hp = Stat::pool(10, 100);
        assert!(hp.can_consume(0, 5));
        assert!(!Stat::attribute(0).can_consume(0, -1));
    }
}
