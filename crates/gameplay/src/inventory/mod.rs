//! The inventory model of Gothic 3, ported from `Game.dll`.
//!
//! * [`slot`]: `gESlot`, use types, carry slots and bone descriptors (`gCInventorySlot`).
//! * [`item`]: item template data (`gCItem_PS`) and the [`item::ItemCatalog`] that resolves it.
//! * [`stack`]: one stack (`gCInventoryStack`): merge/split, equipped tests, icons.
//! * [`order`]: the `Compare*` sort chain.
//! * [`list`]: [`Inventory`] (`gCInventory_PS` and its stack list): stacking, equip and hold
//!   rules, linking, drop and transfer, stats, skills, requirement checks.
//! * [`script`]: item use scripts.
//!
//! This is the single authoritative implementation: the simulation, the script host and the
//! UI read and change inventories only through [`Inventory`].
pub mod item;
pub mod list;
pub mod order;
mod save;
pub mod script;
pub mod slot;
pub mod stack;

pub use item::{
    ArmorSet, Category, ItemCatalog, ItemInfo, MapCatalog, NoCatalog, OneItem, StatMod, StatReq,
    category_from_name, use_type_from_name,
};
pub use list::{
    Consumed, DefaultItem, EquipCheck, FindQuality, Inventory, InventoryEvent, LearnCheck,
    LearnReq, StatKind, Transfer, TransferContext, missing_requirements,
};
pub use order::{OrderContext, PlainOrder};
pub use slot::{LinkKind, Slot};
pub use stack::{Stack, StackType};

#[cfg(test)]
mod tests;
