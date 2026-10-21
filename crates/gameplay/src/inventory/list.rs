//! The inventory itself: `gCInventory_PS` plus its stack list (`gCInventoryStackList`).
//!
//! All state lives here; side effects on the world (spawning or attaching item entities,
//! applying attribute modifiers, telling the quest manager) are reported as
//! [`InventoryEvent`]s that the host drains with [`Inventory::drain_events`].
use super::item::{ItemCatalog, ItemInfo, StatMod};
use super::slot::{SLOT_COUNT, Slot, equip_slots, use_type};
use super::stack::{QUALITY_DAMAGED, Stack, StackType};
use std::collections::BTreeMap;
use std::ops::Index;
use std::sync::Arc;

/// `gEInventoryFindQuality`: how `find_stack_index*` compares the quality bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindQuality {
    /// First stack of the template, any quality.
    Any = 0,
    /// First stack whose quality equals the given value.
    Exact = 1,
    /// Stack with the numerically highest quality (the last one on ties).
    Best = 2,
    /// Stack with the lowest quality (the last one on ties).
    Worst = 3,
}

/// How an attribute modifier event has to be applied by the host's attribute store
/// (`gCPlayerMemory_PS::ApplyMod` / `UnapplyMod` / `Consume`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatKind {
    Apply,
    Unapply,
    Consume,
}

/// Something the host has to mirror in the world.
#[derive(Debug, Clone, PartialEq)]
pub enum InventoryEvent {
    /// `OnStackCreate(index)`.
    StackCreated(usize),
    /// `OnStackChange(index)` (amount, quality, learned, transaction ...).
    StackChanged(usize),
    /// `OnStackDelete(index)`; the index is valid until the next structural change.
    StackDeleted(usize),
    /// The item of a stack is attached to the character in `slot` (`AttachItemToSekelton`).
    Linked {
        slot: Slot,
        template: String,
        use_type: u32,
    },
    /// The item left `slot` (`DetachItemFromSkeleton`).
    Unlinked { slot: Slot, template: String },
    /// A slot's default item (hair, head, body, fist ...) was spawned and attached.
    DefaultLinked { slot: Slot, template: String },
    /// A slot's default item was removed again.
    DefaultUnlinked { slot: Slot, template: String },
    /// Attribute modifier from `OnApplyStats`/`OnUnApplyStats`/`ConsumeItems`.
    Stat {
        kind: StatKind,
        tag: String,
        op: i32,
        value: i32,
    },
    /// A skill became active because of equipment or consumption (HUD message `FO_It_<name>`).
    SkillActivated(String),
    /// `gCQuestManager_PS::OnReceiveItem` (`gained` = items arrived, otherwise they left).
    QuestItems {
        template: String,
        amount: i32,
        gained: bool,
    },
    /// Items left the inventory into the world (`gCInventoryStack::Drop`).
    Dropped {
        template: String,
        amount: i32,
        quality: u32,
        from_slot: Slot,
    },
}

/// Default item of one slot (`gCInventorySlot` that is not a stack).
#[derive(Debug, Clone)]
pub struct DefaultItem {
    pub template: String,
    pub info: Option<Arc<ItemInfo>>,
    /// Currently spawned and attached.
    pub linked: bool,
}

/// Result of an item transfer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Transfer {
    pub moved: i32,
    /// The mission item was removed without reaching the target (NPC receivers).
    pub vanished: bool,
    /// The quest manager has to be told (`NotifyQuestManager`): template and amount.
    pub notify: Option<(String, i32)>,
    /// The template is a `Book_CPT*` book: the host has to run `OnPlayerGetCPTBook`-like
    /// handling.
    pub book_cpt: bool,
}

/// Facts about the receiving inventory's owner that `TransferItemsTo` depends on.
#[derive(Debug, Clone, Copy)]
pub struct TransferContext<'a> {
    pub dst_is_player: bool,
    pub dst_name: &'a str,
    /// The receiver has a navigation property set (it is an NPC).
    pub dst_is_npc: bool,
    /// The trade page (HUD page 5) is open: no quest notification then.
    pub trade_page_open: bool,
}

impl Default for TransferContext<'_> {
    fn default() -> Self {
        Self {
            dst_is_player: true,
            dst_name: "",
            dst_is_npc: false,
            trade_page_open: false,
        }
    }
}

/// Why `can_equip` refuses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EquipCheck {
    pub ok: bool,
    /// Missing attribute points per tag.
    pub missing_attribs: Vec<(String, i32)>,
    pub missing_skills: Vec<String>,
}

/// Prerequisites of a skill or spell (`gCSkill_PS` / `gCMagic_PS` requirement fields).
#[derive(Debug, Clone, Default)]
pub struct LearnReq {
    /// Skill or spell template that has to be learned first.
    pub prerequisite: Option<String>,
    /// Attribute tag and minimal value (up to four).
    pub attribs: Vec<(String, i32)>,
}

/// Result of `can_learn_skill`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LearnCheck {
    pub ok: bool,
    /// Prerequisite skills / spells that are missing (HUD message `FO_It_<name>`).
    pub missing_prerequisites: Vec<String>,
    /// Attributes that are too low: tag and required value.
    pub low_attribs: Vec<(String, i32)>,
}

/// What `consume_items` asks the host to do.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Consumed {
    pub ok: bool,
    /// Skills that were activated by the consumed item.
    pub activated: Vec<String>,
}

/// A character's items: stacks, per-slot default items and generation flags.
#[derive(Debug, Clone)]
pub struct Inventory {
    stacks: Vec<Stack>,
    defaults: Vec<Option<DefaultItem>>,
    /// Treasure set entity names (`TreasureSet1..5`).
    pub treasure_sets: [String; 5],
    pub generated_plunder: bool,
    pub generated_trade: bool,
    /// Apply attribute modifiers / skill activations of linked items (an owner without
    /// a player-memory property set has none).
    pub stats_enabled: bool,
    events: Vec<InventoryEvent>,
    counts: BTreeMap<String, u32>,
}

impl Default for Inventory {
    fn default() -> Self {
        Self {
            stacks: Vec::new(),
            defaults: vec![None; SLOT_COUNT],
            treasure_sets: Default::default(),
            generated_plunder: false,
            generated_trade: false,
            stats_enabled: true,
            events: Vec::new(),
            counts: BTreeMap::new(),
        }
    }
}

impl PartialEq for Inventory {
    fn eq(&self, o: &Inventory) -> bool {
        self.stacks == o.stacks
            && self.treasure_sets == o.treasure_sets
            && self.generated_plunder == o.generated_plunder
            && self.generated_trade == o.generated_trade
            && self.defaults.len() == o.defaults.len()
            && self
                .defaults
                .iter()
                .zip(&o.defaults)
                .all(|(a, b)| match (a, b) {
                    (None, None) => true,
                    (Some(a), Some(b)) => a.template == b.template && a.linked == b.linked,
                    _ => false,
                })
    }
}
impl Eq for Inventory {}

fn eq(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

impl Inventory {
    pub fn new() -> Self {
        Self::default()
    }

    // ---- basic access ------------------------------------------------------------------

    pub fn stacks(&self) -> &[Stack] {
        &self.stacks
    }

    pub fn stack(&self, idx: usize) -> Option<&Stack> {
        self.stacks.get(idx)
    }

    pub fn len(&self) -> usize {
        self.stacks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.stacks.is_empty()
    }

    pub fn drain_events(&mut self) -> Vec<InventoryEvent> {
        std::mem::take(&mut self.events)
    }

    /// Remove and return only the attribute modifier events (`kind`, tag, operation, value),
    /// leaving the stack and link events for the host that mirrors them.
    pub fn take_stat_events(&mut self) -> Vec<(StatKind, String, i32, i32)> {
        let mut out = Vec::new();
        self.events.retain(|e| match e {
            InventoryEvent::Stat {
                kind,
                tag,
                op,
                value,
            } => {
                out.push((*kind, tag.clone(), *op, *value));
                false
            }
            _ => true,
        });
        out
    }

    fn rebuild_counts(&mut self) {
        self.counts.clear();
        for s in &self.stacks {
            if s.kind != StackType::Merchandise && s.amount > 0 {
                *self
                    .counts
                    .entry(s.template.to_ascii_lowercase())
                    .or_insert(0) += s.amount as u32;
            }
        }
    }

    fn changed(&mut self, idx: usize) {
        self.events.push(InventoryEvent::StackChanged(idx));
        self.rebuild_counts();
    }

    /// Item count by template (non-merchandise stacks), 0 when absent.
    pub fn count(&self, template: &str) -> u32 {
        self.counts
            .get(&template.to_ascii_lowercase())
            .copied()
            .unwrap_or(0)
    }

    /// `PSInventory::HasItems(template, count)`.
    pub fn has_items(&self, template: &str, count: u32) -> bool {
        self.count(template) >= count
    }

    /// Rebuild an inventory from saved parts (see `save.rs`); template data stays unresolved
    /// until [`Inventory::rebind`].
    pub(crate) fn restore(
        &mut self,
        stacks: Vec<Stack>,
        defaults: Vec<Option<DefaultItem>>,
        treasure_sets: Vec<String>,
        generated_plunder: bool,
        generated_trade: bool,
    ) {
        self.stacks = stacks;
        self.defaults = defaults;
        for (i, t) in treasure_sets.into_iter().take(5).enumerate() {
            self.treasure_sets[i] = t;
        }
        self.generated_plunder = generated_plunder;
        self.generated_trade = generated_trade;
        self.rebuild_counts();
    }

    /// Re-resolve all template data after loading a save or swapping catalogs.
    pub fn rebind(&mut self, cat: &dyn ItemCatalog) {
        for s in &mut self.stacks {
            s.info = cat.item(&s.template);
        }
        for d in self.defaults.iter_mut().flatten() {
            d.info = cat.item(&d.template);
        }
    }

    // ---- map-like read view of the counts ----------------------------------------------

    pub fn get(&self, template: &str) -> Option<&u32> {
        self.counts.get(&template.to_ascii_lowercase())
    }

    pub fn contains_key(&self, template: &str) -> bool {
        self.get(template).is_some()
    }

    pub fn keys(&self) -> std::collections::btree_map::Keys<'_, String, u32> {
        self.counts.keys()
    }

    pub fn values(&self) -> std::collections::btree_map::Values<'_, String, u32> {
        self.counts.values()
    }

    pub fn iter(&self) -> std::collections::btree_map::Iter<'_, String, u32> {
        self.counts.iter()
    }

    /// Number of distinct templates held.
    pub fn distinct(&self) -> usize {
        self.counts.len()
    }

    /// Add `n` items of a template that the host has no data for (counts only); a later
    /// [`Inventory::rebind`] resolves the template.
    pub fn add(&mut self, template: &str, n: u32) {
        if n > 0 {
            self.add_items(
                template,
                None,
                n.min(i32::MAX as u32) as i32,
                0,
                StackType::Normal,
            );
        }
    }

    /// Remove `n` items of `template` from unlinked stacks; false (and nothing changes) when
    /// fewer than `n` are held.
    pub fn take(&mut self, template: &str, n: u32) -> bool {
        if self.count(template) < n {
            return false;
        }
        self.remove_by_template(template, n);
        true
    }

    /// Set the held amount of a template (legacy count semantics): adds a stack or removes
    /// unlinked items, without a catalog.
    pub fn insert(&mut self, template: &str, n: u32) {
        self.set_count(&super::item::NoCatalog, template, n);
    }

    /// Set the held amount of `template` to `n`, resolving new stacks through `cat`.
    pub fn set_count(&mut self, cat: &dyn ItemCatalog, template: &str, n: u32) {
        let cur = self.count(template);
        if n > cur {
            self.create_items(cat, template, 0, (n - cur) as i32);
        } else if n < cur {
            self.remove_by_template(template, cur - n);
        }
    }

    /// Remove up to `n` items of `template` from unlinked stacks, last stacks first.
    /// Returns how many were removed.
    pub fn remove_by_template(&mut self, template: &str, n: u32) -> u32 {
        let n = n.min(i32::MAX as u32);
        let mut left = n as i32;
        let mut i = self.stacks.len();
        while i > 0 && left > 0 {
            i -= 1;
            let s = &self.stacks[i];
            if !eq(&s.template, template) || s.kind == StackType::Merchandise || s.is_linked() {
                continue;
            }
            let take = left.min(s.amount);
            if take == s.amount {
                self.delete_stack(i);
            } else {
                self.stacks[i].amount -= take;
                self.changed(i);
            }
            left -= take;
        }
        (n as i32 - left) as u32
    }

    /// Delete every stack that is not linked to a slot (`CleanUpPlunderInv`).
    pub fn delete_unlinked(&mut self) {
        for i in (0..self.stacks.len()).rev() {
            if self.stacks[i].is_disposable() {
                self.delete_stack(i);
            }
        }
    }

    pub fn remove(&mut self, template: &str) {
        self.remove_by_template(template, u32::MAX);
    }

    pub fn clear(&mut self) {
        for i in (0..self.stacks.len()).rev() {
            self.force_unlink(i);
            self.events.push(InventoryEvent::StackDeleted(i));
        }
        self.stacks.clear();
        self.rebuild_counts();
    }

    // ---- stack list --------------------------------------------------------------------

    /// Port of the stack list's AddItems (Game:0x100c3700): add `amount` items; they join a
    /// stack of the same template, quality and stack type that is either free (unlinked and
    /// without quick slot) or linked in a slot where one item per stack is not required
    /// (ammunition); otherwise a new stack is created. Returns the stack index, -1 for
    /// `amount <= 0` or an unknown template without name.
    pub fn add_items(
        &mut self,
        template: &str,
        info: Option<Arc<ItemInfo>>,
        amount: i32,
        quality: u32,
        kind: StackType,
    ) -> i32 {
        if template.is_empty() || amount <= 0 {
            return -1;
        }
        for i in 0..self.stacks.len() {
            let s = &self.stacks[i];
            let joinable =
                (!s.is_linked() && s.quick_slot < 0) || !s.must_split_before_linking(s.slot);
            if eq(&s.template, template) && s.quality == quality && s.kind == kind && joinable {
                self.stacks[i].amount += amount;
                self.changed(i);
                return i as i32;
            }
        }
        let mut s = Stack::new(template, info);
        s.amount = amount;
        s.quality = quality;
        s.kind = kind;
        self.stacks.push(s);
        let idx = self.stacks.len() - 1;
        self.events.push(InventoryEvent::StackCreated(idx));
        self.rebuild_counts();
        idx as i32
    }

    /// Port of gCInventory_PS::CreateItems (Game:0x100b5c60) resolving the template via `cat`.
    pub fn create_items(
        &mut self,
        cat: &dyn ItemCatalog,
        template: &str,
        quality: u32,
        amount: i32,
    ) -> i32 {
        self.add_items(
            template,
            cat.item(template),
            amount,
            quality,
            StackType::Normal,
        )
    }

    /// Port of the pickup path (FUN_100c4180): a pickup entity's amount and quality are added
    /// to the inventory.
    pub fn pick_up(
        &mut self,
        cat: &dyn ItemCatalog,
        template: &str,
        amount: i32,
        quality: u32,
    ) -> i32 {
        self.add_items(
            template,
            cat.item(template),
            amount,
            quality,
            StackType::Normal,
        )
    }

    /// Port of gCInventory_PS::AssureItems (Game:0x100b5cc0): make sure the inventory holds
    /// at least `count` items of `template` with exactly `quality` (merchandise stacks do not
    /// count). Missing items are created. Returns the index of the first matching stack,
    /// preferring an unlinked one; for a freshly created stack its index.
    pub fn assure_items(
        &mut self,
        cat: &dyn ItemCatalog,
        template: &str,
        quality: u32,
        count: i32,
    ) -> i32 {
        if template.is_empty() {
            return -1;
        }
        let mut have = 0;
        let mut first: i32 = -1;
        for (i, s) in self.stacks.iter().enumerate() {
            if !eq(&s.template, template)
                || s.quality != quality
                || s.kind == StackType::Merchandise
            {
                continue;
            }
            if first < 0 || !s.is_linked() {
                first = i as i32;
            }
            have += s.amount;
        }
        if have >= count {
            return first;
        }
        self.create_items(cat, template, quality, count - have)
    }

    /// Port of the stack list's FindStackIndex by template (Game:0x100c3ac0): searches after
    /// `after` (-1 starts at the beginning). `quality` is used by `Exact`.
    pub fn find_stack_index(
        &self,
        template: &str,
        mode: FindQuality,
        quality: u32,
        after: i32,
    ) -> i32 {
        self.find_by(|s| eq(&s.template, template), mode, quality, after)
    }

    /// Port of the stack list's FindStackIndex by use type (Game:0x100c39b0).
    pub fn find_stack_index_by_use_type(
        &self,
        use_type: u32,
        mode: FindQuality,
        quality: u32,
        after: i32,
    ) -> i32 {
        self.find_by(|s| s.use_type() == use_type, mode, quality, after)
    }

    fn find_by(
        &self,
        pred: impl Fn(&Stack) -> bool,
        mode: FindQuality,
        quality: u32,
        after: i32,
    ) -> i32 {
        let mut best: i32 = -1;
        let mut best_q: i64 = if mode == FindQuality::Worst {
            i64::MAX
        } else {
            0
        };
        let start = (after + 1).max(0) as usize;
        for (i, s) in self.stacks.iter().enumerate().skip(start) {
            if !pred(s) {
                continue;
            }
            let q = s.quality as i64;
            match mode {
                FindQuality::Any => return i as i32,
                FindQuality::Exact => {
                    if s.quality == quality {
                        return i as i32;
                    }
                }
                FindQuality::Best => {
                    if best_q <= q {
                        best_q = q;
                        best = i as i32;
                    }
                }
                FindQuality::Worst => {
                    if q <= best_q {
                        best_q = q;
                        best = i as i32;
                    }
                }
            }
        }
        best
    }

    /// Port of gCInventory_PS::FindStackIndex(gESlot): index of the stack linked in `slot`.
    pub fn find_stack_index_by_slot(&self, slot: Slot) -> Option<usize> {
        if slot == Slot::None {
            return None;
        }
        self.stacks.iter().position(|s| s.slot == slot)
    }

    /// Port of gCInventory_PS::FindStack(gESlot).
    pub fn find_stack(&self, slot: Slot) -> Option<&Stack> {
        self.find_stack_index_by_slot(slot).map(|i| &self.stacks[i])
    }

    /// Port of gCInventory_PS::FindStackIndexByHotKey (Game:0x100b5900): quick slot lookup.
    pub fn find_stack_index_by_hot_key(&self, key: i32) -> Option<usize> {
        if key < 0 {
            return None;
        }
        self.stacks.iter().position(|s| s.quick_slot == key)
    }

    /// Port of gCInventory_PS::GetStackUseType (Game:0x100b5a90), 0 for an invalid index.
    pub fn stack_use_type(&self, idx: usize) -> u32 {
        self.stacks.get(idx).map_or(0, Stack::use_type)
    }

    /// Port of gCInventory_PS::GetStackCategory (Game:0x100b5a70).
    pub fn stack_category(&self, idx: usize) -> super::item::Category {
        self.stacks
            .get(idx)
            .map_or(Default::default(), Stack::category)
    }

    /// Port of gCInventory_PS::SetStackAmount (Game:0x100b5b40). Amount 0 disposes the stack
    /// (only if it is unlinked).
    pub fn set_stack_amount(&mut self, idx: usize, amount: i32) -> bool {
        if amount == 0 {
            return self.dispose_stack(idx);
        }
        if idx >= self.stacks.len() {
            return false;
        }
        self.stacks[idx].amount = amount;
        self.changed(idx);
        true
    }

    /// Port of gCInventory_PS::SetStackQuality (Game:0x100b5ab0).
    pub fn set_stack_quality(&mut self, idx: usize, quality: u32) -> bool {
        if idx >= self.stacks.len() {
            return false;
        }
        self.stacks[idx].quality = quality;
        self.changed(idx);
        true
    }

    /// Set the `QuickSlot` of a stack (`SetStackHotKey`).
    pub fn set_stack_hot_key(&mut self, idx: usize, key: i32) -> bool {
        if idx >= self.stacks.len() {
            return false;
        }
        self.stacks[idx].quick_slot = key;
        self.changed(idx);
        true
    }

    /// Port of the stack list's DisposeStack (Game:0x100c3c20): delete only unlinked stacks.
    pub fn dispose_stack(&mut self, idx: usize) -> bool {
        match self.stacks.get(idx) {
            Some(s) if s.is_disposable() => {
                self.delete_stack(idx);
                true
            }
            _ => false,
        }
    }

    /// Port of the stack list's DeleteStack (Game:0x100c3dd0): remove the stack, shifting the
    /// following indices down.
    pub fn delete_stack(&mut self, idx: usize) -> bool {
        if idx >= self.stacks.len() {
            return false;
        }
        self.force_unlink(idx);
        self.events.push(InventoryEvent::StackDeleted(idx));
        self.stacks.remove(idx);
        self.rebuild_counts();
        true
    }

    /// Port of the stack list's DisposeItems (Game:0x100c38e0): remove `n` items. Permanent
    /// items (skills, spells) are never reduced but the call succeeds; removing the whole
    /// stack needs it to be unlinked.
    pub fn remove_items(&mut self, idx: usize, n: i32) -> bool {
        let Some(s) = self.stacks.get(idx) else {
            return false;
        };
        if n < 1 || s.amount < n {
            return false;
        }
        if s.is_permanent() {
            return true;
        }
        if s.amount == n {
            return self.dispose_stack(idx);
        }
        self.stacks[idx].amount -= n;
        self.changed(idx);
        true
    }

    /// Port of gCInventory_PS::MergeStacks: merge stack `from` into `into`.
    pub fn merge_stacks(&mut self, into: usize, from: usize) -> bool {
        if into == from || into >= self.stacks.len() || from >= self.stacks.len() {
            return false;
        }
        let mut other = self.stacks[from].clone();
        if !self.stacks[into].merge(&mut other) {
            return false;
        }
        self.changed(into);
        self.delete_stack(from);
        true
    }

    /// Port of gCInventory_PS::SplitStack: cut `n` items into a new stack.
    pub fn split_stack(&mut self, idx: usize, n: i32) -> Option<usize> {
        let new = self.stacks.get_mut(idx)?.split(n)?;
        self.changed(idx);
        self.stacks.push(new);
        let i = self.stacks.len() - 1;
        self.events.push(InventoryEvent::StackCreated(i));
        self.rebuild_counts();
        Some(i)
    }

    /// Sort the stacks with [`super::order::compare`] (`OnSortInventory`); linked stacks keep
    /// their links because links live in the stacks.
    pub fn sort(&mut self, cx: &dyn super::order::OrderContext) {
        super::order::sort_stacks(&mut self.stacks, cx);
        self.rebuild_counts();
    }

    // ---- default items -----------------------------------------------------------------

    /// Port of gCInventory_PS::SetDefaultItem (Game:0x100b5480): declare the default item of
    /// a slot (template inventories only).
    pub fn set_default_item(&mut self, slot: Slot, template: &str, info: Option<Arc<ItemInfo>>) {
        if slot == Slot::None {
            return;
        }
        self.unlink_default_item_from_slot(slot);
        self.defaults[slot.index()] = (!template.is_empty()).then(|| DefaultItem {
            template: template.to_string(),
            info,
            linked: false,
        });
        if self.find_stack_index_by_slot(slot).is_none() {
            self.link_default(slot);
        }
    }

    /// Port of gCInventory_PS::GetDefaultItem (Game:0x100b5420): template name or empty.
    pub fn default_item(&self, slot: Slot) -> &str {
        self.defaults[slot.index()]
            .as_ref()
            .map_or("", |d| d.template.as_str())
    }

    pub fn default_slot(&self, slot: Slot) -> Option<&DefaultItem> {
        self.defaults[slot.index()].as_ref()
    }

    fn link_default(&mut self, slot: Slot) -> bool {
        let Some(Some(d)) = self.defaults.get_mut(slot.index()) else {
            return false;
        };
        if d.linked {
            return true;
        }
        d.linked = true;
        let t = d.template.clone();
        self.events
            .push(InventoryEvent::DefaultLinked { slot, template: t });
        false
    }

    /// Port of gCInventory_PS::LinkDefaultItemToSlot (Game:0x100b5620): spawn the
    /// slot's default item when no stack occupies the slot.
    pub fn link_default_item_to_slot(&mut self, slot: Slot) -> bool {
        if self.find_stack_index_by_slot(slot).is_some() {
            return false;
        }
        self.link_default(slot)
    }

    /// Port of gCInventory_PS::UnlinkDefaultItemFromSlot (Game:0x100b5660): remove the slot's
    /// default item unless a stack occupies the slot. True when there was nothing to do.
    pub fn unlink_default_item_from_slot(&mut self, slot: Slot) -> bool {
        if self.find_stack_index_by_slot(slot).is_some() {
            return false;
        }
        match self.defaults.get_mut(slot.index()) {
            Some(Some(d)) if d.linked => {
                d.linked = false;
                let t = d.template.clone();
                self.events
                    .push(InventoryEvent::DefaultUnlinked { slot, template: t });
                false
            }
            _ => true,
        }
    }

    /// Port of gCInventory_PS::LinkDefaultItems (Game:0x100b5530): for slots 1..=18 without a
    /// stack, spawn the default item.
    pub fn link_default_items(&mut self) {
        for i in 1..SLOT_COUNT {
            let slot = Slot::ALL[i];
            if self.find_stack_index_by_slot(slot).is_none() {
                self.link_default(slot);
            }
        }
    }

    /// Port of gCInventory_PS::UnlinkDefaultItems (Game:0x100b55e0).
    pub fn unlink_default_items(&mut self) {
        for i in 1..SLOT_COUNT {
            let slot = Slot::ALL[i];
            if let Some(Some(d)) = self.defaults.get_mut(i)
                && d.linked
            {
                d.linked = false;
                let t = d.template.clone();
                self.events
                    .push(InventoryEvent::DefaultUnlinked { slot, template: t });
            }
        }
    }

    /// Port of gCInventory_PS::PatchDefaultItems / PatchTreasureSets (Game:0x100b56d0 /
    /// 0x100b7700) as run by `OnChildrenAvailable` for an entity instance: take the default
    /// items and treasure sets of the creating template, then link the defaults.
    pub fn patch_from_template(&mut self, template: &Inventory) {
        self.unlink_default_items();
        for (i, d) in template.defaults.iter().enumerate() {
            self.defaults[i] = d.as_ref().map(|d| DefaultItem {
                linked: false,
                ..d.clone()
            });
        }
        self.treasure_sets = template.treasure_sets.clone();
        self.link_default_items();
    }

    // ---- linking and equipping ---------------------------------------------------------

    /// Detach a stack from its slot without default relinking or stat changes beyond the
    /// bookkeeping (used by deletion).
    fn force_unlink(&mut self, idx: usize) {
        if self.stacks[idx].is_linked() {
            self.unlink_stack_at(idx);
        }
    }

    /// Port of gCInventorySlot::UnLinkFromSlot (Game:0x100bd570) on stack `idx`.
    fn unlink_stack_at(&mut self, idx: usize) {
        let slot = self.stacks[idx].slot;
        if self.stacks[idx].stats_applied {
            self.unapply_stats(idx);
        }
        let s = &mut self.stacks[idx];
        s.slot = Slot::None;
        let template = s.template.clone();
        self.events
            .push(InventoryEvent::Unlinked { slot, template });
        self.events.push(InventoryEvent::StackChanged(idx));
    }

    /// Port of gCInventory_PS::UnlinkStackFromSlot (Game:0x100b51f0): detach whatever stack
    /// sits in `slot`, then give the slot its default item back. Slot `None` does nothing.
    pub fn unlink_stack_from_slot(&mut self, slot: Slot) -> bool {
        if slot == Slot::None {
            return false;
        }
        let Some(idx) = self.find_stack_index_by_slot(slot) else {
            return false;
        };
        self.unlink_stack_at(idx);
        self.link_default_item_to_slot(slot);
        true
    }

    /// Port of gCInventory_PS::LinkStackToSlot (Game:0x100b5180) together with the stack
    /// list's LinkStack (Game:0x100c3f80) and gCInventorySlot::LinkToSlot (Game:0x100bd410).
    /// A stack that already sits in another slot moves; an occupied target slot refuses.
    /// `apply_stats` runs `OnApplyStats` for newly linked equipment.
    pub fn link_stack_to_slot(&mut self, idx: usize, slot: Slot, apply_stats: bool) -> bool {
        self.unlink_default_item_from_slot(slot);
        let ok = self.link_stack(idx, slot, apply_stats);
        if !ok && self.find_stack_index_by_slot(slot).is_none() {
            self.link_default(slot);
        }
        ok
    }

    fn link_stack(&mut self, idx: usize, slot: Slot, apply_stats: bool) -> bool {
        if idx >= self.stacks.len() || slot == Slot::None {
            return false;
        }
        match self.find_stack_index_by_slot(slot) {
            Some(i) if i == idx => return true,
            Some(_) => return false,
            None => {}
        }
        let old = self.stacks[idx].slot;
        if old != Slot::None {
            let template = self.stacks[idx].template.clone();
            self.events.push(InventoryEvent::Unlinked {
                slot: old,
                template,
            });
        }
        let s = &mut self.stacks[idx];
        s.slot = slot;
        let (template, use_type) = (s.template.clone(), s.use_type());
        self.events.push(InventoryEvent::Linked {
            slot,
            template,
            use_type,
        });
        if apply_stats && self.stats_enabled && !self.stacks[idx].stats_applied {
            self.apply_stats(idx);
        }
        self.events.push(InventoryEvent::StackChanged(idx));
        true
    }

    /// Port of gCInventory_PS::EquipStackToSlot (Game:0x100b80e0): put the stack into a
    /// specific slot, freeing the slot and the stack's old slot first.
    pub fn equip_stack_to_slot(&mut self, idx: usize, slot: Slot) -> bool {
        let Some(s) = self.stacks.get(idx) else {
            return false;
        };
        if s.category() == super::item::Category::None || s.use_type() == 0 {
            return false;
        }
        if !s.is_equipped_to_slot(slot) {
            let old = s.slot;
            self.unlink_stack_from_slot(slot);
            self.unlink_stack_from_slot(old);
            self.link_stack_to_slot(idx, slot, true);
        }
        true
    }

    /// Port of gCInventory_PS::EquipStack (Game:0x100b8160): equip an item into the carry
    /// slot its use type implies. A second one-hander (or one when the right hand holds a
    /// one-hander) takes the alternative slot; a left-side item also frees the left slots
    /// and, if the right hand holds a two-hander, the right hand.
    pub fn equip_stack(&mut self, idx: usize) -> bool {
        let Some(st) = self.stacks.get(idx) else {
            return false;
        };
        let use_ = st.use_type();
        if use_ == 0 {
            return false;
        }
        if st.is_equipped() {
            return true;
        }
        let (primary, alt) = st.equip_slots();
        if primary == Slot::None {
            return true;
        }
        let alt_taken = self.find_stack(alt).is_some();
        let prim_taken = self.find_stack(primary).is_some();
        let dual_one_hand = self
            .find_stack(Slot::RightHand)
            .is_some_and(|r| r.use_type() == use_type::ONE_H)
            && use_ == use_type::ONE_H;
        let mut target = primary;
        if (prim_taken || dual_one_hand) && alt != Slot::None && !alt_taken {
            target = alt;
        }
        if matches!(target, Slot::LeftHand | Slot::BackLeft) {
            self.unlink_stack_from_slot(Slot::LeftHand);
            self.unlink_stack_from_slot(Slot::BackLeft);
            let right_use = self.find_stack(Slot::RightHand).map(Stack::use_type);
            if right_use.is_some_and(use_type::is_two_handed) {
                self.unlink_stack_from_slot(Slot::RightHand);
            }
        } else {
            self.unlink_stack_from_slot(target);
        }
        self.link_stack_to_slot(idx, target, true);
        true
    }

    /// Port of gCInventory_PS::UnEquipStack (Game:0x100b8310): only stacks in their carry
    /// slot can be unequipped.
    pub fn unequip_stack(&mut self, idx: usize) -> bool {
        let Some(s) = self.stacks.get(idx) else {
            return false;
        };
        if !s.is_equipped() {
            return false;
        }
        let slot = s.slot;
        self.unlink_stack_from_slot(slot);
        true
    }

    /// Port of gCInventory_PS::GetRightEquipSlot (Game:0x100b85d0): where the right-hand
    /// weapon returns to.
    pub fn right_equip_slot(&self, idx: usize) -> Slot {
        match self.stack_use_type(idx) {
            use_type::ONE_H
            | use_type::TWO_H
            | use_type::STAFF
            | use_type::HALBERD
            | use_type::AXE => Slot::BackRight,
            use_type::ARROW | use_type::BOLT => Slot::Ammo,
            _ => Slot::None,
        }
    }

    /// Port of gCInventory_PS::GetLeftEquipSlot (Game:0x100b84c0): where the left-hand item
    /// returns to. A one-hander returns to `BackLeft` only if the right hand or the back
    /// right slot holds a one-hander too.
    pub fn left_equip_slot(&self, idx: usize) -> Slot {
        match self.stack_use_type(idx) {
            use_type::ONE_H => {
                let r = self.find_stack(Slot::RightHand).map(Stack::use_type);
                let b = self.find_stack(Slot::BackRight).map(Stack::use_type);
                if r == Some(use_type::ONE_H) || b == Some(use_type::ONE_H) {
                    Slot::BackLeft
                } else {
                    Slot::None
                }
            }
            use_type::BOW | use_type::CROSSBOW => Slot::Bow,
            use_type::SHIELD => Slot::BackLeft,
            _ => Slot::None,
        }
    }

    /// Port of gCInventory_PS::HoldStackRightHand (Game:0x100b8430): return the current
    /// right-hand item to its carry slot, then draw stack `idx` (-1: draw nothing, the slot
    /// falls back to its default item).
    pub fn hold_stack_right_hand(&mut self, idx: i32) -> bool {
        if let Some(cur) = self.find_stack_index_by_slot(Slot::RightHand) {
            let back = self.right_equip_slot(cur);
            self.equip_stack_to_slot(cur, back);
        }
        self.unlink_stack_from_slot(Slot::RightHand);
        if idx >= 0 {
            self.link_stack_to_slot(idx as usize, Slot::RightHand, true);
        } else {
            self.link_stack_to_slot(usize::MAX, Slot::RightHand, true);
        }
        true
    }

    /// Port of gCInventory_PS::HoldStackLeftHand (Game:0x100b8390).
    pub fn hold_stack_left_hand(&mut self, idx: i32) -> bool {
        let cur = self.find_stack_index_by_slot(Slot::LeftHand);
        if cur.map(|c| c as i32) != Some(idx) {
            if let Some(cur) = cur {
                let back = self.left_equip_slot(cur);
                self.equip_stack_to_slot(cur, back);
            }
            self.unlink_stack_from_slot(Slot::LeftHand);
            if idx >= 0 {
                self.link_stack_to_slot(idx as usize, Slot::LeftHand, false);
            } else {
                self.link_stack_to_slot(usize::MAX, Slot::LeftHand, false);
            }
        }
        true
    }

    /// Port of gCInventory_PS::HoldStacks (Game:0x100b8370): hold `right` and `left`, the
    /// right hand first. The original function declares its parameters as (left, right);
    /// the script layer calls this with the order of `AiCommand::HoldInventoryStacks`.
    pub fn hold_stacks(&mut self, right: i32, left: i32) -> bool {
        self.hold_stack_right_hand(right);
        self.hold_stack_left_hand(left);
        true
    }

    /// Is the stack worn: in its carry slot or held in a hand?
    pub fn is_stack_worn(&self, idx: usize) -> bool {
        self.stacks.get(idx).is_some_and(Stack::is_in_use)
    }

    // ---- dropping and transfers --------------------------------------------------------

    /// Port of gCInventoryStack::Drop (Game:0x100c06a0) plus gCInventory_PS::DropStack (Game:0x100b4460): unlink the
    /// stack and delete it, reporting what to spawn in the world.
    pub fn drop_stack(&mut self, idx: usize) -> Option<InventoryEvent> {
        let s = self.stacks.get(idx)?;
        let (template, amount, quality, from_slot) =
            (s.template.clone(), s.amount, s.quality, s.slot);
        self.force_unlink(idx);
        self.delete_stack(idx);
        if amount > 0 {
            self.events.push(InventoryEvent::QuestItems {
                template: template.clone(),
                amount,
                gained: false,
            });
        }
        let ev = InventoryEvent::Dropped {
            template,
            amount,
            quality,
            from_slot,
        };
        self.events.push(ev.clone());
        Some(ev)
    }

    /// Port of gCInventory_PS::DropSlot (Game:0x100b4530).
    pub fn drop_slot(&mut self, slot: Slot) -> Option<InventoryEvent> {
        let idx = self.find_stack_index_by_slot(slot)?;
        self.drop_stack(idx)
    }

    /// Port of gCInventory_PS::DropItemsFromStack (Game:0x100b4580): drop `n` items of a
    /// stack (the stack is unlinked from its slot).
    pub fn drop_items_from_stack(&mut self, idx: usize, n: i32) -> Option<InventoryEvent> {
        let s = self.stacks.get(idx)?;
        if n < 1 || s.amount < n {
            return None;
        }
        let (template, quality, from_slot) = (s.template.clone(), s.quality, s.slot);
        self.force_unlink(idx);
        self.remove_items(idx, n);
        self.events.push(InventoryEvent::QuestItems {
            template: template.clone(),
            amount: n,
            gained: false,
        });
        let ev = InventoryEvent::Dropped {
            template,
            amount: n,
            quality,
            from_slot,
        };
        self.events.push(ev.clone());
        Some(ev)
    }

    /// Port of gCInventory_PS::DropItemsFromSlot (Game:0x100b4630).
    pub fn drop_items_from_slot(&mut self, slot: Slot, n: i32) -> Option<InventoryEvent> {
        let idx = self.find_stack_index_by_slot(slot)?;
        self.drop_items_from_stack(idx, n)
    }

    /// Port of gCInventory_PS::TransferItemsTo (Game:0x100b6e20): move `n` items from stack
    /// `idx` into `dst` (as stack type `kind`). Moving the whole stack unlinks it first (a
    /// stack in `BackLeft` also frees `BackRight`); a stack that cannot be disposed refuses.
    /// Mission items given to NPCs vanish (except to the player, `Tom`, `Rathgar` and for the
    /// broken druid stone).
    pub fn transfer_items_to(
        &mut self,
        dst: &mut Inventory,
        idx: usize,
        n: i32,
        kind: StackType,
        ctx: &TransferContext,
    ) -> Option<Transfer> {
        if n < 1 {
            return None;
        }
        let s = self.stacks.get(idx)?;
        if s.amount <= n {
            let mut slot = s.slot;
            if slot != Slot::None {
                if slot == Slot::BackLeft {
                    self.unlink_stack_from_slot(Slot::BackLeft);
                    slot = Slot::BackRight;
                }
                self.unlink_stack_from_slot(slot);
            }
            if !self.stacks[idx].is_disposable() {
                return None;
            }
        }
        let s = &self.stacks[idx];
        let (template, info, quality) = (s.template.clone(), s.info.clone(), s.quality);
        let mission = info.as_deref().is_some_and(|i| i.mission_item);
        let vanish = mission
            && !ctx.dst_is_player
            && !ctx.dst_name.eq_ignore_ascii_case("Tom")
            && !ctx.dst_name.eq_ignore_ascii_case("Rathgar")
            && !eq(&template, "It_DruidStone_Ripper_Broken")
            && ctx.dst_is_npc;
        if !vanish {
            dst.add_items(&template, info, n, quality, kind);
        }
        self.stacks[idx].amount -= n;
        if self.stacks[idx].amount < 1 {
            self.dispose_stack(idx);
        } else {
            self.changed(idx);
        }
        let book_cpt = template.to_ascii_lowercase().contains("book_cpt");
        Some(Transfer {
            moved: n,
            vanished: vanish,
            notify: (!ctx.trade_page_open).then_some((template, n)),
            book_cpt,
        })
    }

    /// Port of gCInventory_PS::TransferStackTo (Game:0x100b6de0): move the whole stack.
    pub fn transfer_stack_to(
        &mut self,
        dst: &mut Inventory,
        idx: usize,
        kind: StackType,
        ctx: &TransferContext,
    ) -> Option<Transfer> {
        let n = self.stacks.get(idx)?.amount;
        self.transfer_items_to(dst, idx, n, kind, ctx)
    }

    /// Port of gCInventory_PS::TransferAllTo (Game:0x100b6d70): move every stack; true when
    /// all moved.
    pub fn transfer_all_to(
        &mut self,
        dst: &mut Inventory,
        kind: StackType,
        ctx: &TransferContext,
    ) -> bool {
        let mut ok = true;
        let mut i = 0;
        while i < self.stacks.len() {
            let n = self.stacks[i].amount;
            if self.transfer_items_to(dst, i, n, kind, ctx).is_some() {
                continue;
            }
            ok = false;
            i += 1;
        }
        ok
    }

    /// Port of gCInventory_PS::SetTransactionAmount (Game:0x100b70c0).
    pub fn set_transaction_amount(&mut self, idx: usize, n: i32) -> bool {
        if idx >= self.stacks.len() {
            return false;
        }
        self.stacks[idx].set_transaction_amount(n);
        self.events.push(InventoryEvent::StackChanged(idx));
        true
    }

    /// Port of gCInventory_PS::GetTransactionAmount (Game:0x100b70a0).
    pub fn transaction_amount(&self, idx: usize) -> i32 {
        self.stacks.get(idx).map_or(0, |s| s.transaction)
    }

    /// Port of gCInventory_PS::TransferTransactionItemsTo (Game:0x100b7170): move every
    /// marked amount (the whole stack when the mark is negative or too large) and reset the
    /// marks.
    pub fn transfer_transaction_items_to(
        &mut self,
        dst: &mut Inventory,
        kind: StackType,
        ctx: &TransferContext,
    ) -> Vec<Transfer> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.stacks.len() {
            let s = &mut self.stacks[i];
            let mut n = s.transaction;
            if n < 0 || s.amount < n {
                n = s.amount;
            }
            if n != 0 {
                s.transaction = 0;
                if let Some(t) = self.transfer_items_to(dst, i, n, kind, ctx) {
                    out.push(t);
                }
                // The original re-examines the same index after every attempt.
                continue;
            }
            i += 1;
        }
        out
    }

    /// Port of gCInventory_PS::PerformTransaction (Game:0x100b7130): both sides hand over
    /// their marked items. `self_gets` is the stack type of the items this side receives and
    /// `other_gets` of the items the other side receives.
    pub fn perform_transaction(
        &mut self,
        other: &mut Inventory,
        self_gets: StackType,
        other_gets: StackType,
        ctx_other: &TransferContext,
        ctx_self: &TransferContext,
    ) -> bool {
        self.transfer_transaction_items_to(other, other_gets, ctx_other);
        other.transfer_transaction_items_to(self, self_gets, ctx_self);
        true
    }

    // ---- stats, skills, requirements ---------------------------------------------------

    fn mods_of(&self, idx: usize) -> Vec<StatMod> {
        let s = &self.stacks[idx];
        let damaged = s.quality & QUALITY_DAMAGED != 0;
        s.item()
            .mods
            .iter()
            .filter(|m| !m.tag.is_empty())
            .map(|m| StatMod {
                tag: m.tag.clone(),
                op: m.op,
                value: if damaged { m.value / 2 } else { m.value },
            })
            .collect()
    }

    fn push_stats(&mut self, kind: StatKind, mods: Vec<StatMod>) {
        for m in mods {
            self.events.push(InventoryEvent::Stat {
                kind,
                tag: m.tag,
                op: m.op,
                value: m.value,
            });
        }
    }

    /// Port of gCInventory_PS::IsEquipped(gCArmorSet_PS) (Game:0x100b7ca0): all six pieces
    /// of the set sit in slots.
    pub fn is_armor_set_equipped(&self, set: &super::item::ArmorSet) -> bool {
        set.pieces
            .iter()
            .all(|p| !p.is_empty() && self.is_armor_piece_equipped(p))
    }

    /// Port of gCInventory_PS::IsArmorPieceEquipped (Game:0x100b7d40).
    pub fn is_armor_piece_equipped(&self, template: &str) -> bool {
        self.stacks
            .iter()
            .any(|s| s.is_linked() && eq(&s.template, template))
    }

    /// Port of gCInventorySlot::OnApplyStats (Game:0x100be5f0). A complete armor set grants
    /// its bonus and skills; the item's own modifiers are halved while the stack is damaged;
    /// an undamaged item also activates its skills.
    fn apply_stats(&mut self, idx: usize) {
        let info = self.stacks[idx].info.clone();
        let Some(info) = info else {
            return;
        };
        if let Some(set) = info.armor_set.clone()
            && self.is_armor_set_equipped(&set)
        {
            self.push_stats(StatKind::Apply, set.mods.clone());
            for sk in &set.skills {
                self.activate_skill(sk);
            }
        }
        let mods = self.mods_of(idx);
        self.push_stats(StatKind::Apply, mods);
        if self.stacks[idx].quality & QUALITY_DAMAGED == 0 {
            for sk in info.activate_skills.iter().filter(|s| !s.is_empty()) {
                self.activate_skill(sk);
            }
        }
        self.stacks[idx].stats_applied = true;
    }

    /// Port of gCInventorySlot::OnUnApplyStats (Game:0x100be920).
    fn unapply_stats(&mut self, idx: usize) {
        let info = self.stacks[idx].info.clone();
        self.stacks[idx].stats_applied = false;
        let Some(info) = info else {
            return;
        };
        if let Some(set) = info.armor_set.clone()
            && self.is_armor_set_equipped(&set)
        {
            self.push_stats(StatKind::Unapply, set.mods.clone());
            for sk in &set.skills {
                self.deactivate_skill(sk);
            }
        }
        let mods = self.mods_of(idx);
        self.push_stats(StatKind::Unapply, mods);
        if self.stacks[idx].quality & QUALITY_DAMAGED == 0 {
            for sk in info.activate_skills.iter().filter(|s| !s.is_empty()) {
                self.deactivate_skill(sk);
            }
        }
    }

    /// Port of gCInventory_PS::FindSkillStackIndex (Game:0x100b6870): the permanent stack
    /// whose item teaches `skill`.
    pub fn find_skill_stack_index(&self, skill: &str) -> Option<usize> {
        if skill.is_empty() {
            return None;
        }
        self.stacks.iter().position(|s| {
            s.item().permanent && s.item().skill.as_deref().is_some_and(|k| eq(k, skill))
        })
    }

    /// Port of gCInventory_PS::FindSpellStackIndex (Game:0x100b6940).
    pub fn find_spell_stack_index(&self, spell: &str) -> Option<usize> {
        if spell.is_empty() {
            return None;
        }
        self.stacks.iter().position(|s| {
            s.item().permanent && s.item().spell.as_deref().is_some_and(|k| eq(k, spell))
        })
    }

    /// Port of gCInventory_PS::ActivateSkill (Game:0x100b67b0).
    pub fn activate_skill(&mut self, skill: &str) -> bool {
        match self.find_skill_stack_index(skill) {
            Some(i) => {
                self.stacks[i].activation_count += 1;
                self.changed(i);
                true
            }
            None => false,
        }
    }

    /// Port of gCInventory_PS::DeactivateSkill (Game:0x100b6810).
    pub fn deactivate_skill(&mut self, skill: &str) -> bool {
        match self.find_skill_stack_index(skill) {
            Some(i) => {
                self.stacks[i].activation_count -= 1;
                self.changed(i);
                true
            }
            None => false,
        }
    }

    /// Port of gCInventory_PS::IsSkillActive (Game:0x100b5f80): learned or kept active by
    /// equipment.
    pub fn is_skill_active(&self, skill: &str) -> bool {
        self.find_skill_stack_index(skill)
            .map(|i| &self.stacks[i])
            .is_some_and(|s| s.learned || s.activation_count > 0)
    }

    /// Port of gCInventory_PS::GetSkillActivationCount (Game:0x100b6760).
    pub fn skill_activation_count(&self, skill: &str) -> i32 {
        self.find_skill_stack_index(skill)
            .map_or(0, |i| self.stacks[i].activation_count)
    }

    /// Port of gCInventory_PS::HasLearnedSkill (Game:0x100b5fc0).
    pub fn has_learned_skill(&self, skill: &str) -> bool {
        self.find_skill_stack_index(skill)
            .is_some_and(|i| self.stacks[i].learned)
    }

    /// Port of gCInventory_PS::HasLearnedSpell (Game:0x100b68f0).
    pub fn has_learned_spell(&self, spell: &str) -> bool {
        self.find_spell_stack_index(spell)
            .is_some_and(|i| self.stacks[i].learned)
    }

    /// Port of gCInventory_PS::LearnStack (Game:0x100b5ef0): mark a skill/spell stack learned.
    pub fn learn_stack(&mut self, idx: usize) -> bool {
        let Some(s) = self.stacks.get_mut(idx) else {
            return false;
        };
        if !s.learned {
            s.learned = true;
            self.changed(idx);
        }
        true
    }

    /// Port of gCInventory_PS::LearnSkill (Game:0x100b6790).
    pub fn learn_skill(&mut self, skill: &str) -> bool {
        self.find_skill_stack_index(skill)
            .is_some_and(|i| self.learn_stack(i))
    }

    /// Port of gCInventory_PS::LearnSpell (Game:0x100b6920).
    pub fn learn_spell(&mut self, spell: &str) -> bool {
        self.find_spell_stack_index(spell)
            .is_some_and(|i| self.learn_stack(i))
    }

    /// Port of gCInventory_PS::CanLearnSkill (Game:0x100b5ff0). `skill` is the requirement
    /// block of the skill template (`gCSkill_PS`), `spell` the one of the magic property set
    /// (`gCMagic_PS`) when the template has one; `attribute` returns the owner's attribute
    /// value (`None` for an unknown tag aborts with `false`).
    pub fn can_learn_skill(
        &self,
        skill: Option<&LearnReq>,
        spell: Option<&LearnReq>,
        attribute: impl Fn(&str) -> Option<i32>,
    ) -> LearnCheck {
        let mut out = LearnCheck {
            ok: true,
            ..Default::default()
        };
        let mut reqs: Vec<&(String, i32)> = Vec::new();
        if let Some(sk) = skill {
            if let Some(p) = sk.prerequisite.as_deref().filter(|p| !p.is_empty())
                && !self.has_learned_skill(p)
            {
                out.ok = false;
                out.missing_prerequisites.push(p.to_string());
            }
            reqs.extend(sk.attribs.iter());
        }
        if let Some(sp) = spell {
            if let Some(p) = sp.prerequisite.as_deref().filter(|p| !p.is_empty()) {
                let learned = if self.find_spell_stack_index(p).is_some() {
                    self.has_learned_spell(p)
                } else {
                    self.has_learned_skill(p)
                };
                if !learned {
                    out.ok = false;
                    out.missing_prerequisites.push(p.to_string());
                }
            }
            reqs.extend(sp.attribs.iter());
        }
        for (tag, need) in reqs {
            if tag.is_empty() {
                continue;
            }
            let Some(have) = attribute(tag) else {
                return LearnCheck { ok: false, ..out };
            };
            if have < *need {
                out.ok = false;
                out.low_attribs.push((tag.clone(), *need));
            }
        }
        out
    }

    /// Port of gCInventory_PS::CanEquip (Game:0x100b7dc0). `equip_bonus` is the result of
    /// the script hook `OnPlayerGetEquipBonus(0, 0, quality)` (added to every attribute
    /// requirement, floored at 0); `missing_value(tag, required)` is
    /// `gCPlayerMemory_PS::CalcMissingValue` (`None` for owners without player memory: the
    /// full requirement counts as missing). Required skills must be active.
    pub fn can_equip(
        &self,
        idx: usize,
        equip_bonus: i32,
        missing_value: Option<MissingValueFn<'_>>,
    ) -> EquipCheck {
        let Some(s) = self.stacks.get(idx) else {
            return EquipCheck::default();
        };
        missing_requirements(
            &s.item().reqs,
            &s.item().required_skills,
            equip_bonus,
            missing_value,
            &|sk| self.is_skill_active(sk),
        )
    }

    /// Quality bits the equip-bonus hook is called with (`GetCombinedQuality`).
    pub fn equip_bonus_quality(&self, idx: usize) -> u32 {
        self.stacks.get(idx).map_or(0, Stack::combined_quality)
    }

    /// Port of gCInventory_PS::ConsumeItems (Game:0x100b7970): apply the item's modifiers as
    /// one-off consumption (`Consume`) and activate its skills permanently. The caller
    /// removes the item afterwards with [`Inventory::remove_items`].
    pub fn consume_items(&mut self, idx: usize, n: i32) -> Consumed {
        let Some(s) = self.stacks.get(idx) else {
            return Consumed::default();
        };
        if n < 0 || n > s.amount {
            return Consumed::default();
        }
        let info = s.info.clone();
        let Some(info) = info else {
            return Consumed::default();
        };
        if !self.stats_enabled {
            return Consumed::default();
        }
        let mods: Vec<StatMod> = info
            .mods
            .iter()
            .filter(|m| !m.tag.is_empty())
            .cloned()
            .collect();
        self.push_stats(StatKind::Consume, mods);
        let mut activated = Vec::new();
        for sk in info.activate_skills.iter().filter(|s| !s.is_empty()) {
            if !self.is_skill_active(sk) && self.activate_skill(sk) {
                self.events.push(InventoryEvent::SkillActivated(sk.clone()));
                activated.push(sk.clone());
            }
        }
        Consumed {
            ok: true,
            activated,
        }
    }
}

/// `gCPlayerMemory_PS::CalcMissingValue(tag, required)`: how much of `required` the owner lacks.
pub type MissingValueFn<'a> = &'a dyn Fn(&str, i32) -> i32;

/// The requirement rule shared by `can_equip` and the simulation's equip check: attribute
/// requirements (`value + bonus`, floored at 0) against what the owner is missing, and the
/// required skills against the active ones.
pub fn missing_requirements(
    reqs: &[super::item::StatReq],
    required_skills: &[String],
    equip_bonus: i32,
    missing_value: Option<MissingValueFn<'_>>,
    skill_active: &dyn Fn(&str) -> bool,
) -> EquipCheck {
    let mut out = EquipCheck {
        ok: true,
        ..Default::default()
    };
    for sk in required_skills.iter().filter(|s| !s.is_empty()) {
        if !skill_active(sk) {
            out.ok = false;
            out.missing_skills.push(sk.clone());
        }
    }
    for r in reqs.iter().filter(|r| !r.tag.is_empty()) {
        let required = (r.value + equip_bonus).max(0);
        let missing = match missing_value {
            Some(f) => f(&r.tag, required),
            None => required,
        };
        if missing > 0 {
            out.ok = false;
            out.missing_attribs.push((r.tag.clone(), missing));
        }
    }
    out
}

impl Index<&str> for Inventory {
    type Output = u32;
    fn index(&self, template: &str) -> &u32 {
        self.get(template).expect("template not in inventory")
    }
}

/// Equipment slot table for items (re-export of the use type rule used by hosts that only
/// have a template).
pub fn carry_slots_of(use_type: u32) -> (Slot, Slot) {
    equip_slots(use_type)
}

impl<'a> IntoIterator for &'a Inventory {
    type Item = (&'a String, &'a u32);
    type IntoIter = std::collections::btree_map::Iter<'a, String, u32>;
    fn into_iter(self) -> Self::IntoIter {
        self.counts.iter()
    }
}
