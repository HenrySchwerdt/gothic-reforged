//! What a character wears and holds, kept in the stack list like the original does it.
//!
//! The original has no equipment record: worn items are stacks linked to `gCInventorySlot`s
//! (`RightHand` / `LeftHand` while a weapon is drawn, the carry slots `BackRight`, `BackLeft`,
//! `Bow`, `Armor` ... otherwise) and the scripts ask `FindStackIndex(slot)` / `HoldStacks`
//! (`gCInventory_PS::EquipStack` Game:0x100b8160, `HoldStacks` 0x100b8370, `UnEquipStack`
//! 0x100b8310). [`Equipment`] is the combat rules' view of that and is derived from the links
//! by [`Actor::derive_equipment`]; everything that changes what is worn goes through
//! [`Actor::wear`], [`Actor::hold`] and [`Actor::unwear`].
//!
//! Derivation: when a hand slot holds a stack the hands are what counts (right hand, left hand);
//! otherwise the carry slots are (`BackRight`; `BackLeft`, then `Bow`). A bow in the left hand
//! comes with the ammunition of the quiver, which stays a plain count of the bag (shots take it
//! from the unlinked stacks). Armor and jewellery come from their slots. Items without any
//! stack (hand-built fixtures) keep their place until a hand change links them (`adopt`).
use crate::actor::Actor;
use crate::item::{Equipment, ItemDb, ItemDef, Use};
use gothic_gameplay::inventory::{Slot, StackType};

fn right_type(u: Use) -> bool {
    matches!(
        u,
        Use::OneH | Use::TwoH | Use::Axe | Use::Staff | Use::Halberd | Use::Cast | Use::Fist
    )
}

fn left_type(u: Use) -> bool {
    matches!(u, Use::Shield | Use::Torch | Use::Bow | Use::CrossBow)
}

fn ammo(u: Use) -> bool {
    matches!(u, Use::Arrow | Use::Bolt)
}

impl Actor {
    /// The stack that holds `def`: any stack of the template other than `avoid`, else a new one.
    fn stack_for(&mut self, def: &ItemDef, items: &ItemDb, avoid: Option<usize>) -> Option<usize> {
        let found = self
            .inventory
            .stacks()
            .iter()
            .enumerate()
            .filter(|(k, s)| {
                Some(*k) != avoid
                    && s.template.eq_ignore_ascii_case(&def.name)
                    && s.kind != StackType::Merchandise
                    && s.amount > 0
            })
            // a stack that is not in a hand first, one that waits in the bag before one worn
            .min_by_key(|(_, s)| (s.slot.is_hand(), s.is_linked()))
            .map(|(k, _)| k);
        found.or_else(|| {
            let k =
                self.inventory
                    .add_items(&def.name, items.info(&def.name), 1, 0, StackType::Normal);
            (k >= 0).then_some(k as usize)
        })
    }

    fn hands_occupied(&self) -> bool {
        self.inventory.find_stack(Slot::RightHand).is_some()
            || self.inventory.find_stack(Slot::LeftHand).is_some()
    }

    /// Rebuild [`Equipment`] from the links of the stack list.
    pub fn derive_equipment(&mut self, items: &ItemDb) {
        let inv = &self.inventory;
        let def = |slot: Slot| {
            inv.find_stack(slot)
                .and_then(|s| items.get(&s.template))
                .cloned()
        };
        let (mut right, mut left) = if self.hands_occupied() {
            (def(Slot::RightHand), def(Slot::LeftHand))
        } else {
            (
                def(Slot::BackRight),
                def(Slot::BackLeft).or_else(|| def(Slot::Bow)),
            )
        };
        if right.is_none()
            && let Some(l) = left
                .as_ref()
                .filter(|l| matches!(l.use_kind(), Use::Bow | Use::CrossBow))
            && self.hands_occupied()
        {
            let name = if l.use_kind() == Use::CrossBow {
                "Bolt"
            } else {
                "Arrow"
            };
            // the ammunition that was in the hand stays (the hero's own arrows), else the
            // default of the bow
            right = self
                .equipment
                .right
                .clone()
                .filter(|d| ammo(d.use_kind()))
                .or_else(|| items.get(name).cloned());
        }
        let mut eq = Equipment {
            left: left.take(),
            right: right.take(),
            body: def(Slot::Armor).or_else(|| def(Slot::Body)),
            accessories: Vec::new(),
        };
        for slot in [
            Slot::Helmet,
            Slot::Amulet,
            Slot::Belt,
            Slot::Ring1,
            Slot::Ring2,
        ] {
            if let Some(d) = def(slot) {
                eq.accessories.push(d);
            }
        }
        // Fixtures that were put on without a stack keep their place.
        for old in self.equipment.all() {
            let has_stack = inv
                .stacks()
                .iter()
                .any(|s| s.template.eq_ignore_ascii_case(&old.name));
            if has_stack || eq.all().any(|d| d.name.eq_ignore_ascii_case(&old.name)) {
                continue;
            }
            // only into a place that is still free: an item that was used up (its stack is
            // gone) must not push out what the hands hold now
            let kind = old.use_kind();
            let free = if left_type(kind) {
                eq.left.is_none()
            } else if right_type(kind) || ammo(kind) {
                eq.right.is_none()
            } else if kind == Use::Body {
                eq.body.is_none()
            } else {
                true
            };
            if free {
                eq.equip(old.clone());
            }
        }
        self.equipment = eq;
    }

    /// Put `def` on (`gCInventory_PS::EquipStack`): into its carry slot, and into the hand when
    /// the hands are drawn and it is something that goes there.
    pub fn wear(&mut self, def: &ItemDef, items: &ItemDb) {
        // stacks that came through a count-only path have no item data yet
        self.inventory.rebind(items);
        let kind = def.use_kind();
        if ammo(kind) {
            // the quiver is a count; a bow in the hand carries it
            self.equipment.equip(def.clone());
            return;
        }
        let Some(k) = self.stack_for(def, items, None) else {
            self.equipment.equip(def.clone());
            return;
        };
        if !self.inventory.equip_stack(k) {
            self.equipment.equip(def.clone());
            return;
        }
        // Spells, fists and torches have no carry slot: they are held.
        let no_carry_slot = self.inventory.stacks()[k].equip_slots().0 == Slot::None;
        if self.hands_occupied() || no_carry_slot {
            if right_type(kind) {
                self.inventory.hold_stack_right_hand(k as i32);
                if matches!(kind, Use::TwoH | Use::Axe | Use::Staff | Use::Halberd) {
                    self.inventory.hold_stack_left_hand(-1);
                }
            } else if left_type(kind) {
                self.inventory.hold_stack_left_hand(k as i32);
            }
        }
        self.inventory.take_stat_events();
        self.inventory.clear_link_events();
        self.derive_equipment(items);
    }

    /// Items that were put on without a stack (fixtures) get their stacks linked first, so
    /// that a change of the hands sees them.
    fn adopt(&mut self, items: &ItemDb) {
        let foreign: Vec<ItemDef> = self
            .equipment
            .all()
            .filter(|d| {
                !ammo(d.use_kind())
                    && !self
                        .inventory
                        .stacks()
                        .iter()
                        .any(|s| s.is_linked() && s.template.eq_ignore_ascii_case(&d.name))
            })
            .cloned()
            .collect();
        for d in foreign {
            self.wear(&d, items);
        }
    }

    /// `HoldStacks`: `right` and `left` go into the hands, what they held goes back to its
    /// carry slot; `None` for both puts everything away.
    pub fn hold(&mut self, right: Option<&ItemDef>, left: Option<&ItemDef>, items: &ItemDb) {
        self.inventory.rebind(items);
        self.adopt(items);
        let r = right
            .filter(|d| !ammo(d.use_kind()))
            .and_then(|d| self.stack_for(d, items, None));
        let l = left.and_then(|d| self.stack_for(d, items, r));
        // a stack of a hand that is wanted in the other one first leaves its carry slot
        self.inventory
            .hold_stacks(r.map_or(-1, |k| k as i32), l.map_or(-1, |k| k as i32));
        self.inventory.take_stat_events();
        self.inventory.clear_link_events();
        self.derive_equipment(items);
    }

    /// `UnEquipStack`: the item leaves its slot (hand or carry slot) and stays in the bag.
    pub fn unwear(&mut self, template: &str, items: &ItemDb) {
        self.inventory.rebind(items);
        let slot = self
            .inventory
            .stacks()
            .iter()
            .find(|s| s.is_linked() && s.template.eq_ignore_ascii_case(template))
            .map(|s| s.slot);
        if let Some(slot) = slot {
            self.inventory.unlink_stack_from_slot(slot);
        }
        self.inventory.take_stat_events();
        self.inventory.clear_link_events();
        let eq = &mut self.equipment;
        for f in [&mut eq.left, &mut eq.right, &mut eq.body] {
            if f.as_ref()
                .is_some_and(|d| d.name.eq_ignore_ascii_case(template))
            {
                *f = None;
            }
        }
        eq.accessories
            .retain(|d| !d.name.eq_ignore_ascii_case(template));
        self.derive_equipment(items);
    }

    /// The weapon of the right hand or, when the hands are empty, of its carry slot.
    pub fn weapon_defs(&self, items: &ItemDb) -> (Option<ItemDef>, Option<ItemDef>) {
        let def = |slot: Slot| {
            self.inventory
                .find_stack(slot)
                .and_then(|s| items.get(&s.template))
                .cloned()
        };
        (
            def(Slot::RightHand)
                .or_else(|| def(Slot::BackRight))
                .or_else(|| self.equipment.right.clone().filter(|d| !ammo(d.use_kind()))),
            def(Slot::LeftHand)
                .or_else(|| def(Slot::BackLeft))
                .or_else(|| def(Slot::Bow))
                .or_else(|| self.equipment.left.clone()),
        )
    }
}
