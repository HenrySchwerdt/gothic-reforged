//! `PS_Magic*` and `PS_MagicPotion` (B17, casting half): the hero's spell states, and the eight
//! book stand states `BS_*` (B17, reading half).
//!
//! `PS_Magic` readies the spell in the right hand and settles in `PS_Magic_Loop`. The cast key
//! picks the state by the spell's animation ([`crate::magic::cast::get_cast_action`]): quick
//! cast, cast, power cast, summon or flame sword. Each of them first asks the cast check
//! ([`crate::magic::cast::can_cast`]), plays the cast animation and runs the cast
//! ([`crate::magic::cast::cast_begin`] and [`crate::magic::cast::cast_execute`], the DLL's `_AI_Cast` begin and execute phases), and then
//! `PS_Magic_Reload` re-holds the spell (after the reload delay of the spell) or, for a used-up
//! scroll, drops back to `PS_Normal`.

use crate::enums::{Action, AiMode, AniState, Slot};
use crate::helpers::WeaponMode;
use crate::host::{AiCommand, Attribute, EntityId, ScriptHost};
use crate::magic::api::MagicEvent;
use crate::magic::cast::{self, Phase};
use crate::player::api::{Book, PlayerEvent};
use crate::player::step;
use crate::routine::select::continue_routine;
use crate::routine::state::State;
use crate::routine::work::{Def, Flow};
use crate::speech::say;

/// Template of the weapon `Spl_Flamesword` conjures.
pub const FLAME_SWORD: &str = "It_1H_MagicFlameSword_01";

/// The cast state of an animation action, `None` for actions that do not cast.
pub fn state_for_action(action: Action) -> Option<State> {
    match action {
        Action::Cast => Some(State::PsMagicCast),
        Action::PowerCast => Some(State::PsMagicPowerCast),
        Action::QuickCast => Some(State::PsMagicQuickCast),
        Action::Summon => Some(State::PsMagicSummon),
        Action::FlameSword => Some(State::PsMagicFlameSword),
        _ => None,
    }
}

fn target_of(host: &dyn ScriptHost, p: EntityId) -> EntityId {
    let t = host.current_target(p);
    if t.is_some() { t } else { host.focus_entity() }
}

/// `gEAction` ids the magic loop reads from the queue (the jump table of `PS_Magic_Loop`,
/// S:7a590): the staff parade (16), the cast (44), power cast (45), quick cast (47), summon (48),
/// jump (57), weapon key (68), flame sword (72), sneak (77), quick slots (131..140) and potions
/// (141..143).
const ID_PARADE: u32 = 16;
const ID_CAST: u32 = 44;
const ID_POWER_CAST: u32 = 45;
const ID_QUICK_CAST: u32 = 47;
const ID_SUMMON: u32 = 48;
const ID_JUMP: u32 = 57;
const ID_HOLD: u32 = 68;
const ID_FLAME_SWORD: u32 = 72;
const ID_SNEAK: u32 = 77;
const ID_QUICK_SLOT_BASE: u32 = 131;
const ID_POTION_BASE: u32 = 141;

/// `PSFocus.CurrentMode` of the magic task (`gECombatMode` magic = 4, `GetFocusEntity(.., 4)`).
pub const FOCUS_MODE_MAGIC: u32 = 4;
/// `PSCharacterControl::SetMovementConstraints` of the casting states (`_DAT_101074cc`): the
/// camera does not steer the body, the alignment target does.
pub const CAST_CONSTRAINT_DEGREES: f32 = 360.0;

/// Port of `PS_Magic` (S:79e20): without a spell in hand the hero is back in `PS_Normal`.
/// Otherwise he enters combat mode, clears focus, look-at and alignment, remembers the spell
/// (`LastSpell`), holds it in the right hand and stands up; then `PS_Magic_Loop`.
fn ps_magic(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_magic");
    if crate::combat::select::npc_weapon_mode(host, p) != WeaponMode::Magic {
        host.set_task(p, State::PsNormal);
        return Flow::Done;
    }
    match step(host, p) {
        0 => {
            host.set_ai_mode(p, AiMode::Combat);
            host.set_current_target(p, EntityId::NONE);
            host.set_last_weapon_config(2);
            host.set_focus_entity(EntityId::NONE);
            host.set_focus_current_mode(FOCUS_MODE_MAGIC);
            host.set_look_at(p, EntityId::NONE);
            host.set_alignment_target(p, EntityId::NONE);
            // Whatever the state before left in the constraint [I]: the loop gives the body to the
            // camera (`sub_20b0`) as soon as the hero moves; before that it keeps the last one.
            host.set_movement_constraints(p, crate::player::ranged::FOLLOW_CAMERA_CONSTRAINT);
            let right = host.find_stack_index(p, Slot::RightHand);
            let name = host.stack_spell(p, right);
            host.set_last_spell(p, &name);
            host.queue_ai(p, AiCommand::HoldInventoryStacks { right, left: -1 });
            Flow::Yield
        }
        1 => {
            host.queue_ai(p, AiCommand::StandUp);
            Flow::Yield
        }
        _ => {
            host.set_state(p, State::PsMagicLoop);
            Flow::Done
        }
    }
}

/// Port of `PS_Magic_Loop` (S:7a590), run every pass: the target is the entity the queued action
/// names, else (for the spells that act on a character) the focused one; it is written to the
/// focus and to `CurrentTarget`. While a movement key is down the alignment is cleared and the
/// body follows the camera (`sub_20b0`). Then the head of the action queue becomes the next state:
/// the casts, the staff parade, jump, sneak, the weapon key, quick slots and potions.
fn ps_magic_loop(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_magic_loop");
    if crate::combat::select::npc_weapon_mode(host, p) != WeaponMode::Magic {
        host.set_task(p, State::PsNormal);
        return Flow::Done;
    }
    let needs_focus = cast::hand_spell(host, p).is_some_and(|s| s.needs_focus);
    let queued = host.action_queue_head();
    let target = match queued {
        Some((_, e)) if e.is_some() => e,
        _ if needs_focus => host.focus_entity(),
        _ => EntityId::NONE,
    };
    host.set_focus_entity(target);
    host.set_current_target(p, target);
    if host.movement_key_down() {
        host.set_alignment_target(p, EntityId::NONE);
        host.set_movement_constraints(p, crate::player::ranged::FOLLOW_CAMERA_CONSTRAINT);
    }
    host.set_current_action(p, Action::None);
    let Some((id, _)) = queued else {
        host.queue_ai(p, AiCommand::WaitMs { ms: 20 });
        return Flow::Yield;
    };
    host.action_queue_pop();
    let next = match id {
        ID_PARADE => Some(State::PsMagicStaffParade),
        ID_CAST => Some(State::PsMagicCast),
        ID_POWER_CAST => Some(State::PsMagicPowerCast),
        ID_QUICK_CAST => Some(State::PsMagicQuickCast),
        ID_SUMMON => Some(State::PsMagicSummon),
        ID_JUMP => Some(State::PsMagicJump),
        ID_FLAME_SWORD => Some(State::PsMagicFlameSword),
        ID_SNEAK => Some(State::PsMagicSneak),
        ID_HOLD => {
            crate::player::select_weapon::holster_from_melee(host, p);
            None
        }
        n if (ID_QUICK_SLOT_BASE..ID_QUICK_SLOT_BASE + 10).contains(&n) => {
            host.emit(PlayerEvent::QuickSlot(n - ID_QUICK_SLOT_BASE));
            None
        }
        n if (ID_POTION_BASE..ID_POTION_BASE + 3).contains(&n) => {
            host.emit(PlayerEvent::PotionKey(n - ID_POTION_BASE));
            None
        }
        _ => None,
    };
    match next {
        Some(state) => {
            host.set_state(p, state);
            Flow::Done
        }
        None => {
            host.queue_ai(p, AiCommand::WaitMs { ms: 20 });
            Flow::Yield
        }
    }
}

/// Port of `PS_Magic_QuickCast` (S:7ad30), `PS_Magic_Cast` (S:7b0a0), `PS_Magic_PowerCast`
/// (S:7b410) and `PS_Magic_Summon` (S:7bee0): the cast check (a refused cast drops the queued
/// action and returns to the loop), then, outside the first-person view, the body aligns to the
/// target (a summon has none) with the blocked constraint; `_AI_Cast` starts (the spell is loaded
/// and its mana paid), the cast animation plays and the cast executes at its end. Afterwards
/// `Spl_SoulTravel` leaves the body (`PS_Ghost`), `Spl_DuskToDawn` runs `PS_DuskToDawn`,
/// everything else reloads.
fn ps_magic_cast(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_magic_cast");
    let state = host.current_state(p).unwrap_or(State::PsMagicCast);
    let (action, power) = match state {
        State::PsMagicQuickCast => (Action::QuickCast, false),
        State::PsMagicPowerCast => (Action::PowerCast, true),
        State::PsMagicSummon => (Action::Summon, false),
        _ => (Action::Cast, false),
    };
    match step(host, p) {
        0 => {
            let target = target_of(host, p);
            if !cast::can_cast(host, p, target) {
                host.set_state(p, State::PsMagicLoop);
                return Flow::Done;
            }
            host.set_current_target(p, target);
            host.set_current_action(p, action);
            host.set_ani_state(p, AniState::Stand);
            if !host.camera_in_first_person() {
                let aligned = if state == State::PsMagicSummon {
                    EntityId::NONE
                } else {
                    target
                };
                host.set_alignment_target(p, aligned);
                host.set_movement_constraints(p, CAST_CONSTRAINT_DEGREES);
            }
            host.full_stop(p);
            cast::cast_begin(host, p, power);
            host.queue_ai(p, AiCommand::Action { action, secs: 1 });
            Flow::Yield
        }
        _ => {
            let last = host.slot_spell_name(p, Slot::RightHand);
            cast::cast_execute(host, p);
            // The `Cast_Hit` and `Cast_Recover` clips play after the missile left.
            let (hit, recover) = match action {
                Action::QuickCast => (200, 360),
                Action::PowerCast => (240, 760),
                _ => (280, 840),
            };
            host.queue_ai(p, AiCommand::WaitMs { ms: hit + recover });
            let next = match last.as_str() {
                "Spl_SoulTravel" => {
                    host.full_stop(p);
                    host.set_task(p, State::PsGhost);
                    return Flow::Done;
                }
                "Spl_DuskToDawn" => {
                    host.full_stop(p);
                    host.set_task(p, State::PsDuskToDawn);
                    return Flow::Done;
                }
                _ => State::PsMagicReload,
            };
            host.set_state(p, next);
            Flow::Done
        }
    }
}

/// Port of `PS_Magic_Summon_Abort` (S:7c2f0): the hero lets go of a summon spell in the middle
/// of the cast; the mana paid comes back (cast phase 3).
fn ps_magic_summon_abort(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_magic_summon_abort");
    let stack = host.find_stack_index(p, Slot::RightHand);
    host.full_stop(p);
    cast::start_cast_phase(host, p, EntityId::NONE, stack, Phase::Abort);
    host.set_state(p, State::PsMagicLoop);
    Flow::Done
}

/// Port of `PS_Magic_FlameSword` (S:7b780): load the spell, conjure the flame sword into the
/// hand (`AssureItems` with flag 8, hands: nothing right, the sword left), use the scroll up and
/// let the routine continue (the sword is a weapon now, the mode follows).
fn ps_magic_flame_sword(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_magic_flame_sword");
    match step(host, p) {
        0 => {
            let target = target_of(host, p);
            if !cast::can_cast(host, p, target) {
                host.set_state(p, State::PsMagicLoop);
                return Flow::Done;
            }
            host.set_current_action(p, Action::FlameSword);
            host.set_ani_state(p, AniState::Stand);
            host.full_stop(p);
            let stack = host.find_stack_index(p, Slot::RightHand);
            host.queue_ai(
                p,
                AiCommand::HoldInventoryStacks {
                    right: -1,
                    left: stack,
                },
            );
            Flow::Yield
        }
        1 => {
            let stack = host.find_stack_index(p, Slot::RightHand);
            cast::start_cast_phase(host, p, EntityId::NONE, stack, Phase::Load);
            host.queue_ai(
                p,
                AiCommand::Action {
                    action: Action::FlameSword,
                    secs: 1,
                },
            );
            Flow::Yield
        }
        _ => {
            let stack = host.find_stack_index(p, Slot::RightHand);
            let permanent = host.stack_permanent(p, stack);
            let sword = host.assure_items(p, FLAME_SWORD, 8, 1);
            host.queue_ai(
                p,
                AiCommand::HoldInventoryStacks {
                    right: -1,
                    left: sword,
                },
            );
            host.emit_magic(MagicEvent::Weapon { owner: p, on: true });
            host.set_cast_spell(p, "");
            host.set_mana_used(p, 0);
            if !permanent {
                host.delete_stack_items(p, stack, 1);
            }
            continue_routine(host, p);
            Flow::Done
        }
    }
}

/// Port of `PS_Magic_Reload` (S:7ca40): reset the cast phase; a spell without `AutoReload` (or
/// one whose scroll is gone) sends the hero to `PS_Normal`; otherwise he holds the spell again,
/// waits the spell's reload delay and returns to the loop.
fn ps_magic_reload(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_magic_reload");
    let stack = host.find_stack_index(p, Slot::RightHand);
    match step(host, p) {
        0 => {
            cast::start_cast_phase(host, p, EntityId::NONE, stack, Phase::Reset);
            let Some(spell) = cast::hand_spell(host, p) else {
                host.set_task(p, State::PsNormal);
                return Flow::Done;
            };
            if !spell.auto_reload {
                host.queue_ai(
                    p,
                    AiCommand::HoldInventoryStacks {
                        right: -1,
                        left: -1,
                    },
                );
                host.set_task(p, State::PsNormal);
                return Flow::Done;
            }
            cast::start_cast_phase(host, p, EntityId::NONE, stack, Phase::Hold);
            host.queue_ai(
                p,
                AiCommand::WaitMs {
                    ms: (spell.reload_secs * 1000.0) as u32,
                },
            );
            Flow::Yield
        }
        _ => {
            host.set_state(p, State::PsMagicLoop);
            Flow::Done
        }
    }
}

/// Port of `PS_Magic_StaffParade` (S:7c630) and `PS_Magic_StaffParade_Loop` (S:7c8c0): with the
/// staff the mage parries (`_AI_Parade`); the loop leaves it when a cast key comes.
fn ps_magic_staff_parade(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_magic_staff_parade");
    match step(host, p) {
        0 => {
            let target = target_of(host, p);
            crate::combat::commands::start_parade(host, p);
            host.queue_ai(p, AiCommand::Parade { target });
            Flow::Yield
        }
        _ => {
            host.set_state(p, State::PsMagicStaffParadeLoop);
            Flow::Done
        }
    }
}

fn ps_magic_staff_parade_loop(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    host.queue_ai(p, AiCommand::WaitMs { ms: 100 });
    Flow::Yield
}

/// Port of `PS_MagicPotion` (S:7d870): the hero stops, drinks (the spell potions of the animal
/// forms and the like) and the potion's spell is cast as a quick cast (`_AI_Cast` with the spell
/// of the potion's template item). The potion sits in the right hand while the spell is read
/// from it ([`crate::player::use_item::use_stack`] put it there), is used up by the cast and the
/// hands go down afterwards. [I] The original puts the hands down first and reads the spell from
/// the template item; the port keeps the potion in the hand until the cast instead.
fn ps_magic_potion(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_magic_potion");
    match step(host, p) {
        0 => {
            host.full_stop(p);
            host.set_current_action(p, Action::Drink);
            host.queue_ai(
                p,
                AiCommand::Action {
                    action: Action::Drink,
                    secs: 1,
                },
            );
            Flow::Yield
        }
        _ => {
            let potion = host.slot_item_name(p, Slot::RightHand);
            cast::run_cast(host, p, false);
            if !potion.is_empty() {
                host.remove_item(p, &potion, 1);
            }
            host.queue_ai(
                p,
                AiCommand::HoldInventoryStacks {
                    right: -1,
                    left: -1,
                },
            );
            host.set_state(p, State::PsNormalLoop);
            Flow::Done
        }
    }
}

// ---- book stands ---------------------------------------------------------------------------------------

/// Pages a book can be read to (`BookOfFlood` 18, `BookOfRhobar` and `BookOfZuben` 8).
pub const fn book_pages(book: Book) -> i32 {
    match book {
        Book::Flood => 18,
        Book::Rhobar | Book::Zuben => 8,
    }
}

/// Intelligence a read grants: 3 with `ExtendedContent`, otherwise 5.
fn read_bonus(host: &dyn ScriptHost) -> i32 {
    if host.setup().extended_content { 3 } else { 5 }
}

fn book_state(book: Book) -> State {
    match book {
        Book::Flood => State::BsBookOfFlood,
        Book::Rhobar => State::BsBookOfRhobar,
        Book::Zuben => State::BsBookOfZuben,
    }
}

fn book_line(book: Book, page: i32) -> &'static str {
    // The voice samples are `BookOf<Name>_<page>`; the table keeps them static.
    const FLOOD: [&str; 18] = [
        "BookOfFlood_0",
        "BookOfFlood_1",
        "BookOfFlood_2",
        "BookOfFlood_3",
        "BookOfFlood_4",
        "BookOfFlood_5",
        "BookOfFlood_6",
        "BookOfFlood_7",
        "BookOfFlood_8",
        "BookOfFlood_9",
        "BookOfFlood_10",
        "BookOfFlood_11",
        "BookOfFlood_12",
        "BookOfFlood_13",
        "BookOfFlood_14",
        "BookOfFlood_15",
        "BookOfFlood_16",
        "BookOfFlood_17",
    ];
    const RHOBAR: [&str; 8] = [
        "BookOfRhobar_0",
        "BookOfRhobar_1",
        "BookOfRhobar_2",
        "BookOfRhobar_3",
        "BookOfRhobar_4",
        "BookOfRhobar_5",
        "BookOfRhobar_6",
        "BookOfRhobar_7",
    ];
    const ZUBEN: [&str; 8] = [
        "BookOfZuben_0",
        "BookOfZuben_1",
        "BookOfZuben_2",
        "BookOfZuben_3",
        "BookOfZuben_4",
        "BookOfZuben_5",
        "BookOfZuben_6",
        "BookOfZuben_7",
    ];
    let i = page.max(0) as usize;
    match book {
        Book::Flood => FLOOD[i.min(17)],
        Book::Rhobar => RHOBAR[i.min(7)],
        Book::Zuben => ZUBEN[i.min(7)],
    }
}

/// Port of `BS_BookOfFlood` (S:65180), `BS_BookOfRhobar` (S:656f0) and `BS_BookOfZuben`: after half
/// a second the hero reads two pages aloud (`BookOf<Name>_<page>`, the page counter advances by
/// two) and gains Intelligence; a finished book sends him to `BS_NothingNew`. The bookstand is
/// marked used.
fn bs_book(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("bs_book");
    let book = match host.current_state(p) {
        Some(State::BsBookOfRhobar) => Book::Rhobar,
        Some(State::BsBookOfZuben) => Book::Zuben,
        _ => Book::Flood,
    };
    match step(host, p) {
        0 => {
            if host.book_progress(book) >= book_pages(book) {
                host.set_state(p, State::BsNothingNew);
                return Flow::Done;
            }
            host.queue_ai(p, AiCommand::WaitMs { ms: 500 });
            Flow::Yield
        }
        1 => {
            let page = host.book_progress(book);
            say(host, p, book_line(book, page));
            host.set_book_progress(book, page + 1);
            host.queue_ai(p, AiCommand::WaitMs { ms: 500 });
            Flow::Yield
        }
        _ => {
            let page = host.book_progress(book);
            say(host, p, book_line(book, page));
            host.set_book_progress(book, page + 1);
            let bonus = read_bonus(host);
            crate::stats::add_attribute(host, p, Attribute::Intelligence, bonus);
            let object = host.current_target(p);
            if object.is_some() {
                host.set_used_by_player(object, true);
            }
            host.emit(PlayerEvent::BookRead {
                book: book_state(book).name(),
            });
            continue_routine(host, p);
            Flow::Done
        }
    }
}

/// Port of `BS_AncientWisdom` (S:65080): the bookstand of the ancient wisdom picks the book the
/// hero has not finished: Flood, then Rhobar, then Zuben, and when all are read `BS_NothingNew`.
fn bs_ancient_wisdom(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("bs_ancient_wisdom");
    let next = [Book::Flood, Book::Rhobar, Book::Zuben]
        .into_iter()
        .find(|b| host.book_progress(*b) < book_pages(*b))
        .map_or(State::BsNothingNew, book_state);
    host.set_state(p, next);
    Flow::Done
}

/// Port of `BS_NothingNew` (S:64da0): half a second, then one of three remarks
/// (`NothingNew_00..02`), and on with the routine.
fn bs_nothing_new(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("bs_nothing_new");
    match step(host, p) {
        0 => {
            host.queue_ai(p, AiCommand::WaitMs { ms: 500 });
            Flow::Yield
        }
        _ => {
            let key = match host.random(3) {
                0 => "NothingNew_00",
                1 => "NothingNew_01",
                _ => "NothingNew_02",
            };
            say(host, p, key);
            continue_routine(host, p);
            Flow::Done
        }
    }
}

/// Port of `BS_Alchemy` (S:66a00) and `BS_Smith`: after half a second a remark (`AlchemyBook`,
/// `SmithBook`) and +3 Alchemy or Smithing (+5 without `ExtendedContent`); the stand is used up.
fn bs_skill_book(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("bs_skill_book");
    let alchemy = host.current_state(p) == Some(State::BsAlchemy);
    match step(host, p) {
        0 => {
            host.queue_ai(p, AiCommand::WaitMs { ms: 500 });
            Flow::Yield
        }
        _ => {
            say(host, p, if alchemy { "AlchemyBook" } else { "SmithBook" });
            let bonus = read_bonus(host);
            let attr = if alchemy {
                Attribute::Alchemy
            } else {
                Attribute::Smithing
            };
            crate::stats::add_attribute(host, p, attr, bonus);
            let object = host.current_target(p);
            if object.is_some() {
                host.set_used_by_player(object, true);
            }
            continue_routine(host, p);
            Flow::Done
        }
    }
}

/// Port of `BS_Chronicles` (S:66190): the Dargoth chronicles are read once; the game event
/// `Dargoth_Chronicles` marks it. The five remarks and the quest hooks of the DLL are story data
/// [U]; the port raises the event and continues.
fn bs_chronicles(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("bs_chronicles");
    match step(host, p) {
        0 => {
            host.queue_ai(p, AiCommand::WaitMs { ms: 500 });
            Flow::Yield
        }
        _ => {
            if !host.game_event_set("Dargoth_Chronicles") {
                host.raise_game_event("Dargoth_Chronicles");
            }
            continue_routine(host, p);
            Flow::Done
        }
    }
}

// ---- soul travel -------------------------------------------------------------------------------------

/// Port of `PS_Ghost` (S:673f0): the soul has left the body. The hero changes action, puts his
/// hands down, loses his focus and stands up, then waits in `PS_Ghost_Loop`.
fn ps_ghost(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_ghost");
    match step(host, p) {
        0 => {
            host.set_ai_mode(p, AiMode::Talk);
            host.queue_ai(p, AiCommand::ChangeAction);
            Flow::Yield
        }
        1 => {
            host.queue_ai(
                p,
                AiCommand::HoldInventoryStacks {
                    right: -1,
                    left: -1,
                },
            );
            Flow::Yield
        }
        2 => {
            host.set_current_target(p, EntityId::NONE);
            host.queue_ai(p, AiCommand::StandUp);
            Flow::Yield
        }
        _ => {
            host.set_state(p, State::PsGhostLoop);
            Flow::Done
        }
    }
}

/// Port of `PS_Ghost_Loop` (S:67900): the ghost looks at whatever is in focus; a jump key sends
/// him into `PS_Ghost_Jump`.
fn ps_ghost_loop(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_ghost_loop");
    let focus = host.focus_entity();
    host.set_current_target(p, focus);
    host.queue_ai(p, AiCommand::WaitMs { ms: 100 });
    Flow::Yield
}

/// Port of `PS_Ghost_Jump` (S:67a70): one jump, then back to the loop.
fn ps_ghost_jump(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_ghost_jump");
    match step(host, p) {
        0 => {
            crate::combat::commands::start_jump(host, p, false);
            host.queue_ai(
                p,
                AiCommand::Action {
                    action: Action::Jump,
                    secs: 1,
                },
            );
            Flow::Yield
        }
        _ => {
            host.set_state(p, State::PsGhostLoop);
            Flow::Done
        }
    }
}

/// Port of `PS_GhostSummoner` (S:67c80) and `PS_GhostSummoner_Loop`: the body the soul left behind
/// is abandoned: hands down, standing, idle.
fn ps_ghost_summoner(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_ghost_summoner");
    match step(host, p) {
        0 => {
            host.set_ai_mode(p, AiMode::Abandoned);
            host.queue_ai(
                p,
                AiCommand::HoldInventoryStacks {
                    right: -1,
                    left: -1,
                },
            );
            Flow::Yield
        }
        1 => {
            host.queue_ai(p, AiCommand::StandUp);
            Flow::Yield
        }
        _ => {
            host.set_state(p, State::PsGhostSummonerLoop);
            Flow::Done
        }
    }
}

fn ps_ghost_summoner_loop(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    host.queue_ai(p, AiCommand::WaitMs { ms: 200 });
    Flow::Yield
}

/// Definitions of the magic and book stand states.
pub fn definition(state: State) -> Option<Def> {
    use State as S;
    let custom = |f: fn(&mut dyn ScriptHost, EntityId) -> Flow| Some(Def::Custom(f));
    match state {
        S::PsMagic => custom(ps_magic),
        S::PsMagicLoop => custom(ps_magic_loop),
        S::PsMagicQuickCast | S::PsMagicCast | S::PsMagicPowerCast | S::PsMagicSummon => {
            custom(ps_magic_cast)
        }
        S::PsMagicSummonAbort => custom(ps_magic_summon_abort),
        S::PsMagicFlameSword => custom(ps_magic_flame_sword),
        S::PsMagicReload => custom(ps_magic_reload),
        S::PsMagicStaffParade => custom(ps_magic_staff_parade),
        S::PsMagicStaffParadeLoop => custom(ps_magic_staff_parade_loop),
        S::PsMagicPotion => custom(ps_magic_potion),
        S::BsBookOfFlood | S::BsBookOfRhobar | S::BsBookOfZuben => custom(bs_book),
        S::BsAncientWisdom => custom(bs_ancient_wisdom),
        S::BsNothingNew => custom(bs_nothing_new),
        S::BsAlchemy | S::BsSmith => custom(bs_skill_book),
        S::BsChronicles => custom(bs_chronicles),
        S::PsGhost => custom(ps_ghost),
        S::PsGhostLoop => custom(ps_ghost_loop),
        S::PsGhostJump => custom(ps_ghost_jump),
        S::PsGhostSummoner => custom(ps_ghost_summoner),
        S::PsGhostSummonerLoop => custom(ps_ghost_summoner_loop),
        _ => None,
    }
}
