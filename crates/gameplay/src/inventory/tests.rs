//! Behaviour tests of [`Inventory`] with synthetic items (the rules, not the data).
use super::item::{ArmorSet, StatMod, StatReq};
use super::slot::use_type::*;
use super::*;
use std::sync::Arc;

fn info(name: &str, cat: Category, ut: u32) -> ItemInfo {
    ItemInfo::new(name, cat, ut)
}

fn catalog() -> MapCatalog {
    let mut c = MapCatalog::default();
    c.insert(info("It_1H_Sword", Category::Weapon, ONE_H));
    c.insert(info("It_1H_Axe", Category::Weapon, ONE_H));
    c.insert(info("It_2H_Sword", Category::Weapon, TWO_H));
    c.insert(info("It_Shield", Category::Weapon, SHIELD));
    c.insert(info("It_Bow", Category::Weapon, BOW));
    c.insert(info("It_Arrow", Category::Weapon, ARROW));
    c.insert(info("It_Potion", Category::Alchemy, 16));
    c.insert(info("It_Letter", Category::Artefact, LETTER));
    let mut ring = info("It_Ring", Category::Artefact, RING);
    ring.mods.push(StatMod {
        tag: "STR".into(),
        op: 0,
        value: 10,
    });
    ring.mods.push(StatMod {
        tag: "PROT_BLADE".into(),
        op: 0,
        value: 5,
    });
    ring.activate_skills.push("Skill_Sneak".into());
    c.insert(ring);
    let mut perk = info("It_Perk_Sneak", Category::Skill, 0);
    perk.permanent = true;
    perk.skill = Some("Skill_Sneak".into());
    c.insert(perk);
    let mut quest = info("It_Key", Category::Misc, 45);
    quest.mission_item = true;
    c.insert(quest);
    c.insert(info("It_Hair", Category::Misc, HAIR));
    c.insert(info("It_Hair2", Category::Misc, HAIR));
    c
}

fn slot_of(inv: &Inventory, i: i32) -> Slot {
    inv.stack(i as usize).unwrap().slot
}

#[test]
fn stacks_merge_by_template_quality_and_type() {
    let c = catalog();
    let mut inv = Inventory::new();
    let a = inv.create_items(&c, "It_Potion", 0, 3);
    let b = inv.create_items(&c, "it_potion", 0, 2);
    assert_eq!((a, b), (0, 0));
    assert_eq!(inv.stack(0).unwrap().amount, 5);
    let q = inv.create_items(&c, "It_Potion", 0x20, 1);
    assert_eq!(q, 1);
    let m = inv.add_items(
        "It_Potion",
        c.item("It_Potion"),
        4,
        0,
        StackType::Merchandise,
    );
    assert_eq!(m, 2);
    assert_eq!(inv.create_items(&c, "It_Potion", 0, 0), -1);
    // Merchandise does not count as owned.
    assert_eq!(inv.count("it_potion"), 6);
    assert_eq!(inv["IT_POTION"], 6);
}

#[test]
fn assure_items_tops_up_to_the_requested_count() {
    let c = catalog();
    let mut inv = Inventory::new();
    inv.create_items(&c, "It_Potion", 0, 2);
    let i = inv.assure_items(&c, "It_Potion", 0, 5);
    assert_eq!(inv.count("It_Potion"), 5);
    assert_eq!(inv.stack(i as usize).unwrap().template, "It_Potion");
    // Already enough: nothing created, first stack returned.
    let n = inv.len();
    assert_eq!(inv.assure_items(&c, "It_Potion", 0, 5), 0);
    assert_eq!(inv.len(), n);
    // Merchandise stacks are ignored.
    inv.add_items("It_Key", c.item("It_Key"), 3, 0, StackType::Merchandise);
    inv.assure_items(&c, "It_Key", 0, 1);
    assert_eq!(inv.count("It_Key"), 1);
}

#[test]
fn find_stack_index_quality_modes() {
    let c = catalog();
    let mut inv = Inventory::new();
    inv.create_items(&c, "It_Potion", 0x20, 1);
    inv.create_items(&c, "It_Potion", 0x80, 1);
    inv.create_items(&c, "It_Potion", 0x40, 1);
    let t = "It_Potion";
    assert_eq!(inv.find_stack_index(t, FindQuality::Any, 0, -1), 0);
    assert_eq!(inv.find_stack_index(t, FindQuality::Exact, 0x40, -1), 2);
    assert_eq!(inv.find_stack_index(t, FindQuality::Best, 0, -1), 1);
    assert_eq!(inv.find_stack_index(t, FindQuality::Worst, 0, -1), 0);
    assert_eq!(inv.find_stack_index(t, FindQuality::Any, 0, 0), 1);
    assert_eq!(inv.find_stack_index("none", FindQuality::Any, 0, -1), -1);
    assert_eq!(
        inv.find_stack_index_by_use_type(16, FindQuality::Any, 0, -1),
        0
    );
}

#[test]
fn equip_goes_to_primary_then_alternative_slot() {
    let c = catalog();
    let mut inv = Inventory::new();
    let s1 = inv.create_items(&c, "It_1H_Sword", 0, 1);
    let s2 = inv.create_items(&c, "It_1H_Axe", 0, 1);
    assert!(inv.equip_stack(s1 as usize));
    assert_eq!(slot_of(&inv, s1), Slot::BackRight);
    assert!(inv.equip_stack(s2 as usize));
    assert_eq!(slot_of(&inv, s2), Slot::BackLeft);
    // Equipping again is a no-op that succeeds.
    assert!(inv.equip_stack(s1 as usize));
    assert!(inv.stack(s1 as usize).unwrap().is_equipped_in_primary());
    assert!(inv.stack(s2 as usize).unwrap().is_equipped_in_alternative());
    // Items without a use type cannot be equipped.
    let p = inv.create_items(&c, "It_Hair", 0, 1);
    let perk = inv.create_items(&c, "It_Perk_Sneak", 0, 1);
    assert!(!inv.equip_stack(perk as usize));
    // A use type without a carry slot succeeds without linking anything.
    let potion = inv.create_items(&c, "It_Potion", 0, 1);
    assert!(inv.equip_stack(potion as usize));
    assert_eq!(slot_of(&inv, potion), Slot::None);
    assert!(inv.equip_stack(p as usize));
    assert_eq!(slot_of(&inv, p), Slot::Hair);
}

#[test]
fn a_new_primary_item_replaces_the_old_one() {
    let c = catalog();
    let mut inv = Inventory::new();
    let one = inv.create_items(&c, "It_1H_Sword", 0, 1) as usize;
    let two = inv.create_items(&c, "It_2H_Sword", 0, 1) as usize;
    inv.equip_stack(one);
    inv.equip_stack(two);
    assert_eq!(inv.stack(two).unwrap().slot, Slot::BackRight);
    assert_eq!(inv.stack(one).unwrap().slot, Slot::None);
}

#[test]
fn shield_unlinks_a_held_two_hander() {
    let c = catalog();
    let mut inv = Inventory::new();
    let two = inv.create_items(&c, "It_2H_Sword", 0, 1) as usize;
    let sh = inv.create_items(&c, "It_Shield", 0, 1) as usize;
    inv.equip_stack(two);
    inv.hold_stack_right_hand(two as i32);
    assert_eq!(inv.stack(two).unwrap().slot, Slot::RightHand);
    inv.equip_stack(sh);
    assert_eq!(inv.stack(sh).unwrap().slot, Slot::BackLeft);
    assert_eq!(inv.stack(two).unwrap().slot, Slot::None);
}

#[test]
fn second_one_hander_goes_left_when_the_right_hand_holds_one() {
    let c = catalog();
    let mut inv = Inventory::new();
    let a = inv.create_items(&c, "It_1H_Sword", 0, 1) as usize;
    let b = inv.create_items(&c, "It_1H_Axe", 0, 1) as usize;
    inv.equip_stack(a);
    inv.hold_stack_right_hand(a as i32);
    assert_eq!(inv.stack(a).unwrap().slot, Slot::RightHand);
    // The back-right slot is free but the right hand holds a one-hander: use the left slot.
    inv.equip_stack(b);
    assert_eq!(inv.stack(b).unwrap().slot, Slot::BackLeft);
}

#[test]
fn drawing_a_weapon_returns_the_previous_one_to_its_carry_slot() {
    let c = catalog();
    let mut inv = Inventory::new();
    let a = inv.create_items(&c, "It_1H_Sword", 0, 1) as usize;
    let b = inv.create_items(&c, "It_2H_Sword", 0, 1) as usize;
    inv.equip_stack(a);
    inv.hold_stack_right_hand(a as i32);
    assert!(inv.stack(a).unwrap().is_in_use());
    assert!(!inv.stack(a).unwrap().is_equipped());
    assert_eq!(inv.right_equip_slot(a), Slot::BackRight);
    inv.hold_stack_right_hand(b as i32);
    assert_eq!(inv.stack(a).unwrap().slot, Slot::BackRight);
    assert_eq!(inv.stack(b).unwrap().slot, Slot::RightHand);
    // Holding nothing returns the weapon.
    inv.hold_stacks(-1, -1);
    assert_eq!(inv.stack(b).unwrap().slot, Slot::BackRight);
    // Moving `a` out of BackRight by returning b freed nothing else: a is unlinked.
    assert_eq!(inv.stack(a).unwrap().slot, Slot::None);
}

#[test]
fn bow_and_shield_are_held_in_the_left_hand() {
    let c = catalog();
    let mut inv = Inventory::new();
    let bow = inv.create_items(&c, "It_Bow", 0, 1) as usize;
    let sh = inv.create_items(&c, "It_Shield", 0, 1) as usize;
    inv.equip_stack(bow);
    assert_eq!(inv.stack(bow).unwrap().slot, Slot::Bow);
    inv.hold_stack_left_hand(bow as i32);
    assert_eq!(inv.stack(bow).unwrap().slot, Slot::LeftHand);
    assert_eq!(inv.left_equip_slot(bow), Slot::Bow);
    inv.hold_stack_left_hand(sh as i32);
    assert_eq!(inv.stack(bow).unwrap().slot, Slot::Bow);
    assert_eq!(inv.stack(sh).unwrap().slot, Slot::LeftHand);
}

#[test]
fn unequip_only_works_from_the_carry_slot() {
    let c = catalog();
    let mut inv = Inventory::new();
    let a = inv.create_items(&c, "It_1H_Sword", 0, 1) as usize;
    assert!(!inv.unequip_stack(a));
    inv.equip_stack(a);
    assert!(inv.unequip_stack(a));
    assert_eq!(inv.stack(a).unwrap().slot, Slot::None);
    inv.equip_stack(a);
    inv.hold_stack_right_hand(a as i32);
    assert!(!inv.unequip_stack(a));
}

#[test]
fn ammunition_keeps_merging_while_linked() {
    let c = catalog();
    let mut inv = Inventory::new();
    let ar = inv.create_items(&c, "It_Arrow", 0, 10) as usize;
    inv.equip_stack(ar);
    assert_eq!(inv.stack(ar).unwrap().slot, Slot::Ammo);
    // Picking up more arrows joins the linked ammunition stack.
    assert_eq!(inv.create_items(&c, "It_Arrow", 0, 5), ar as i32);
    assert_eq!(inv.stack(ar).unwrap().amount, 15);
    // A linked sword stack does not accept more swords.
    let s = inv.create_items(&c, "It_1H_Sword", 0, 1) as usize;
    inv.equip_stack(s);
    let s2 = inv.create_items(&c, "It_1H_Sword", 0, 1) as usize;
    assert_ne!(s, s2);
}

#[test]
fn default_items_fill_empty_slots_and_yield_to_stacks() {
    let c = catalog();
    let mut inv = Inventory::new();
    inv.set_default_item(Slot::Hair, "It_Hair", c.item("It_Hair"));
    assert_eq!(inv.default_item(Slot::Hair), "It_Hair");
    assert!(inv.default_slot(Slot::Hair).unwrap().linked);
    inv.drain_events();
    // A stack linked to the slot removes the default item.
    let h = inv.create_items(&c, "It_Hair2", 0, 1) as usize;
    inv.equip_stack(h);
    assert!(!inv.default_slot(Slot::Hair).unwrap().linked);
    let ev = inv.drain_events();
    assert!(ev.iter().any(|e| matches!(
        e,
        InventoryEvent::DefaultUnlinked {
            slot: Slot::Hair,
            ..
        }
    )));
    // Unequipping brings the default item back.
    inv.unequip_stack(h);
    assert!(inv.default_slot(Slot::Hair).unwrap().linked);
}

#[test]
fn patch_from_template_copies_defaults_and_links_them() {
    let c = catalog();
    let mut tpl = Inventory::new();
    tpl.set_default_item(Slot::Hair, "It_Hair", c.item("It_Hair"));
    tpl.treasure_sets[0] = "TS_Bandit".into();
    let mut inst = Inventory::new();
    inst.patch_from_template(&tpl);
    assert!(inst.default_slot(Slot::Hair).unwrap().linked);
    assert_eq!(inst.treasure_sets[0], "TS_Bandit");
}

#[test]
fn equipment_modifiers_apply_once_and_unapply() {
    let c = catalog();
    let mut inv = Inventory::new();
    let perk = inv.create_items(&c, "It_Perk_Sneak", 0, 1) as usize;
    let ring = inv.create_items(&c, "It_Ring", 0, 1) as usize;
    inv.drain_events();
    inv.equip_stack(ring);
    let ev = inv.drain_events();
    let stats: Vec<_> = ev
        .iter()
        .filter_map(|e| match e {
            InventoryEvent::Stat {
                kind, tag, value, ..
            } => Some((*kind, tag.as_str(), *value)),
            _ => None,
        })
        .collect();
    assert_eq!(
        stats,
        vec![
            (StatKind::Apply, "STR", 10),
            (StatKind::Apply, "PROT_BLADE", 5)
        ]
    );
    // The ring's skill is kept active while it is worn.
    assert!(inv.is_skill_active("skill_sneak"));
    assert_eq!(inv.skill_activation_count("Skill_Sneak"), 1);
    assert!(!inv.has_learned_skill("Skill_Sneak"));
    inv.drain_events();
    inv.unequip_stack(ring);
    let un = inv
        .drain_events()
        .into_iter()
        .filter(|e| {
            matches!(
                e,
                InventoryEvent::Stat {
                    kind: StatKind::Unapply,
                    ..
                }
            )
        })
        .count();
    assert_eq!(un, 2);
    assert!(!inv.is_skill_active("Skill_Sneak"));
    let _ = perk;
}

#[test]
fn damaged_items_apply_half_their_modifiers_and_no_skills() {
    let c = catalog();
    let mut inv = Inventory::new();
    inv.create_items(&c, "It_Perk_Sneak", 0, 1);
    let ring = inv.create_items(&c, "It_Ring", QUALITY_DAMAGED_FOR_TESTS, 1) as usize;
    inv.drain_events();
    inv.equip_stack(ring);
    let vals: Vec<i32> = inv
        .drain_events()
        .iter()
        .filter_map(|e| match e {
            InventoryEvent::Stat {
                kind: StatKind::Apply,
                value,
                ..
            } => Some(*value),
            _ => None,
        })
        .collect();
    assert_eq!(vals, vec![5, 2]);
    assert!(!inv.is_skill_active("Skill_Sneak"));
}

const QUALITY_DAMAGED_FOR_TESTS: u32 = 0x100;

#[test]
fn complete_armor_set_grants_its_bonus_once() {
    let mut c = MapCatalog::default();
    let names = [
        "P_Armor", "P_Helmet", "P_Amulet", "P_RingA", "P_RingB", "P_Shield",
    ];
    let uses = [ARMOR, HELMET, AMULET, RING, RING, SHIELD];
    let set = Arc::new(ArmorSet {
        pieces: names.map(|n| n.to_ascii_lowercase()),
        mods: vec![StatMod {
            tag: "MAXHP".into(),
            op: 0,
            value: 30,
        }],
        skills: vec![],
    });
    for (n, u) in names.iter().zip(uses) {
        let mut i = info(n, Category::Armor, u);
        i.armor_set = Some(set.clone());
        c.insert(i);
    }
    let mut inv = Inventory::new();
    let idx: Vec<usize> = names
        .iter()
        .map(|n| inv.create_items(&c, n, 0, 1) as usize)
        .collect();
    let bonus = |ev: &[InventoryEvent], k: StatKind| {
        ev.iter()
            .filter(|e| matches!(e, InventoryEvent::Stat { kind, tag, .. } if *kind == k && tag == "MAXHP"))
            .count()
    };
    for &i in &idx[..5] {
        inv.equip_stack(i);
    }
    assert_eq!(bonus(&inv.drain_events(), StatKind::Apply), 0);
    inv.equip_stack(idx[5]);
    assert_eq!(bonus(&inv.drain_events(), StatKind::Apply), 1);
    inv.unequip_stack(idx[0]);
    assert_eq!(bonus(&inv.drain_events(), StatKind::Unapply), 1);
    inv.unequip_stack(idx[1]);
    assert_eq!(bonus(&inv.drain_events(), StatKind::Unapply), 0);
}

#[test]
fn can_equip_applies_the_bonus_and_reports_missing_values() {
    let mut c = MapCatalog::default();
    let mut w = info("It_Heavy", Category::Weapon, TWO_H);
    w.reqs = vec![
        StatReq {
            tag: "STR".into(),
            op: 0,
            value: 50,
        },
        StatReq {
            tag: "DEX".into(),
            op: 0,
            value: 20,
        },
    ];
    w.required_skills = vec!["Skill_Heavy".into()];
    c.insert(w);
    c.insert({
        let mut p = info("It_Perk_Heavy", Category::Skill, 0);
        p.permanent = true;
        p.skill = Some("Skill_Heavy".into());
        p
    });
    let mut inv = Inventory::new();
    let w = inv.create_items(&c, "It_Heavy", 0, 1) as usize;
    let attr = |tag: &str, need: i32| -> i32 {
        let have = if tag == "STR" { 40 } else { 25 };
        (need - have).max(0)
    };
    let r = inv.can_equip(w, 0, Some(&attr));
    assert!(!r.ok);
    assert_eq!(r.missing_attribs, vec![("STR".to_string(), 10)]);
    assert_eq!(r.missing_skills, vec!["Skill_Heavy".to_string()]);
    // A negative bonus lowers every requirement (floored at 0).
    let r = inv.can_equip(w, -10, Some(&attr));
    assert!(r.missing_attribs.is_empty());
    // Without the player-memory set the whole requirement counts as missing.
    let r = inv.can_equip(w, 0, None);
    assert_eq!(r.missing_attribs.len(), 2);
    // Learning the perk satisfies the skill.
    let perk = inv.create_items(&c, "It_Perk_Heavy", 0, 1) as usize;
    inv.learn_stack(perk);
    let r = inv.can_equip(w, -100, Some(&attr));
    assert!(r.ok);
}

#[test]
fn consume_applies_modifiers_and_activates_skills_once() {
    let mut c = catalog();
    let mut elixir = info("It_Elixir", Category::Alchemy, 16);
    elixir.mods.push(StatMod {
        tag: "HP".into(),
        op: 0,
        value: 40,
    });
    elixir.activate_skills.push("Skill_Sneak".into());
    c.insert(elixir);
    let mut inv = Inventory::new();
    inv.create_items(&c, "It_Perk_Sneak", 0, 1);
    let e = inv.create_items(&c, "It_Elixir", 0, 2) as usize;
    inv.drain_events();
    assert!(!inv.consume_items(e, 3).ok);
    let r = inv.consume_items(e, 1);
    assert!(r.ok);
    assert_eq!(r.activated, vec!["Skill_Sneak".to_string()]);
    let ev = inv.drain_events();
    assert!(ev.iter().any(|x| matches!(x,
        InventoryEvent::Stat { kind: StatKind::Consume, tag, value: 40, .. } if tag == "HP")));
    // Second use: the skill is active already.
    assert!(inv.consume_items(e, 1).activated.is_empty());
    assert!(inv.remove_items(e, 1));
    assert_eq!(inv.count("It_Elixir"), 1);
}

#[test]
fn removing_all_items_needs_an_unlinked_stack() {
    let c = catalog();
    let mut inv = Inventory::new();
    let s = inv.create_items(&c, "It_1H_Sword", 0, 1) as usize;
    inv.equip_stack(s);
    assert!(!inv.remove_items(s, 1));
    assert_eq!(inv.count("It_1H_Sword"), 1);
    inv.unequip_stack(s);
    assert!(inv.remove_items(s, 1));
    assert!(inv.is_empty());
    // Skills are permanent: removal succeeds but changes nothing.
    let p = inv.create_items(&c, "It_Perk_Sneak", 0, 1) as usize;
    assert!(inv.remove_items(p, 1));
    assert_eq!(inv.count("It_Perk_Sneak"), 1);
}

#[test]
fn drop_unlinks_and_reports_the_pickup() {
    let c = catalog();
    let mut inv = Inventory::new();
    let s = inv.create_items(&c, "It_Arrow", 0x40, 8) as usize;
    inv.equip_stack(s);
    inv.drain_events();
    let ev = inv.drop_items_from_stack(s, 3).unwrap();
    assert_eq!(
        ev,
        InventoryEvent::Dropped {
            template: "It_Arrow".into(),
            amount: 3,
            quality: 0x40,
            from_slot: Slot::Ammo
        }
    );
    assert_eq!(inv.count("It_Arrow"), 5);
    assert_eq!(inv.stack(s).unwrap().slot, Slot::None);
    let all = inv.drop_slot(Slot::Ammo);
    assert!(all.is_none());
    let w = inv.create_items(&c, "It_1H_Sword", 0, 1) as usize;
    inv.equip_stack(w);
    assert!(inv.drop_slot(Slot::BackRight).is_some());
    assert_eq!(inv.count("It_1H_Sword"), 0);
    assert!(inv.drop_items_from_stack(0, 99).is_none());
}

fn npc_ctx(name: &str) -> TransferContext<'_> {
    TransferContext {
        dst_is_player: false,
        dst_name: name,
        dst_is_npc: true,
        trade_page_open: false,
    }
}

#[test]
fn transfer_moves_items_and_notifies_the_quest_manager() {
    let c = catalog();
    let mut a = Inventory::new();
    let mut b = Inventory::new();
    let p = a.create_items(&c, "It_Potion", 0x20, 5) as usize;
    let t = a
        .transfer_items_to(&mut b, p, 2, StackType::Normal, &TransferContext::default())
        .unwrap();
    assert_eq!(t.moved, 2);
    assert_eq!(t.notify, Some(("It_Potion".to_string(), 2)));
    assert_eq!((a.count("It_Potion"), b.count("It_Potion")), (3, 2));
    assert_eq!(b.stack(0).unwrap().quality, 0x20);
    // Whole remaining stack: the source stack disappears.
    a.transfer_stack_to(&mut b, 0, StackType::Normal, &TransferContext::default())
        .unwrap();
    assert!(a.is_empty());
    assert_eq!(b.count("It_Potion"), 5);
    // With the trade page open no quest notification is sent.
    let ctx = TransferContext {
        trade_page_open: true,
        ..TransferContext::default()
    };
    let t = b
        .transfer_items_to(&mut a, 0, 1, StackType::Normal, &ctx)
        .unwrap();
    assert!(t.notify.is_none());
    // Invalid requests change nothing.
    assert!(
        b.transfer_items_to(&mut a, 9, 1, StackType::Normal, &ctx)
            .is_none()
    );
    assert!(
        b.transfer_items_to(&mut a, 0, 0, StackType::Normal, &ctx)
            .is_none()
    );
}

#[test]
fn transferring_a_linked_stack_unlinks_it_first() {
    let c = catalog();
    let mut a = Inventory::new();
    let mut b = Inventory::new();
    let s = a.create_items(&c, "It_1H_Sword", 0, 1) as usize;
    let sh = a.create_items(&c, "It_1H_Axe", 0, 1) as usize;
    a.equip_stack(s);
    a.equip_stack(sh);
    assert_eq!(a.stack(sh).unwrap().slot, Slot::BackLeft);
    let t = a
        .transfer_stack_to(&mut b, sh, StackType::Normal, &TransferContext::default())
        .unwrap();
    assert_eq!(t.moved, 1);
    // A stack in BackLeft also frees BackRight (the original unlinks both).
    assert_eq!(a.stack(0).unwrap().slot, Slot::None);
    assert_eq!(b.count("It_1H_Axe"), 1);
}

#[test]
fn mission_items_vanish_when_given_to_npcs() {
    let c = catalog();
    let mut a = Inventory::new();
    let mut b = Inventory::new();
    let k = a.create_items(&c, "It_Key", 0, 1) as usize;
    let t = a
        .transfer_items_to(&mut b, k, 1, StackType::Normal, &npc_ctx("Bartok"))
        .unwrap();
    assert!(t.vanished);
    assert_eq!(b.count("It_Key"), 0);
    let k = a.create_items(&c, "It_Key", 0, 1) as usize;
    let t = a
        .transfer_items_to(&mut b, k, 1, StackType::Normal, &npc_ctx("Tom"))
        .unwrap();
    assert!(!t.vanished);
    assert_eq!(b.count("It_Key"), 1);
}

#[test]
fn transaction_marks_drive_the_trade() {
    let c = catalog();
    let mut hero = Inventory::new();
    let mut trader = Inventory::new();
    hero.create_items(&c, "It_Potion", 0, 5);
    hero.create_items(&c, "It_Arrow", 0, 10);
    trader.create_items(&c, "It_Shield", 0, 1);
    hero.set_transaction_amount(0, 9);
    assert_eq!(hero.transaction_amount(0), 5);
    hero.set_transaction_amount(0, 2);
    trader.set_transaction_amount(0, 1);
    let ctx = npc_ctx("Trader");
    let hero_ctx = TransferContext::default();
    hero.perform_transaction(
        &mut trader,
        StackType::Normal,
        StackType::Merchandise,
        &ctx,
        &hero_ctx,
    );
    assert_eq!(hero.count("It_Potion"), 3);
    assert_eq!(hero.count("It_Shield"), 1);
    assert_eq!(hero.count("It_Arrow"), 10);
    // The trader holds the sold potions as merchandise, which does not count as owned.
    assert_eq!(trader.count("It_Potion"), 0);
    let merch = trader
        .stacks()
        .iter()
        .find(|s| s.template == "It_Potion")
        .unwrap();
    assert_eq!((merch.amount, merch.kind), (2, StackType::Merchandise));
    assert!(hero.stacks().iter().all(|s| s.transaction == 0));
}

#[test]
fn transfer_all_moves_everything() {
    let c = catalog();
    let mut a = Inventory::new();
    let mut b = Inventory::new();
    a.create_items(&c, "It_Potion", 0, 2);
    a.create_items(&c, "It_Arrow", 0, 4);
    let s = a.create_items(&c, "It_1H_Sword", 0, 1) as usize;
    a.equip_stack(s);
    assert!(a.transfer_all_to(&mut b, StackType::Normal, &TransferContext::default()));
    assert!(a.is_empty());
    assert_eq!(
        b.count("It_Arrow") + b.count("It_Potion") + b.count("It_1H_Sword"),
        7
    );
}

#[test]
fn skill_stacks_learn_and_activate() {
    let c = catalog();
    let mut inv = Inventory::new();
    let p = inv.create_items(&c, "It_Perk_Sneak", 0, 1) as usize;
    assert_eq!(inv.find_skill_stack_index("skill_sneak"), Some(p));
    assert!(!inv.is_skill_active("Skill_Sneak"));
    assert!(inv.learn_skill("Skill_Sneak"));
    assert!(inv.is_skill_active("Skill_Sneak") && inv.has_learned_skill("Skill_Sneak"));
    assert!(inv.activate_skill("Skill_Sneak"));
    assert_eq!(inv.skill_activation_count("Skill_Sneak"), 1);
    assert!(inv.deactivate_skill("Skill_Sneak"));
    assert!(!inv.activate_skill("Skill_Unknown"));
    assert!(inv.stack(p).unwrap().shows_normal_icon());
}

#[test]
fn can_learn_checks_prerequisites_and_attributes() {
    let c = catalog();
    let mut inv = Inventory::new();
    inv.create_items(&c, "It_Perk_Sneak", 0, 1);
    let req = LearnReq {
        prerequisite: Some("Skill_Sneak".into()),
        attribs: vec![("STR".into(), 30), ("DEX".into(), 20)],
    };
    let attr = |t: &str| Some(if t == "STR" { 35 } else { 10 });
    let r = inv.can_learn_skill(Some(&req), None, attr);
    assert!(!r.ok);
    assert_eq!(r.missing_prerequisites, vec!["Skill_Sneak".to_string()]);
    assert_eq!(r.low_attribs, vec![("DEX".to_string(), 20)]);
    inv.learn_skill("Skill_Sneak");
    let r = inv.can_learn_skill(Some(&req), None, |_| Some(99));
    assert!(r.ok);
    // An attribute the owner does not have aborts.
    assert!(!inv.can_learn_skill(Some(&req), None, |_| None).ok);
}

#[test]
fn count_view_behaves_like_the_old_item_map() {
    let c = catalog();
    let mut inv = Inventory::new();
    inv.insert("It_Potion", 3);
    inv.insert("it_potion", 5);
    assert_eq!(inv.get("IT_POTION"), Some(&5));
    inv.insert("It_Potion", 2);
    assert_eq!(inv.count("It_Potion"), 2);
    inv.insert("It_Potion", 0);
    assert!(inv.get("It_Potion").is_none() && inv.is_empty());
    inv.set_count(&c, "It_Arrow", 4);
    assert_eq!(inv.keys().collect::<Vec<_>>(), vec!["it_arrow"]);
    assert_eq!(inv.values().sum::<u32>(), 4);
    inv.remove("It_Arrow");
    assert_eq!(inv.distinct(), 0);
}

#[test]
fn merge_and_split_through_the_inventory() {
    let c = catalog();
    let mut inv = Inventory::new();
    let a = inv.create_items(&c, "It_Potion", 0, 5) as usize;
    let n = inv.split_stack(a, 2).unwrap();
    assert_eq!(
        (inv.stack(a).unwrap().amount, inv.stack(n).unwrap().amount),
        (3, 2)
    );
    assert_eq!(inv.count("It_Potion"), 5);
    assert!(inv.merge_stacks(a, n));
    assert_eq!(inv.len(), 1);
    assert_eq!(inv.stack(0).unwrap().amount, 5);
    assert!(inv.split_stack(0, 5).is_none());
    assert!(inv.set_stack_amount(0, 0));
    assert!(inv.is_empty());
}

#[test]
fn hot_key_lookup_and_sort_keeps_links() {
    let c = catalog();
    let mut inv = Inventory::new();
    let p = inv.create_items(&c, "It_Potion", 0, 1) as usize;
    let s = inv.create_items(&c, "It_1H_Sword", 0, 1) as usize;
    inv.set_stack_hot_key(p, 3);
    assert_eq!(inv.find_stack_index_by_hot_key(3), Some(p));
    assert_eq!(inv.find_stack_index_by_hot_key(-1), None);
    inv.equip_stack(s);
    inv.sort(&PlainOrder);
    // Weapons come before alchemy.
    assert_eq!(inv.stack(0).unwrap().template, "It_1H_Sword");
    assert_eq!(inv.find_stack_index_by_slot(Slot::BackRight), Some(0));
}

#[test]
fn events_describe_creation_linking_and_deletion() {
    let c = catalog();
    let mut inv = Inventory::new();
    let s = inv.create_items(&c, "It_1H_Sword", 0, 1) as usize;
    inv.equip_stack(s);
    inv.unequip_stack(s);
    inv.remove_items(s, 1);
    let ev = inv.drain_events();
    assert!(matches!(ev[0], InventoryEvent::StackCreated(0)));
    assert!(ev.iter().any(|e| matches!(
        e,
        InventoryEvent::Linked {
            slot: Slot::BackRight,
            use_type: 2,
            ..
        }
    )));
    assert!(ev.iter().any(|e| matches!(
        e,
        InventoryEvent::Unlinked {
            slot: Slot::BackRight,
            ..
        }
    )));
    assert!(matches!(ev.last(), Some(InventoryEvent::StackDeleted(0))));
}

#[test]
fn random_transfer_sequences_conserve_items() {
    // Deterministic pseudo random walk over transfers between three inventories.
    let c = catalog();
    let mut inv = [Inventory::new(), Inventory::new(), Inventory::new()];
    inv[0].create_items(&c, "It_Potion", 0, 40);
    inv[0].create_items(&c, "It_Arrow", 0, 33);
    inv[0].create_items(&c, "It_Potion", 0x20, 7);
    let total = |inv: &[Inventory; 3]| -> u32 {
        inv.iter()
            .map(|i| i.stacks().iter().map(|s| s.amount as u32).sum::<u32>())
            .sum()
    };
    let start = total(&inv);
    let mut x: u64 = 0x2545F4914F6CDD1D;
    for _ in 0..500 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let (from, to) = ((x % 3) as usize, ((x >> 8) % 3) as usize);
        if from == to || inv[from].is_empty() {
            continue;
        }
        let idx = ((x >> 16) as usize) % inv[from].len();
        let n = 1 + ((x >> 24) as i32 % inv[from].stack(idx).unwrap().amount);
        let (a, b) = if from < to {
            let (l, r) = inv.split_at_mut(to);
            (&mut l[from], &mut r[0])
        } else {
            let (l, r) = inv.split_at_mut(from);
            (&mut r[0], &mut l[to])
        };
        a.transfer_items_to(b, idx, n, StackType::Normal, &TransferContext::default());
        assert_eq!(total(&inv), start);
    }
    // Same-quality stacks always merged: at most two stacks per template/quality.
    for i in &inv {
        let mut keys: Vec<_> = i
            .stacks()
            .iter()
            .map(|s| (s.template.clone(), s.quality))
            .collect();
        let n = keys.len();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), n);
    }
}
