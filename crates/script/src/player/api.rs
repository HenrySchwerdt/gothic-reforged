//! Host interface of the player-side port: interact objects, recipes, loose items, the
//! speech counters and the events the UI layer reacts to.
//!
//! The ported `PS_*` states do not draw anything or open any window themselves; they call
//! [`PlayerApi::emit`] with a [`PlayerEvent`] and the game layer (Bevy play mode) turns it into
//! a HUD window, a sound or a message. This is the contract the runtime commands
//! (`UseInteractObject`, `PickUp`, `Lockpick`, `Craft`, ...) drive.

use crate::host::EntityId;

/// `gELockStatus` of a `PSLock`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LockStatus {
    /// Locked (raw 0).
    #[default]
    Locked,
    /// Open or unlocked (raw 1).
    Unlocked,
}

/// The lock of a chest or door (`PSLock`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Lock {
    /// Locked or open.
    pub status: LockStatus,
    /// Template of the key; empty when the lock has no key.
    pub key: String,
    /// `KeyAmount`: how many keys the lock takes (at least one).
    pub key_amount: u32,
    /// `Difficulty`: 1-3 can be picked (needs `Perk_LockPick_<n>`), anything else only opens
    /// with the key.
    pub difficulty: u32,
}

/// A recipe skill category (`gESkillCategory` argument of `HasRecipes`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SkillCategory {
    /// Smithing recipes (raw 3).
    Smith,
    /// Alchemy recipes (raw 5).
    Alchemy,
    /// Cooking recipes (raw 7).
    Cooking,
}

/// A synthesis recipe (`PSRecipe`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recipe {
    /// Category (decides the work station).
    pub category: SkillCategory,
    /// Ingredients as `(template, amount)`.
    pub ingredients: Vec<(String, u32)>,
    /// Result template.
    pub result: String,
    /// `ResultAmount`: zero means one.
    pub result_amount: u32,
    /// Perk the player needs, empty for none (`ReqSkill1`).
    pub required_perk: String,
}

/// A HUD window the game layer should open (`GUI2::Open*`, `PlayerOpen*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UiWindow {
    /// Loot window of a container (`OpenLoot`).
    Loot,
    /// Smithing recipes at the anvil.
    Smithing,
    /// Alchemy recipes.
    Alchemy,
    /// Cooking recipes at the cauldron.
    Cooking,
    /// Frying meat at the barbecue (`OpenFry`).
    Fry,
    /// Sharpening a blade at the grindstone (`OpenSharpen`).
    Sharpen,
    /// Poisoning a blade (`OpenPoison`).
    Poison,
    /// Blessing a weapon (`OpenBless`).
    Bless,
    /// A book page.
    Book,
    /// Dialog window.
    Dialog,
    /// Inventory.
    Inventory,
    /// Trade window.
    Trade,
    /// Map.
    Map,
    /// Quest log.
    Quests,
    /// Skills.
    Skills,
    /// Spell book.
    Spells,
    /// A letter.
    Letter,
}

/// Why an interaction or a synthesis was refused (the HUD prints the matching message).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The player needs a tool (`FO_<template>` in the loc table).
    MissingItem(&'static str),
    /// The lock needs its key and cannot be picked.
    NeedKey,
    /// The lock has a key but there is neither key nor lockpick.
    KeyOrLockpick,
    /// The lock has no key and there is no lockpick.
    NeedLockpick,
    /// A perk is missing (`HUD_MissingSkill` with the perk name).
    MissingPerk(&'static str),
    /// The lock cannot be picked at all.
    Impossible,
    /// No recipes of this category are known.
    NoRecipes(SkillCategory),
    /// Sharpening needs a bladed weapon in hand.
    NoBlade,
    /// The object is empty or exhausted.
    NothingToGet,
    /// The shrine of another god.
    WrongGod,
    /// The object is blocked or in use.
    DoesntWork,
    /// Synthesis: ingredients missing.
    MissingIngredients,
    /// Synthesis: the learning points / perk are missing.
    NotEnoughLp(String),
}

/// What the game layer is told about the player's actions.
#[derive(Debug, Clone, PartialEq)]
pub enum PlayerEvent {
    /// A window should open.
    Window(UiWindow),
    /// The script closed the window again.
    CloseWindow,
    /// The interaction started (`PS_Interact`).
    InteractStarted {
        /// The interact object.
        object: EntityId,
    },
    /// The interaction ended (`PS_Interact_End`).
    InteractEnded {
        /// The interact object.
        object: EntityId,
    },
    /// A door opened or closed.
    DoorChanged {
        /// The door.
        object: EntityId,
        /// New state.
        open: bool,
    },
    /// A lock was picked or the lockpick broke.
    LockPicked {
        /// The locked object.
        object: EntityId,
        /// Success.
        success: bool,
    },
    /// An item was taken from the world.
    PickedUp {
        /// Template of the item.
        template: String,
        /// The item belonged to somebody (a theft was counted).
        stolen: bool,
    },
    /// A recipe was synthesised.
    Crafted {
        /// Result template.
        template: String,
        /// Amount produced.
        amount: u32,
    },
    /// An interaction or synthesis was refused.
    Refused(Refusal),
    /// The player slept.
    Slept {
        /// Hours of world time that passed.
        hours: f32,
    },
    /// A book gave a permanent bonus.
    BookRead {
        /// Which book (the `BS_*` state name).
        book: &'static str,
    },
    /// The world clock runs faster or slower (sleeping).
    TimeScale(f32),
}

/// What the focus (crosshair target) of the player is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FocusKind {
    /// Nothing usable.
    #[default]
    None,
    /// A character (alive, sleeping, down or dead).
    Npc,
    /// A loose item lying around.
    Item,
    /// An interact object (chest, door, work station, bed, ...).
    Object,
}

/// Counters of the rotating voice-line choosers (static counters in the DLL).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VoiceCounter {
    /// Rotation of the refuse-talk family (`StartSayRefuseTalk`, `CrimeRefuse`, `ZoneRefuse`).
    Refuse,
    /// Variation counter of `StartSayAargh`.
    Aargh,
}

/// Everything the player states need besides the NPC and inventory traits.
pub trait PlayerApi {
    /// Read a speech counter (zero until written).
    fn voice_counter(&self, c: VoiceCounter) -> u32;
    /// Write a speech counter.
    fn set_voice_counter(&mut self, c: VoiceCounter, v: u32);

    /// The lock of an object, `None` when it has none.
    fn lock(&self, object: EntityId) -> Option<Lock>;
    /// Set the lock status.
    fn set_lock_status(&mut self, object: EntityId, status: LockStatus);
    /// Is the door open?
    fn door_open(&self, object: EntityId) -> bool;
    /// Open or close a door.
    fn set_door_open(&mut self, object: EntityId, open: bool);
    /// `PSInteraction.UsedByPlayer`: the player already used this object up (ore, bookstands).
    fn used_by_player(&self, object: EntityId) -> bool;
    /// Mark the object as used by the player.
    fn set_used_by_player(&mut self, object: EntityId, v: bool);
    /// Name of the game event set when the object is used (empty for none).
    fn use_event(&self, object: EntityId) -> String;
    /// Owner enclave of an object or item (`NONE` for ownerless things).
    fn owner_enclave(&self, thing: EntityId) -> EntityId;
    /// Stacks left in the object's generated plunder.
    fn loot_stacks(&self, object: EntityId) -> i32;
    /// Generate the plunder from the treasure sets once and return the stack count.
    fn generate_loot(&mut self, object: EntityId) -> i32;

    /// What kind of thing an entity is, for the confirm key.
    fn focus_kind(&self, e: EntityId) -> FocusKind;
    /// Stacks of a character that a looter may take: everything but merchandise and the
    /// stacks of category 0 and 9 (`GetCategory`), as `(template, amount)`.
    fn lootable(&self, npc: EntityId) -> Vec<(String, u32)>;
    /// Move `amount` of `template` from one inventory to another.
    fn transfer_item(&mut self, from: EntityId, to: EntityId, template: &str, amount: u32);

    /// Template name of a loose item entity.
    fn item_template(&self, item: EntityId) -> String;
    /// Move a loose item into an inventory.
    fn take_item(&mut self, taker: EntityId, item: EntityId);
    /// Add items to an inventory.
    fn add_item(&mut self, e: EntityId, template: &str, amount: u32);
    /// Remove up to `amount` items; returns whether the full amount was there.
    fn remove_item(&mut self, e: EntityId, template: &str, amount: u32) -> bool;
    /// How many of `template` the entity carries.
    fn item_count(&self, e: EntityId, template: &str) -> u32;

    /// `HasRecipes`: does the inventory hold a recipe item of this category?
    fn has_recipes(&self, e: EntityId, category: SkillCategory) -> bool;
    /// `HasBladeWeapon`: does the inventory hold an item with the blade damage type?
    fn has_blade_weapon(&self, e: EntityId) -> bool;
    /// Use type of an item template (`PSItem` use type: weapon class, food, potion, ...).
    fn item_use_type(&self, template: &str) -> crate::enums::UseType;
    /// Put an item on or in a hand; `false` when a requirement is unmet (the host prints it).
    fn equip_item(&mut self, e: EntityId, template: &str) -> bool;
    /// Take an equipped item off again.
    fn unequip_item(&mut self, e: EntityId, template: &str);
    /// Is the item currently equipped?
    fn is_equipped(&self, e: EntityId, template: &str) -> bool;
    /// Use up one item (eat, drink, read) and apply its effect.
    fn consume_item(&mut self, e: EntityId, template: &str) -> bool;
    /// A recipe by id.
    fn recipe(&self, id: i32) -> Option<Recipe>;
    /// Ids of all recipes the player knows.
    fn known_recipes(&self) -> Vec<i32>;

    /// Set a game event in the player memory.
    fn raise_game_event(&mut self, name: &str);
    /// Advance the world clock (and the playing time) by this many hours.
    fn advance_hours(&mut self, hours: f32);
    /// Report something to the game layer.
    fn emit(&mut self, event: PlayerEvent);
}
