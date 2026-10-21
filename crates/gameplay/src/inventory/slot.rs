//! Equipment slots, use types and bone descriptors (`gESlot`, `gEUseType`).

/// `gESlot`. Slots 1..=3 are the hand slots, 5..=14 the carry slots, 15..=18 the body parts
/// that are filled by default items (hair, head, body, helmet).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
#[repr(u8)]
pub enum Slot {
    #[default]
    None = 0,
    RightHand = 1,
    LeftHand = 2,
    EquipLeftHand = 3,
    Beard = 4,
    BackLeft = 5,
    BackRight = 6,
    Bow = 7,
    Crossbow = 8,
    Armor = 9,
    Belt = 10,
    Amulet = 11,
    Ring1 = 12,
    Ring2 = 13,
    Ammo = 14,
    Hair = 15,
    Head = 16,
    Body = 17,
    Helmet = 18,
}

/// Number of `gESlot` values (`None` included); the inventory owns one default-item object
/// per slot.
pub const SLOT_COUNT: usize = 19;

impl Slot {
    pub const ALL: [Slot; SLOT_COUNT] = [
        Slot::None,
        Slot::RightHand,
        Slot::LeftHand,
        Slot::EquipLeftHand,
        Slot::Beard,
        Slot::BackLeft,
        Slot::BackRight,
        Slot::Bow,
        Slot::Crossbow,
        Slot::Armor,
        Slot::Belt,
        Slot::Amulet,
        Slot::Ring1,
        Slot::Ring2,
        Slot::Ammo,
        Slot::Hair,
        Slot::Head,
        Slot::Body,
        Slot::Helmet,
    ];

    pub fn from_index(i: usize) -> Option<Slot> {
        Self::ALL.get(i).copied()
    }

    pub fn index(self) -> usize {
        self as usize
    }

    /// Enumerator name as printed by the engine (slot name table, Game 0x102baf98).
    pub fn name(self) -> &'static str {
        const NAMES: [&str; SLOT_COUNT] = [
            "None",
            "RightHand",
            "LeftHand",
            "EquipLeftHand",
            "Beard",
            "BackLeft",
            "BackRight",
            "Bow",
            "Crossbow",
            "Armor",
            "Belt",
            "Amulet",
            "Ring1",
            "Ring2",
            "Ammo",
            "Hair",
            "Head",
            "Body",
            "Helmet",
        ];
        NAMES[self.index()]
    }

    /// The two hand slots a drawn weapon is held in.
    pub fn is_hand(self) -> bool {
        matches!(self, Slot::RightHand | Slot::LeftHand)
    }
}

/// Port of gCInventory_PS::IsDisplaySlot (Game:0x100b34e0): slots whose item is shown
/// on the character (carry slots, armor, jewellery and the body parts below Helmet).
pub fn is_display_slot(slot: Slot) -> bool {
    gothic_portcov::hit!("is_display_slot");
    (5..=0x11).contains(&(slot as u8))
}

/// `gEUseType` values that the slot rules distinguish (the full enum is in
/// `gothic_formats::props::enums`).
pub mod use_type {
    pub const NONE: u32 = 0;
    pub const ONE_H: u32 = 2;
    pub const TWO_H: u32 = 3;
    pub const ARROW: u32 = 4;
    pub const BOW: u32 = 5;
    pub const CROSSBOW: u32 = 6;
    pub const BOLT: u32 = 7;
    pub const FIST: u32 = 8;
    pub const SHIELD: u32 = 9;
    pub const ARMOR: u32 = 10;
    pub const HELMET: u32 = 11;
    pub const STAFF: u32 = 12;
    pub const AMULET: u32 = 13;
    pub const RING: u32 = 14;
    pub const CAST: u32 = 15;
    pub const TORCH: u32 = 24;
    pub const LETTER: u32 = 44;
    pub const HALBERD: u32 = 51;
    pub const AXE: u32 = 52;
    pub const PHYSICAL_FIST: u32 = 55;
    pub const ITEM_H: u32 = 56;
    pub const HAIR: u32 = 112;
    pub const BODY: u32 = 114;
    /// Highest valid value (`Talk`); `CompareType` clamps anything else to `None`.
    pub const MAX: u32 = 116;

    /// Use types of weapons that are carried with two hands (they occupy the right hand and
    /// force the left hand free).
    pub fn is_two_handed(u: u32) -> bool {
        matches!(u, TWO_H | STAFF | HALBERD | AXE)
    }
}

/// Port of gCInventorySlot::GetEquipSlots (static; Game:0x100bd2a0). Returns the primary and
/// the alternative carry slot of an item with this use type; `Slot::None` when it has none.
pub fn equip_slots(use_type: u32) -> (Slot, Slot) {
    gothic_portcov::hit!("equip_slots");
    use use_type::*;
    match use_type {
        ONE_H => (Slot::BackRight, Slot::BackLeft),
        TWO_H | STAFF | HALBERD | AXE => (Slot::BackRight, Slot::None),
        ARROW | BOLT => (Slot::Ammo, Slot::None),
        BOW | CROSSBOW => (Slot::Bow, Slot::None),
        SHIELD => (Slot::BackLeft, Slot::None),
        ARMOR => (Slot::Armor, Slot::None),
        HELMET => (Slot::Helmet, Slot::None),
        AMULET => (Slot::Amulet, Slot::None),
        RING => (Slot::Ring1, Slot::Ring2),
        HAIR => (Slot::Hair, Slot::None),
        BODY => (Slot::Body, Slot::None),
        _ => (Slot::None, Slot::None),
    }
}

/// `gCInventorySlot::ELink`: how an item's visual joins the character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    /// No visual on the skeleton.
    None,
    /// Attached to a bone (`eCVisualAnimation_PS::AddAttachment`).
    Attachment,
    /// Skinned body part replacing the base mesh part (head, body).
    BodyPart,
}

/// Port of gCInventorySlot::GetBone (Game:0x100bd8d0): the bone descriptor and link kind for
/// an item with `use_type` in `slot`. Slots without a visual return an empty descriptor and
/// `LinkKind::None`.
///
/// Reconstruction note: for the back slots the decompiler shows an inner switch whose
/// `Fist`/`Torch`/`ITEM_H` cases fall out of the switch; they are treated as "no bone".
pub fn bone(slot: Slot, use_type: u32) -> (&'static str, LinkKind) {
    gothic_portcov::hit!("bone");
    use use_type::*;
    const NONE_: (&str, LinkKind) = ("", LinkKind::None);
    let att = |n| (n, LinkKind::Attachment);
    match slot {
        Slot::None | Slot::EquipLeftHand => NONE_,
        Slot::Crossbow | Slot::Armor | Slot::Belt | Slot::Amulet => NONE_,
        Slot::Ring1 | Slot::Ring2 | Slot::Ammo => NONE_,
        Slot::RightHand if use_type != FIST => att("Slot_RightHand_Weapon"),
        Slot::RightHand => NONE_,
        Slot::LeftHand if use_type == SHIELD => att("Slot_LeftHand_Shield"),
        Slot::LeftHand if use_type != FIST => att("Slot_LeftHand_Weapon"),
        Slot::LeftHand => NONE_,
        Slot::Beard => att("Slot_Beard"),
        Slot::BackLeft => match use_type {
            FIST | TORCH | ITEM_H => NONE_,
            SHIELD => att("Slot_ShieldBack"),
            _ => att("Slot_MeleeBackLeft"),
        },
        Slot::BackRight => match use_type {
            TWO_H | AXE => att("Slot_AxeBack"),
            STAFF | HALBERD => att("Slot_StaffBack"),
            FIST | ITEM_H => NONE_,
            _ => att("Slot_MeleeBackRight"),
        },
        Slot::Bow => att(if use_type == CROSSBOW {
            "Slot_CrossBow"
        } else {
            "Slot_Bow"
        }),
        Slot::Hair => att("Slot_Hair"),
        Slot::Head => ("Slot_Head", LinkKind::BodyPart),
        Slot::Body => ("Slot_Body", LinkKind::BodyPart),
        Slot::Helmet => att("Slot_Helmet"),
    }
}

/// Port of gCInventory_PS::IsSkinnedSlot (Game:0x100b50b0): the item in this slot replaces a skinned body part.
pub fn is_skinned_slot(slot: Slot, use_type: u32) -> bool {
    gothic_portcov::hit!("is_skinned_slot");
    bone(slot, use_type).1 == LinkKind::BodyPart
}

#[cfg(test)]
mod tests {
    use super::*;
    use use_type::*;

    #[test]
    fn equip_slot_table_matches_the_original() {
        assert_eq!(equip_slots(ONE_H), (Slot::BackRight, Slot::BackLeft));
        assert_eq!(equip_slots(TWO_H), (Slot::BackRight, Slot::None));
        assert_eq!(equip_slots(HALBERD), (Slot::BackRight, Slot::None));
        assert_eq!(equip_slots(SHIELD), (Slot::BackLeft, Slot::None));
        assert_eq!(equip_slots(ARROW), (Slot::Ammo, Slot::None));
        assert_eq!(equip_slots(CROSSBOW), (Slot::Bow, Slot::None));
        assert_eq!(equip_slots(RING), (Slot::Ring1, Slot::Ring2));
        assert_eq!(equip_slots(BODY), (Slot::Body, Slot::None));
        assert_eq!(equip_slots(16 /* Potion */), (Slot::None, Slot::None));
    }

    #[test]
    fn bones_follow_use_type() {
        assert_eq!(bone(Slot::BackRight, TWO_H).0, "Slot_AxeBack");
        assert_eq!(bone(Slot::BackRight, STAFF).0, "Slot_StaffBack");
        assert_eq!(bone(Slot::BackRight, ONE_H).0, "Slot_MeleeBackRight");
        assert_eq!(bone(Slot::LeftHand, SHIELD).0, "Slot_LeftHand_Shield");
        assert_eq!(bone(Slot::Bow, CROSSBOW).0, "Slot_CrossBow");
        assert!(is_skinned_slot(Slot::Head, 113));
        assert!(!is_skinned_slot(Slot::Hair, HAIR));
        assert_eq!(bone(Slot::Armor, ARMOR).1, LinkKind::None);
    }

    #[test]
    fn display_slots() {
        assert!(!is_display_slot(Slot::RightHand));
        assert!(is_display_slot(Slot::BackLeft));
        assert!(is_display_slot(Slot::Body));
        assert!(!is_display_slot(Slot::Helmet));
    }
}
