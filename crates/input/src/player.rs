//! The player key router: what each key does in each player state.
//!
//! Port of the script input interface of `Script_Game.dll` (`Script/Player/Player_Interface/
//! OnPlayerGameKeyPressed.cpp`): `OnPlayerGameKeyPressed` (S:61640), `OnPlayerMovement`
//! (S:622f0), `OnPlayerAction` (S:62480, the first action key), `OnPlayerSecondaryAction`
//! (S:62ff0), `OnPlayerWalk` (S:638b0), `OnPlayerSneak` (S:63a00), `OnPlayerJump` (S:63b80),
//! `OnMagicHandleInput` (S:7d360) and the message loops of `PS_Ranged_Walk_Loop` (S:84080) and
//! `PS_Ranged_Aim_Loop` (S:84d60) / `OnPowerAim_Loop` (S:84b90).
//!
//! The protocol: `gCCharacterControl_PS::OnAction` stores the key state in the
//! `PSCharacterControl` properties and calls the script function `OnPlayerGameKeyPressed` for
//! every [`GameKeyMessage`](crate::GameKeyMessage). The handlers do not act themselves; they
//! enqueue a gEAction id (an [`Order::Message`]) in the player's action queue, which the loop
//! states of the `PS_*` tasks poll and turn into state changes ([`ranged_loop`]). A few handlers
//! switch states directly ([`Order::EnterState`]).
//!
//! Every constant below is read from the DLL unless marked [I] (inferred from the structure of
//! the code where the decompiler lost a string or an argument).
use crate::keys::SessionKey;
use crate::mapper::GameKeyMessage;

/// `gEAction` ids the handlers enqueue.
pub mod msg {
    pub const ATTACK: u32 = 1;
    pub const QUICK_ATTACK: u32 = 3;
    pub const PARADE: u32 = 16;
    pub const GET_UP_ATTACK: u32 = 30;
    pub const GET_UP_PARADE: u32 = 31;
    /// Cancel the aim in progress (`AbortAttack`).
    pub const ABORT_ATTACK: u32 = 39;
    pub const AIM: u32 = 40;
    pub const SHOOT: u32 = 41;
    pub const RELOAD: u32 = 42;
    pub const CAST: u32 = 44;
    pub const POWER_CAST: u32 = 45;
    pub const QUICK_CAST: u32 = 47;
    pub const SUMMON: u32 = 48;
    pub const JUMP_BACK: u32 = 51;
    pub const JUMP: u32 = 57;
    /// The use key in `PS_Normal` and `PS_Interact`: talk, take, open, pull.
    pub const USE: u32 = 63;
    /// The weapon key: draw or put away.
    pub const HOLD: u32 = 68;
    pub const SNEAK: u32 = 77;
    /// `PS_Interact`, right button.
    pub const INTERACT_SECONDARY: u32 = 102;
    /// `QuickUse n` enqueues `QUICK_SLOT_BASE + n`.
    pub const QUICK_SLOT_BASE: u32 = 131;
    /// `Use Potion HP/MP/SP` enqueue `POTION_BASE + 0..2`.
    pub const POTION_BASE: u32 = 141;
}

/// The click is a tap up to this many milliseconds (`DurationPressedMSecs < 0xc9`).
pub const TAP_MAX_MS: u32 = 200;
/// A held secondary key starts the parry after this long (`> 0x32`).
pub const PARRY_HOLD_MS: u32 = 50;
/// The attack key held this long (`> 100`) sets the attack latch and starts the charge.
pub const CHARGE_HOLD_MS: u32 = 100;
/// A secondary key held this long in melee gives up the guard (`> 0x9c4`).
pub const GUARD_RELEASE_MS: u32 = 2500;
/// A secondary key held this long in magic puts the spell away (`>= 0x5dd`).
pub const MAGIC_CANCEL_MS: u32 = 1501;
/// Releasing the cast key earlier than this many seconds into `PS_Magic_Cast` aborts the cast
/// (`_DAT_101076d8`).
pub const CAST_MIN_SECS: f32 = 0.7;

/// The task of `PSRoutine` the player is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Task {
    Normal,
    Melee,
    Ranged,
    Magic,
    Interact,
    /// `ZS_SitKnockDown`: sitting on the floor after a knock down.
    SitKnockDown,
    /// Dialog (`ZS_Talk_Loop`).
    Talk,
    Other,
}

/// `gEUseType` values the router looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hand {
    /// Nothing in the left hand.
    Empty,
    Bow,
    CrossBow,
    Staff,
    Other,
}

/// What the router needs to know about the player this frame.
#[derive(Debug, Clone)]
pub struct Context<'a> {
    /// The current task (`PSRoutine.GetCurrentTask`).
    pub task: Task,
    /// The current state name (`GetCurrentState`), e.g. `PS_Melee_Parade`.
    pub state: &'a str,
    /// Seconds in the current state.
    pub state_time: f32,
    /// `PSRoutine.AniState`: 3 (sneak) and 4 (parade) block the ranged action key.
    pub ani_state: u32,
    /// The item in the left hand.
    pub left: Hand,
    /// An ammunition item is in the right-hand slot.
    pub ammo_in_hand: bool,
    /// The player is a human (not transformed): only humans jump and sneak by key.
    pub human: bool,
    /// `PSCharacterMovement::CanJump`.
    pub can_jump: bool,
    /// `Entity::IsStanding || IsBraking`.
    pub standing_or_braking: bool,
    /// The AI mode is one that ignores keys (`gEAIMode` 8 or 9: dead, unconscious).
    pub keys_ignored: bool,
    /// A HUD page or the console handled the key (`OnHUDHandleInput`).
    pub hud_consumed: bool,
    /// `CanDo(Parade)` / `CanDo(QuickAttack)` of the melee layer (S:4f80).
    pub can_parade: bool,
    pub can_quick_attack: bool,
    /// The spell in hand: its `GetCastAction` (S:52a10), `None` when unusable.
    pub cast_action: Option<u32>,
    /// `eSSetupEngine` flag at +0x1eb: the extended hold controls of the options.
    pub extended_hold: bool,
}

impl Default for Context<'_> {
    fn default() -> Self {
        Self {
            task: Task::Normal,
            state: "PS_Normal_Loop",
            state_time: 0.0,
            ani_state: 2,
            left: Hand::Empty,
            ammo_in_hand: false,
            human: true,
            can_jump: true,
            standing_or_braking: false,
            keys_ignored: false,
            hud_consumed: false,
            can_parade: true,
            can_quick_attack: true,
            cast_action: None,
            extended_hold: false,
        }
    }
}

/// What a key asks the player to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Order {
    /// Enqueue this action id in the player's action queue (the loop states read it).
    Message(u32),
    /// `FullStop` and `SetState(name)` at once.
    EnterState(&'static str),
    /// Walk modifier of the movement: `true` makes the player walk.
    Walk(bool),
    /// The duck key changed: `true` while it is held (`OnPlayerSneak` also enqueues
    /// [`msg::SNEAK`] for humans on the press).
    SneakHeld(bool),
    /// Skip the dialog line (`ZS_Talk_Loop`, secondary key).
    SkipTalkLine,
    /// Toggle the HUD.
    ToggleHud,
    /// The confirm key while transformed.
    ConfirmTransformed,
    /// The primary key in `PS_Melee`: the host runs the melee layer's own rule
    /// (`melee::move_for_press`) with this edge and duration.
    MeleeAttackKey {
        press: bool,
        release: bool,
        duration_ms: u32,
    },
}

/// The static latches of the script DLL.
#[derive(Debug, Clone, Default)]
pub struct PlayerInput {
    /// `DAT_10118aaf`: the aim key went down in `PS_Ranged` (a bow is being drawn).
    aimed: bool,
    /// `DAT_10118b40`: the attack key is latched (held beyond the charge threshold).
    attack_latch: bool,
    /// `DAT_10118b41`: the secondary key is latched (the parry started).
    secondary_latch: bool,
    /// `DAT_10118aac`: a movement key is down.
    movement_down: bool,
    /// `DAT_10118ab3`: the forward key is down.
    forward_down: bool,
    /// `DAT_10118b42`: the duck key is down.
    sneak_down: bool,
}

impl PlayerInput {
    /// A movement key is held (drives the walk/run decision of the movement).
    pub fn movement_held(&self) -> bool {
        self.movement_down
    }
    /// The forward key is held.
    pub fn forward_held(&self) -> bool {
        self.forward_down
    }
    /// The duck key is held.
    pub fn sneak_held(&self) -> bool {
        self.sneak_down
    }

    /// Port of `OnPlayerGameKeyPressed` (S:61640): what one key message asks of the player.
    pub fn route(&mut self, m: &GameKeyMessage, ctx: &Context<'_>) -> Vec<Order> {
        let mut out = Vec::new();
        if ctx.keys_ignored {
            return out;
        }
        if ctx.hud_consumed {
            self.movement_down = false;
            self.secondary_latch = false;
            return out;
        }
        if self.magic_input(m, ctx, &mut out) {
            return out;
        }
        use SessionKey::*;
        match m.key {
            Forward | Backward | StrafeLeft | StrafeRight => {
                self.movement_down = m.is_pressed();
                if m.key == Forward {
                    self.forward_down = m.is_pressed();
                }
                return out;
            }
            Use1 => self.primary(m, ctx, &mut out),
            Use2 => self.secondary(m, ctx, &mut out),
            _ => {}
        }
        // Everything below reacts to the edges, not to the held frames.
        if m.is_pressed() && m.was_pressed() {
            return out;
        }
        match m.key {
            Walk => {
                if m.is_pressed() {
                    out.push(Order::Walk(!ctx.standing_or_braking));
                } else {
                    out.push(Order::Walk(false));
                }
            }
            Duck => {
                self.sneak_down = m.is_pressed();
                if m.is_pressed() {
                    if ctx.human {
                        out.push(Order::Message(msg::SNEAK));
                    }
                    out.push(Order::SneakHeld(true));
                } else {
                    out.push(Order::SneakHeld(false));
                }
            }
            _ => {}
        }
        if !m.is_pressed() {
            return out;
        }
        match m.key {
            Jump => {
                if ctx.can_jump && ctx.human {
                    out.push(Order::Message(msg::JUMP));
                }
            }
            k if k.quick_slot().is_some() => {
                out.push(Order::Message(
                    msg::QUICK_SLOT_BASE + u32::from(k.quick_slot().unwrap_or(0)),
                ));
            }
            Confirm => out.push(Order::ConfirmTransformed),
            WeaponMode => out.push(Order::Message(msg::HOLD)),
            ToggleHud => out.push(Order::ToggleHud),
            UsePotionHp => out.push(Order::Message(msg::POTION_BASE)),
            UsePotionMp => out.push(Order::Message(msg::POTION_BASE + 1)),
            UsePotionSp => out.push(Order::Message(msg::POTION_BASE + 2)),
            _ => {}
        }
        out
    }

    /// Port of `OnPlayerAction` (S:62480): the primary key (left mouse button).
    fn primary(&mut self, m: &GameKeyMessage, ctx: &Context<'_>, out: &mut Vec<Order>) {
        if !m.is_pressed() {
            self.attack_latch = false;
        }
        match ctx.task {
            Task::Ranged => {
                if ctx.ani_state == 3 || ctx.ani_state == 4 {
                    return;
                }
                if m.is_press_edge() {
                    if matches!(ctx.left, Hand::CrossBow | Hand::Empty) {
                        // A crossbow shoots on the click; without a loaded bolt it is reloaded.
                        out.push(Order::Message(if ctx.ammo_in_hand {
                            msg::SHOOT
                        } else {
                            msg::RELOAD
                        }));
                    } else {
                        self.aimed = true;
                        out.push(Order::Message(msg::AIM));
                    }
                } else if m.is_release_edge() && ctx.left != Hand::CrossBow && self.aimed {
                    // Letting go of a drawn bow looses the arrow.
                    out.push(Order::Message(msg::SHOOT));
                }
            }
            Task::Melee => out.push(Order::MeleeAttackKey {
                press: m.is_press_edge(),
                release: m.is_release_edge(),
                duration_ms: m.duration_ms,
            }),
            Task::SitKnockDown if m.is_press_edge() => out.push(Order::Message(msg::GET_UP_ATTACK)),
            Task::Interact | Task::Normal if m.is_press_edge() => {
                out.push(Order::Message(msg::USE))
            }
            _ => {}
        }
    }

    /// Port of `OnPlayerSecondaryAction` (S:62ff0): the secondary key (right mouse button).
    fn secondary(&mut self, m: &GameKeyMessage, ctx: &Context<'_>, out: &mut Vec<Order>) {
        if !m.is_pressed() {
            self.secondary_latch = false;
            self.movement_down = false;
        }
        if ctx.state == "ZS_Talk_Loop" {
            if m.is_press_edge() {
                out.push(Order::SkipTalkLine);
            }
            return;
        }
        match ctx.task {
            Task::Melee => {
                if m.is_press_edge() {
                    self.attack_latch = false;
                } else if m.is_pressed() {
                    if m.duration_ms > PARRY_HOLD_MS {
                        if !self.secondary_latch {
                            // Hold the guard: a parade when the move is possible, a hop back
                            // otherwise.
                            out.push(Order::Message(if ctx.can_parade {
                                msg::PARADE
                            } else {
                                msg::JUMP_BACK
                            }));
                            self.secondary_latch = true;
                        } else if m.duration_ms > GUARD_RELEASE_MS && ctx.extended_hold {
                            out.push(Order::EnterState("PS_Melee_Loop"));
                            self.secondary_latch = false;
                        }
                    }
                } else if m.is_release_edge() && m.duration_ms <= TAP_MAX_MS {
                    // A tap: out of the parry it is the riposte, otherwise a quick attack, or a
                    // hop back when the quick attack is not possible.
                    if ctx.state == "PS_Melee_Parade" {
                        out.push(Order::EnterState("PS_Melee_QuickAttack"));
                    } else if ctx.can_quick_attack {
                        out.push(Order::Message(msg::QUICK_ATTACK));
                    } else {
                        out.push(Order::Message(msg::JUMP_BACK));
                    }
                }
            }
            Task::SitKnockDown if m.is_press_edge() => out.push(Order::Message(msg::GET_UP_PARADE)),
            Task::Interact if m.is_press_edge() => {
                out.push(Order::Message(msg::INTERACT_SECONDARY))
            }
            Task::Ranged if m.is_press_edge() && self.aimed => {
                // [I] The shipped DLL has no handler for it; the aim loops know the message and
                // the manual lets the right button give up the draw.
                out.push(Order::Message(msg::ABORT_ATTACK));
                self.aimed = false;
            }
            _ => {}
        }
    }

    /// Port of `OnMagicHandleInput` (S:7d360). Returns whether the key was consumed.
    fn magic_input(&mut self, m: &GameKeyMessage, ctx: &Context<'_>, out: &mut Vec<Order>) -> bool {
        if ctx.task != Task::Magic {
            return false;
        }
        if m.is_pressed() && m.was_pressed() || !m.is_pressed() && !m.was_pressed() {
            // Held: the right button held long enough puts the spell away.
            if ctx.extended_hold && m.key == SessionKey::Use2 && m.duration_ms >= MAGIC_CANCEL_MS {
                self.movement_down = false;
                out.push(Order::EnterState("PS_Magic_Loop"));
                return true;
            }
            return false;
        }
        if m.is_press_edge() {
            match m.key {
                SessionKey::Use1 => {
                    // [I] Ignored while a cast or summon runs.
                    if matches!(
                        ctx.state,
                        "PS_Magic_Cast" | "PS_Magic_PowerCast" | "PS_Magic_Summon"
                    ) {
                        return true;
                    }
                    if let Some(action) = ctx.cast_action {
                        out.push(Order::Message(action));
                    }
                    return true;
                }
                SessionKey::Use2 => {
                    self.secondary_latch = true;
                    // A staff in the left hand parries with the right button.
                    if ctx.state != "PS_Magic_Summon" && ctx.left == Hand::Staff {
                        out.push(Order::Message(msg::PARADE));
                        return true;
                    }
                }
                _ => {}
            }
            return false;
        }
        // Release edge.
        match m.key {
            SessionKey::Use1 => {
                if ctx.state == "PS_Magic_Cast" && ctx.state_time < CAST_MIN_SECS {
                    out.push(Order::EnterState("PS_Magic_Loop"));
                    return true;
                }
                false
            }
            SessionKey::Use2 => {
                self.secondary_latch = false;
                self.movement_down = false;
                false
            }
            _ => false,
        }
    }
}

/// What a message does in the loop states of `PS_Ranged`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reaction {
    /// Nothing; the message is dropped.
    Ignore,
    /// Switch to this state.
    State(&'static str),
    /// Switch to this state after putting the ammunition away (`HoldStack(-1)`).
    StateUnholdAmmo(&'static str),
    /// Quick slot `n` (`OnPlayerUseQuickSlot`).
    QuickSlot(u8),
    /// Potion quick use (`OnPlayerPotionQuickUse`) of the pool 0..2.
    Potion(u8),
    /// The weapon key: `OnPlayerSelectWeapon`.
    SelectWeapon,
}

fn common(id: u32) -> Option<Reaction> {
    match id {
        msg::HOLD => Some(Reaction::SelectWeapon),
        131..=140 => Some(Reaction::QuickSlot((id - msg::QUICK_SLOT_BASE) as u8)),
        141..=143 => Some(Reaction::Potion((id - msg::POTION_BASE) as u8)),
        _ => None,
    }
}

/// Port of the message switch of `PS_Ranged_Walk_Loop` (S:84080): the hero walks with the
/// weapon ready. `crossbow`: the left hand holds a crossbow.
pub fn ranged_walk_loop(message: u32, crossbow: bool) -> Reaction {
    match message {
        msg::ABORT_ATTACK => Reaction::StateUnholdAmmo("PS_Ranged_Reload"),
        msg::AIM => Reaction::State("PS_Ranged_Aim"),
        // The crossbow shoots from the walk state; a bow turns the press into an aim.
        msg::SHOOT => Reaction::State(if crossbow {
            "PS_Ranged_Shoot"
        } else {
            "PS_Ranged_Aim"
        }),
        msg::RELOAD => Reaction::State("PS_Ranged_Reload"),
        msg::JUMP => Reaction::State("PS_Ranged_Jump"),
        msg::SNEAK => Reaction::State("PS_Ranged_Sneak"),
        other => common(other).unwrap_or(Reaction::Ignore),
    }
}

/// Port of the message switch of `PS_Ranged_Aim_Loop` (S:84d60): the weapon is up.
pub fn ranged_aim_loop(message: u32, crossbow: bool) -> Reaction {
    match message {
        msg::ABORT_ATTACK => Reaction::StateUnholdAmmo("PS_Ranged_Reload"),
        msg::SHOOT => Reaction::State("PS_Ranged_Shoot"),
        // Only a crossbow sneaks out of the aim.
        msg::SNEAK if crossbow => Reaction::State("PS_Ranged_Sneak"),
        other => common(other).unwrap_or(Reaction::Ignore),
    }
}

/// Port of the message switch of `OnPowerAim_Loop` (S:84b90), the callback of the drawn bow:
/// the abort message gives up the draw, the shoot message looses the arrow.
pub fn power_aim_loop(message: u32) -> Reaction {
    match message {
        msg::ABORT_ATTACK => Reaction::StateUnholdAmmo("PS_Ranged_Reload"),
        msg::SHOOT => Reaction::State("PS_Ranged_Shoot"),
        _ => Reaction::Ignore,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dik;
    use crate::keys::mouse;
    use crate::mapper::{ActionMapper, RawInput};

    fn find(v: &[GameKeyMessage], k: SessionKey) -> GameKeyMessage {
        v.iter().copied().find(|m| m.key == k).expect("message")
    }

    /// Hold `Use1` for `frames` of 16 ms and return the router output of the press and release.
    fn click(
        input: &mut PlayerInput,
        ctx: &Context<'_>,
        button: u32,
        key: SessionKey,
        frames: u32,
    ) -> (Vec<Order>, Vec<Order>, Vec<Order>) {
        let mut mapper = ActionMapper::standard();
        let mut raw = RawInput::default();
        raw.set_button(button, true);
        let mut t = 1000;
        let press = input.route(&find(&mapper.process(&raw, t), key), ctx);
        let mut held = Vec::new();
        for _ in 0..frames {
            t += 16;
            held.extend(input.route(&find(&mapper.process(&raw, t), key), ctx));
        }
        raw.set_button(button, false);
        t += 16;
        let release = input.route(&find(&mapper.process(&raw, t), key), ctx);
        (press, held, release)
    }

    fn ranged(left: Hand) -> Context<'static> {
        Context {
            task: Task::Ranged,
            state: "PS_Ranged_Walk_Loop",
            left,
            ammo_in_hand: true,
            ..Context::default()
        }
    }

    #[test]
    fn a_bow_draws_while_the_button_is_down_and_shoots_on_release() {
        let mut input = PlayerInput::default();
        let (press, held, release) = click(
            &mut input,
            &ranged(Hand::Bow),
            mouse::LEFT,
            SessionKey::Use1,
            30,
        );
        assert_eq!(press, vec![Order::Message(msg::AIM)]);
        assert!(held.is_empty(), "holding the button only draws");
        assert_eq!(release, vec![Order::Message(msg::SHOOT)]);
    }

    #[test]
    fn a_crossbow_shoots_on_the_click_and_reloads_when_empty() {
        let mut input = PlayerInput::default();
        let (press, _, release) = click(
            &mut input,
            &ranged(Hand::CrossBow),
            mouse::LEFT,
            SessionKey::Use1,
            3,
        );
        assert_eq!(press, vec![Order::Message(msg::SHOOT)]);
        assert!(
            release.is_empty(),
            "the release of a crossbow click does nothing"
        );
        let mut empty = ranged(Hand::CrossBow);
        empty.ammo_in_hand = false;
        let (press, ..) = click(
            &mut PlayerInput::default(),
            &empty,
            mouse::LEFT,
            SessionKey::Use1,
            1,
        );
        assert_eq!(press, vec![Order::Message(msg::RELOAD)]);
    }

    #[test]
    fn sneaking_or_parrying_blocks_the_ranged_click() {
        let mut ctx = ranged(Hand::Bow);
        ctx.ani_state = 3;
        let (press, ..) = click(
            &mut PlayerInput::default(),
            &ctx,
            mouse::LEFT,
            SessionKey::Use1,
            1,
        );
        assert!(press.is_empty());
    }

    #[test]
    fn right_click_gives_up_a_draw() {
        let mut input = PlayerInput::default();
        let ctx = ranged(Hand::Bow);
        let mut mapper = ActionMapper::standard();
        let mut raw = RawInput::default();
        raw.set_button(mouse::LEFT, true);
        input.route(&find(&mapper.process(&raw, 0), SessionKey::Use1), &ctx);
        raw.set_button(mouse::RIGHT, true);
        let v = mapper.process(&raw, 16);
        let o = input.route(&find(&v, SessionKey::Use2), &ctx);
        assert_eq!(o, vec![Order::Message(msg::ABORT_ATTACK)]);
    }

    #[test]
    fn the_aim_loops_turn_messages_into_states() {
        assert_eq!(
            ranged_walk_loop(msg::AIM, false),
            Reaction::State("PS_Ranged_Aim")
        );
        assert_eq!(
            ranged_walk_loop(msg::SHOOT, true),
            Reaction::State("PS_Ranged_Shoot")
        );
        assert_eq!(
            ranged_walk_loop(msg::SHOOT, false),
            Reaction::State("PS_Ranged_Aim")
        );
        assert_eq!(
            ranged_aim_loop(msg::SHOOT, false),
            Reaction::State("PS_Ranged_Shoot")
        );
        assert_eq!(ranged_aim_loop(msg::SNEAK, false), Reaction::Ignore);
        assert_eq!(
            ranged_aim_loop(msg::SNEAK, true),
            Reaction::State("PS_Ranged_Sneak")
        );
        assert_eq!(
            power_aim_loop(msg::ABORT_ATTACK),
            Reaction::StateUnholdAmmo("PS_Ranged_Reload")
        );
        assert_eq!(
            power_aim_loop(msg::SHOOT),
            Reaction::State("PS_Ranged_Shoot")
        );
        assert_eq!(
            ranged_walk_loop(msg::QUICK_SLOT_BASE + 4, false),
            Reaction::QuickSlot(4)
        );
        assert_eq!(
            ranged_aim_loop(msg::POTION_BASE + 2, false),
            Reaction::Potion(2)
        );
        assert_eq!(ranged_walk_loop(msg::HOLD, false), Reaction::SelectWeapon);
    }

    #[test]
    fn the_right_button_held_in_melee_parries_and_a_tap_attacks_quickly() {
        let ctx = Context {
            task: Task::Melee,
            state: "PS_Melee_Loop",
            ..Context::default()
        };
        let (press, held, release) = click(
            &mut PlayerInput::default(),
            &ctx,
            mouse::RIGHT,
            SessionKey::Use2,
            20,
        );
        assert!(press.is_empty());
        assert_eq!(
            held,
            vec![Order::Message(msg::PARADE)],
            "one parade after 50 ms"
        );
        assert!(release.is_empty(), "a held guard is not a tap");
        // A quick tap is a quick attack.
        let (_, _, tap) = click(
            &mut PlayerInput::default(),
            &ctx,
            mouse::RIGHT,
            SessionKey::Use2,
            2,
        );
        assert_eq!(tap, vec![Order::Message(msg::QUICK_ATTACK)]);
        // Without the move it is a hop back.
        let mut stuck = ctx.clone();
        stuck.can_quick_attack = false;
        let (_, _, tap) = click(
            &mut PlayerInput::default(),
            &stuck,
            mouse::RIGHT,
            SessionKey::Use2,
            2,
        );
        assert_eq!(tap, vec![Order::Message(msg::JUMP_BACK)]);
        // Out of the parry a tap is the riposte.
        let parry = Context {
            state: "PS_Melee_Parade",
            ..ctx
        };
        let (_, _, tap) = click(
            &mut PlayerInput::default(),
            &parry,
            mouse::RIGHT,
            SessionKey::Use2,
            2,
        );
        assert_eq!(tap, vec![Order::EnterState("PS_Melee_QuickAttack")]);
    }

    #[test]
    fn the_left_button_is_the_use_key_in_the_normal_state() {
        let (press, ..) = click(
            &mut PlayerInput::default(),
            &Context::default(),
            mouse::LEFT,
            SessionKey::Use1,
            1,
        );
        assert_eq!(press, vec![Order::Message(msg::USE)]);
    }

    #[test]
    fn magic_the_click_casts_and_a_short_hold_aborts() {
        let mut ctx = Context {
            task: Task::Magic,
            state: "PS_Magic_Loop",
            cast_action: Some(msg::CAST),
            ..Context::default()
        };
        let (press, ..) = click(
            &mut PlayerInput::default(),
            &ctx,
            mouse::LEFT,
            SessionKey::Use1,
            1,
        );
        assert_eq!(press, vec![Order::Message(msg::CAST)]);
        ctx.state = "PS_Magic_Cast";
        ctx.state_time = 0.3;
        let (press, _, release) = click(
            &mut PlayerInput::default(),
            &ctx,
            mouse::LEFT,
            SessionKey::Use1,
            1,
        );
        assert!(press.is_empty(), "no second cast while casting");
        assert_eq!(release, vec![Order::EnterState("PS_Magic_Loop")]);
        ctx.state_time = 0.9;
        let (_, _, release) = click(
            &mut PlayerInput::default(),
            &ctx,
            mouse::LEFT,
            SessionKey::Use1,
            1,
        );
        assert!(release.is_empty(), "a held cast runs on");
    }

    #[test]
    fn a_staff_parries_with_the_right_button() {
        let ctx = Context {
            task: Task::Magic,
            state: "PS_Magic_Loop",
            left: Hand::Staff,
            ..Context::default()
        };
        let (press, ..) = click(
            &mut PlayerInput::default(),
            &ctx,
            mouse::RIGHT,
            SessionKey::Use2,
            1,
        );
        assert_eq!(press, vec![Order::Message(msg::PARADE)]);
    }

    #[test]
    fn keys_of_the_standard_layout() {
        let mut input = PlayerInput::default();
        let mut mapper = ActionMapper::standard();
        let ctx = Context::default();
        let mut run = |key: u8| {
            let mut raw = RawInput::default();
            raw.set_key(key, true);
            let v = mapper.process(&raw, 0);
            let out: Vec<Order> = v.iter().flat_map(|m| input.route(m, &ctx)).collect();
            mapper.process(&RawInput::default(), 16);
            out
        };
        assert_eq!(run(dik::SPACE), vec![Order::Message(msg::HOLD)]);
        assert_eq!(run(dik::D5), vec![Order::Message(msg::QUICK_SLOT_BASE + 5)]);
        assert_eq!(run(dik::D0), vec![Order::Message(msg::QUICK_SLOT_BASE)]);
        assert_eq!(run(dik::LMENU), vec![Order::Message(msg::JUMP)]);
        assert_eq!(
            run(dik::NUMPAD8),
            vec![Order::Message(msg::POTION_BASE + 1)]
        );
        let duck = run(dik::LCONTROL);
        assert_eq!(
            duck,
            vec![Order::Message(msg::SNEAK), Order::SneakHeld(true)]
        );
        // Shift is the walk modifier.
        assert_eq!(run(dik::LSHIFT), vec![Order::Walk(true)]);
    }

    #[test]
    fn walking_needs_movement_and_ends_on_release() {
        let mut input = PlayerInput::default();
        let mut mapper = ActionMapper::standard();
        let ctx = Context {
            standing_or_braking: true,
            ..Context::default()
        };
        let mut raw = RawInput::default();
        raw.set_key(dik::LSHIFT, true);
        let v = mapper.process(&raw, 0);
        assert_eq!(
            input.route(&find(&v, SessionKey::Walk), &ctx),
            vec![Order::Walk(false)]
        );
        raw.set_key(dik::LSHIFT, false);
        let v = mapper.process(&raw, 16);
        assert_eq!(
            input.route(&find(&v, SessionKey::Walk), &ctx),
            vec![Order::Walk(false)]
        );
    }

    #[test]
    fn dead_players_and_open_pages_take_no_keys() {
        let ctx = Context {
            keys_ignored: true,
            ..Context::default()
        };
        let (press, ..) = click(
            &mut PlayerInput::default(),
            &ctx,
            mouse::LEFT,
            SessionKey::Use1,
            1,
        );
        assert!(press.is_empty());
    }
}
