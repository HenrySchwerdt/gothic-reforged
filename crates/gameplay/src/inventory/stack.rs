//! One inventory stack: `gCInventoryStack` on top of `gCInventorySlot`.
use super::item::{Category, ItemInfo};
use super::slot::{Slot, equip_slots, use_type};
use std::sync::{Arc, OnceLock};

/// `gEStackType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum StackType {
    #[default]
    Normal = 0,
    Treasure = 1,
    /// Items a merchant offers for sale; excluded from the "does the owner have it" counts.
    Merchandise = 2,
}

impl StackType {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => StackType::Treasure,
            2 => StackType::Merchandise,
            _ => StackType::Normal,
        }
    }
}

/// Quality flag: the item is damaged (halves its modifiers).
pub const QUALITY_DAMAGED: u32 = 0x100;

fn unknown_item() -> &'static ItemInfo {
    static UNKNOWN: OnceLock<ItemInfo> = OnceLock::new();
    UNKNOWN.get_or_init(ItemInfo::default)
}

/// A stack of identical items. `slot` is the `gESlot` it is linked to (the original links an
/// item entity; here the host mirrors `Linked`/`Unlinked` events into entities).
#[derive(Debug, Clone)]
pub struct Stack {
    /// Template name as given when the stack was created.
    pub template: String,
    /// Resolved template data; `None` for templates the catalog does not know.
    pub info: Option<Arc<ItemInfo>>,
    /// Linked slot, `Slot::None` when the stack only sits in the bag.
    pub slot: Slot,
    /// `Amount`.
    pub amount: i32,
    /// `Quality` flag bits of this stack.
    pub quality: u32,
    /// `QuickSlot`; negative means unassigned.
    pub quick_slot: i32,
    pub sort_index: i32,
    pub kind: StackType,
    /// `Learned` (skills, spells and recipes).
    pub learned: bool,
    /// `ActivationCount` (skills that equipment keeps active).
    pub activation_count: i32,
    /// `TransactionCount`: amount marked for the running trade.
    pub transaction: i32,
    /// `OnApplyStats` ran for this link and has not been undone yet.
    pub stats_applied: bool,
}

impl PartialEq for Stack {
    /// Stacks are equal when their saved state is; resolved template data is ignored.
    fn eq(&self, o: &Stack) -> bool {
        self.template == o.template
            && self.slot == o.slot
            && self.amount == o.amount
            && self.quality == o.quality
            && self.quick_slot == o.quick_slot
            && self.sort_index == o.sort_index
            && self.kind == o.kind
            && self.learned == o.learned
            && self.activation_count == o.activation_count
            && self.transaction == o.transaction
            && self.stats_applied == o.stats_applied
    }
}
impl Eq for Stack {}

impl Stack {
    /// Port of gCInventoryStack::ApplyDefaults (Game:0x100c0330): amount 1, quality 0, no
    /// quick slot, nothing learned or activated, empty transaction.
    pub fn new(template: &str, info: Option<Arc<ItemInfo>>) -> Self {
        gothic_portcov::hit!("new");
        Self {
            template: template.to_string(),
            info,
            slot: Slot::None,
            amount: 1,
            quality: 0,
            quick_slot: -1,
            sort_index: 0,
            kind: StackType::Normal,
            learned: false,
            activation_count: 0,
            transaction: 0,
            stats_applied: false,
        }
    }

    /// Item data, a blank placeholder for unknown templates.
    pub fn item(&self) -> &ItemInfo {
        match self.info.as_deref() {
            Some(i) => i,
            None => unknown_item(),
        }
    }

    /// Port of gCInventorySlot::GetCategory.
    pub fn category(&self) -> Category {
        gothic_portcov::hit!("category");
        self.item().category
    }

    /// Port of gCInventorySlot::GetUseType.
    pub fn use_type(&self) -> u32 {
        gothic_portcov::hit!("use_type");
        self.item().use_type
    }

    /// Port of gCInventorySlot::GetEquipSlots (member form): primary and alternative slot.
    pub fn equip_slots(&self) -> (Slot, Slot) {
        gothic_portcov::hit!("equip_slots");
        equip_slots(self.use_type())
    }

    /// Port of gCInventorySlot::IsPermanent (Game:0x100bcfd0): spells and skills, or items
    /// flagged `Permanent`.
    pub fn is_permanent(&self) -> bool {
        gothic_portcov::hit!("is_permanent");
        matches!(self.category(), Category::Spellbook | Category::Skill) || self.item().permanent
    }

    /// Port of gCInventorySlot::IsEquipment (Game:0x100bd040): weapons, armor and artefacts
    /// except letters.
    pub fn is_equipment(&self) -> bool {
        gothic_portcov::hit!("is_equipment");
        match self.category() {
            Category::Weapon | Category::Armor => true,
            Category::Artefact => self.use_type() != use_type::LETTER,
            _ => false,
        }
    }

    /// Port of gCInventorySlot::IsConsumable (Game:0x100bd0b0): the alchemy category.
    pub fn is_consumable(&self) -> bool {
        gothic_portcov::hit!("is_consumable");
        self.category() == Category::Alchemy
    }

    /// Port of gCInventorySlot::IsLinked.
    pub fn is_linked(&self) -> bool {
        gothic_portcov::hit!("is_linked");
        self.slot != Slot::None
    }

    /// Port of gCInventoryStack::IsEquippedInPrimary (Game:0x100c0440).
    pub fn is_equipped_in_primary(&self) -> bool {
        gothic_portcov::hit!("is_equipped_in_primary");
        self.is_linked() && self.equip_slots().0 == self.slot
    }

    /// Port of gCInventoryStack::IsEquippedInAlternative (Game:0x100c0490).
    pub fn is_equipped_in_alternative(&self) -> bool {
        gothic_portcov::hit!("is_equipped_in_alternative");
        self.is_linked() && self.equip_slots().1 == self.slot
    }

    /// Port of gCInventoryStack::IsEquipped (Game:0x100c03e0): linked to its primary or
    /// alternative carry slot. A weapon that is currently drawn sits in a hand slot and is
    /// not "equipped" by this test; see [`Stack::is_in_use`].
    pub fn is_equipped(&self) -> bool {
        gothic_portcov::hit!("is_equipped");
        self.is_equipped_in_primary() || self.is_equipped_in_alternative()
    }

    /// Linked to any slot, including the hands (what UIs mark as "worn").
    pub fn is_in_use(&self) -> bool {
        self.is_linked()
    }

    /// Port of gCInventoryStack::IsEquippedToSlot (Game:0x100c03c0).
    pub fn is_equipped_to_slot(&self, slot: Slot) -> bool {
        gothic_portcov::hit!("is_equipped_to_slot");
        self.slot != Slot::None && slot == self.slot
    }

    /// Port of gCInventoryStack::IsDisposable (Game:0x100c04e0): a stack may only disappear
    /// when it is not linked.
    pub fn is_disposable(&self) -> bool {
        gothic_portcov::hit!("is_disposable");
        !self.is_linked()
    }

    /// Port of gCInventoryStack::MustSplitBeforeLinking (Game:0x100c0550): a stack of an
    /// equippable item has to be split before one piece is linked; ammunition only when the
    /// target is the right hand.
    pub fn must_split_before_linking(&self, slot: Slot) -> bool {
        gothic_portcov::hit!("must_split_before_linking");
        let primary = self.equip_slots().0;
        if primary == Slot::Ammo {
            slot == Slot::RightHand
        } else {
            primary != Slot::None
        }
    }

    /// Port of gCInventoryStack::IsMergable (Game:0x100c0530): unassigned quick slot
    /// and not linked.
    pub fn is_mergeable(&self) -> bool {
        gothic_portcov::hit!("is_mergeable");
        self.quick_slot < 0 && !self.is_linked()
    }

    /// Port of gCInventoryStack::CanMerge (Game:0x100c0590): same template, quality and
    /// stack type, both without quick slot and both unlinked.
    pub fn can_merge(&self, other: &Stack) -> bool {
        gothic_portcov::hit!("can_merge");
        self.quick_slot < 0
            && !self.is_linked()
            && other.quick_slot < 0
            && !other.is_linked()
            && self.template.eq_ignore_ascii_case(&other.template)
            && self.quality == other.quality
            && self.kind == other.kind
    }

    /// Port of gCInventoryStack::Merge (Game:0x100c0950): move all of `other` into `self`.
    pub fn merge(&mut self, other: &mut Stack) -> bool {
        gothic_portcov::hit!("merge");
        if !self.can_merge(other) {
            return false;
        }
        self.amount += other.amount;
        other.amount = 0;
        true
    }

    /// Port of gCInventoryStack::Split (Game:0x100c09b0): cut `n` items off into a new
    /// unlinked stack. `None` unless `0 < n < amount`.
    pub fn split(&mut self, n: i32) -> Option<Stack> {
        gothic_portcov::hit!("split");
        if n < 1 || self.amount <= n {
            return None;
        }
        let mut s = Stack::new(&self.template, self.info.clone());
        s.amount = n;
        s.quality = self.quality;
        self.amount -= n;
        Some(s)
    }

    /// Port of gCInventoryStack::GetCombinedQuality (Game:0x100c0600): stack bits or the template bits.
    pub fn combined_quality(&self) -> u32 {
        gothic_portcov::hit!("combined_quality");
        self.quality | self.item().quality
    }

    /// Port of gCInventoryStack::GetItemGoldValue (Game:0x100c0630).
    pub fn item_gold_value(&self) -> i32 {
        gothic_portcov::hit!("item_gold_value");
        self.item().gold_value
    }

    /// Port of gCInventoryStack::GetStackGoldValue (Game:0x100c0660).
    pub fn stack_gold_value(&self) -> i32 {
        gothic_portcov::hit!("stack_gold_value");
        self.amount.saturating_mul(self.item().gold_value)
    }

    /// Port of gCInventoryStack::IsEmpty (Game:0x100c03b0).
    pub fn is_empty(&self) -> bool {
        gothic_portcov::hit!("is_empty");
        self.amount < 1
    }

    /// Port of gCInventoryStack::SetTransactionAmount (Game:0x100c0d30): clamp to the amount.
    pub fn set_transaction_amount(&mut self, n: i32) {
        gothic_portcov::hit!("set_transaction_amount");
        self.transaction = n.min(self.amount);
    }

    /// Port of gCInventoryStack::GetTransferAmount (Game:0x100c0ce0): the marked amount, everything when the
    /// mark is negative, never more than the stack holds.
    pub fn transfer_amount(&self) -> i32 {
        gothic_portcov::hit!("transfer_amount");
        if self.transaction < 0 {
            self.amount
        } else {
            self.transaction.min(self.amount)
        }
    }

    /// Port of gCInventoryStack::GetTransferRemainder (Game:0x100c0d00): what stays behind after the transfer.
    pub fn transfer_remainder(&self) -> i32 {
        gothic_portcov::hit!("transfer_remainder");
        let t = if (0..=self.amount).contains(&self.transaction) {
            self.transaction
        } else {
            self.amount
        };
        self.amount - t
    }

    /// Port of gCInventoryStack::GetIcon (Game:0x100c0a40, selection rule): the normal icon once the stack
    /// is learned or activated, the grey one before.
    pub fn shows_normal_icon(&self) -> bool {
        gothic_portcov::hit!("shows_normal_icon");
        self.learned || self.activation_count > 0
    }

    /// Port of gCInventoryStack::GetIconFileNames (Game:0x100c0ae0): `(normal, grey)` icon
    /// file names. An empty template icon is replaced by `category_default(category)` for
    /// both. A grey variant (`_Grey` before the extension) exists for spell books with a
    /// magic spell and for anything with a skill proxy.
    pub fn icon_file_names(
        &self,
        category_default: impl Fn(Category) -> String,
    ) -> (String, String) {
        gothic_portcov::hit!("icon_file_names");
        let it = self.item();
        if self.info.is_none() {
            let d = category_default(Category::None);
            return (d.clone(), d);
        }
        if it.icon.is_empty() {
            let d = category_default(it.category);
            return (d.clone(), d);
        }
        let magic = it.category == Category::Spellbook && it.spell_is_magic;
        if !magic && !it.skill_is_skill {
            return (it.icon.clone(), it.icon.clone());
        }
        let grey = match it.icon.rfind('.') {
            Some(dot) => format!("{}_Grey{}", &it.icon[..dot], &it.icon[dot..]),
            None => format!("{}_Grey", it.icon),
        };
        (it.icon.clone(), grey)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stack(name: &str, cat: Category, ut: u32) -> Stack {
        Stack::new(name, Some(Arc::new(ItemInfo::new(name, cat, ut))))
    }

    #[test]
    fn defaults_match_apply_defaults() {
        let s = Stack::new("It_X", None);
        assert_eq!((s.amount, s.quality, s.quick_slot), (1, 0, -1));
        assert_eq!(
            (s.activation_count, s.transaction, s.learned),
            (0, 0, false)
        );
    }

    #[test]
    fn category_predicates() {
        let w = stack("sword", Category::Weapon, use_type::ONE_H);
        assert!(w.is_equipment() && !w.is_consumable() && !w.is_permanent());
        let letter = stack("letter", Category::Artefact, use_type::LETTER);
        assert!(!letter.is_equipment());
        let ring = stack("ring", Category::Artefact, use_type::RING);
        assert!(ring.is_equipment());
        assert!(stack("potion", Category::Alchemy, 16).is_consumable());
        assert!(stack("perk", Category::Skill, 0).is_permanent());
        assert!(stack("spell", Category::Spellbook, 0).is_permanent());
    }

    #[test]
    fn merge_requires_equal_unlinked_unassigned_stacks() {
        let mut a = stack("apple", Category::Alchemy, 19);
        let mut b = stack("APPLE", Category::Alchemy, 19);
        a.amount = 3;
        b.amount = 4;
        assert!(a.can_merge(&b));
        b.quality = 0x20;
        assert!(!a.can_merge(&b));
        b.quality = 0;
        b.quick_slot = 2;
        assert!(!a.can_merge(&b));
        b.quick_slot = -1;
        b.slot = Slot::BackRight;
        assert!(!a.can_merge(&b));
        b.slot = Slot::None;
        b.kind = StackType::Merchandise;
        assert!(!a.can_merge(&b));
        b.kind = StackType::Normal;
        assert!(a.merge(&mut b));
        assert_eq!((a.amount, b.amount), (7, 0));
    }

    #[test]
    fn split_keeps_both_halves_non_empty() {
        let mut a = stack("arrow", Category::Weapon, use_type::ARROW);
        a.amount = 10;
        a.quality = 0x40;
        assert!(a.split(0).is_none());
        assert!(a.split(10).is_none());
        let b = a.split(4).unwrap();
        assert_eq!((a.amount, b.amount, b.quality), (6, 4, 0x40));
    }

    #[test]
    fn must_split_rules() {
        let sword = stack("sword", Category::Weapon, use_type::ONE_H);
        assert!(sword.must_split_before_linking(Slot::BackRight));
        let arrow = stack("arrow", Category::Weapon, use_type::ARROW);
        assert!(!arrow.must_split_before_linking(Slot::Ammo));
        assert!(arrow.must_split_before_linking(Slot::RightHand));
        let potion = stack("potion", Category::Alchemy, 16);
        assert!(!potion.must_split_before_linking(Slot::RightHand));
    }

    #[test]
    fn equipped_tests_use_primary_and_alternative_slots() {
        let mut s = stack("sword", Category::Weapon, use_type::ONE_H);
        s.slot = Slot::BackRight;
        assert!(s.is_equipped_in_primary() && s.is_equipped());
        s.slot = Slot::BackLeft;
        assert!(s.is_equipped_in_alternative() && !s.is_equipped_in_primary());
        s.slot = Slot::RightHand;
        assert!(!s.is_equipped() && s.is_in_use() && !s.is_disposable());
    }

    #[test]
    fn transaction_amounts() {
        let mut s = stack("ore", Category::Misc, 0);
        s.amount = 5;
        s.set_transaction_amount(9);
        assert_eq!(s.transaction, 5);
        s.set_transaction_amount(2);
        assert_eq!((s.transfer_amount(), s.transfer_remainder()), (2, 3));
        s.transaction = -1;
        assert_eq!((s.transfer_amount(), s.transfer_remainder()), (5, 0));
        s.amount = 3;
        s.item_gold_value();
        assert_eq!(s.stack_gold_value(), 0);
    }

    #[test]
    fn icon_names_and_grey_variant() {
        let mut info = ItemInfo::new("Perk", Category::Spellbook, 0);
        info.icon = "IT_Perk_Foo.tga".into();
        info.skill = Some("skill".into());
        info.skill_is_skill = true;
        let s = Stack::new("perk", Some(Arc::new(info)));
        let (n, g) = s.icon_file_names(|_| "default.tga".into());
        assert_eq!(
            (n.as_str(), g.as_str()),
            ("IT_Perk_Foo.tga", "IT_Perk_Foo_Grey.tga")
        );
        assert!(!s.shows_normal_icon());
        let mut plain = ItemInfo::new("Apple", Category::Alchemy, 19);
        plain.icon.clear();
        let s = Stack::new("apple", Some(Arc::new(plain)));
        let (n, g) = s.icon_file_names(|c| format!("cat{}.tga", c as u8));
        assert_eq!((n.as_str(), g.as_str()), ("cat4.tga", "cat4.tga"));
    }
}
