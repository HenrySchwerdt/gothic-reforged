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
    pub const POWER_ATTACK: u32 = 2;
    pub const QUICK_ATTACK: u32 = 3;
    pub const WHIRL_ATTACK: u32 = 10;
    pub const PIERCE_ATTACK: u32 = 11;
    pub const HACK_ATTACK: u32 = 14;
    pub const FINISHING_ATTACK: u32 = 15;
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
    /// What `OnPlayerAction` / `OnPlayerSecondaryAction` ask the melee layer (`CanDo`, the
    /// weapon-class helpers and the finishing check).
    pub melee: MeleeFacts,
    /// The spell in hand: its `GetCastAction` (S:52a10), `None` when unusable.
    pub cast_action: Option<u32>,
    /// `eSSetupEngine` flag at +0x1eb: the extended hold controls of the options.
    pub extended_hold: bool,
}

/// The facts of the melee layer the key handlers read (`Script_Game.dll`, `Script/Player/
/// Player_Interface/OnPlayerGameKeyPressed.cpp`). The host fills them from the ported script
/// functions, never from guesses.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MeleeFacts {
    /// Bit `n` is `CanDo(self, None, gEAction n)` (S:4f80) for the action ids below 64.
    pub can_do: u64,
    /// S:4260: the item in the right-hand slot has use type `OneH` (a pierce, not a hack).
    pub right_one_hander: bool,
    /// S:4220 (`sub_3050`): the right-hand weapon is a two-hander (use types 3, 12, 51, 52) or a
    /// swing tool (26, 29, 36, 49): the tap with the guard up is a whirl, not a quick attack.
    pub two_hander_or_tool: bool,
    /// S:41a0: the left-hand item is a one-hander or a torch (use types 2, 24): a short power
    /// attack turns into a simple whirl.
    pub left_one_hander_or_torch: bool,
    /// The focus entity is knocked down (`AIMode` 8), `CanBeKilled` and within 300 cm
    /// (S:62480, the hold branch with the guard up).
    pub finishing_target: bool,
}

impl MeleeFacts {
    /// `CanDo(action)` for the action id.
    pub const fn can_do(&self, action: u32) -> bool {
        action < 64 && self.can_do & (1u64 << action) != 0
    }
}

/// Seconds of a power attack within which letting go of the key turns it into a (simple) whirl
/// or a plain attack. Read from the constant pool of `Script_Game.dll`: the double at 0x101076c8
/// (compared in `OnPlayerAction`, S:62a95) is 0.20000000298023224 = the f32 0.2 widened; the
/// float at 0x10107468 (S:62ace) is 0.5.
pub const POWER_RELEASE_EARLY_SECS: f32 = 0.2;
/// See [`POWER_RELEASE_EARLY_SECS`].
pub const POWER_RELEASE_LATE_SECS: f32 = 0.5;
/// A release within this many seconds of a finishing or pierce attack's start chains a quick
/// attack (float at 0x10107634 of `Script_Game.dll`, compared at S:62b43: 0.699999988 = f32 0.7).
pub const CHAIN_QUICK_SECS: f32 = 0.7;
/// A release within this many seconds of a hack's start chains a whirl attack (the same double
/// at 0x101076c8 as [`POWER_RELEASE_EARLY_SECS`], compared at S:62bd4).
pub const CHAIN_WHIRL_SECS: f32 = 0.2;

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
            melee: MeleeFacts::default(),
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
        gothic_portcov::hit!("route");
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
        gothic_portcov::hit!("primary");
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
            Task::Melee => self.melee_primary(m, ctx, out),
            Task::SitKnockDown if m.is_press_edge() => out.push(Order::Message(msg::GET_UP_ATTACK)),
            Task::Interact | Task::Normal if m.is_press_edge() => {
                out.push(Order::Message(msg::USE))
            }
            _ => {}
        }
    }

    /// Port of the `PS_Melee` branch of `OnPlayerAction` (S:62480..S:62cb2): which attack the
    /// primary key (left mouse button) asks for.
    ///
    /// * press edge: nothing is queued (the focus entity is only remembered);
    /// * held beyond 100 ms, once (`DAT_10118b40`): with the guard up (`DAT_10118b41`, set by
    ///   `OnPlayerSecondaryAction`) a finishing blow on a knocked-down, killable focus within
    ///   300 cm, else a pierce for a one-hander and a hack for any other weapon; without the
    ///   guard a power attack;
    /// * release: out of `PS_Melee_PowerAttack` within 0.2 s a plain attack, between 0.2 and 0.5
    ///   s a simple whirl when the left hand holds a one-hander or torch (else a plain attack);
    ///   out of `PS_Melee_FinishingAttack` / `PS_Melee_PierceAttack` within 0.7 s a quick
    ///   attack; out of `PS_Melee_HackAttack` within 0.2 s a whirl attack; in any other state,
    ///   for a press of at most 200 ms: with the guard up a whirl attack (two-hander or tool) or a
    ///   quick attack, without it a plain attack.
    ///
    /// The queued action only counts when `CanDo(action)` (S:4f80) allows it; the state
    /// switches of the release branch are direct (`FullStop` + `SetState`).
    fn melee_primary(&mut self, m: &GameKeyMessage, ctx: &Context<'_>, out: &mut Vec<Order>) {
        gothic_portcov::hit!("melee_primary");
        let mut action = 0u32;
        if m.is_pressed() {
            if !m.was_pressed() {
                return;
            }
            if m.duration_ms > CHARGE_HOLD_MS && !self.attack_latch {
                self.attack_latch = true;
                if self.secondary_latch {
                    action = if ctx.melee.finishing_target {
                        msg::FINISHING_ATTACK
                    } else if ctx.melee.right_one_hander {
                        msg::PIERCE_ATTACK
                    } else {
                        msg::HACK_ATTACK
                    };
                } else {
                    action = msg::POWER_ATTACK;
                }
            }
        } else {
            let t = ctx.state_time;
            match ctx.state {
                "PS_Melee_PowerAttack" => {
                    if t > POWER_RELEASE_EARLY_SECS {
                        if t > POWER_RELEASE_LATE_SECS {
                            return;
                        }
                        if ctx.melee.left_one_hander_or_torch {
                            out.push(Order::EnterState("PS_Melee_SimpleWhirl"));
                            return;
                        }
                    }
                    out.push(Order::EnterState("PS_Melee_Attack"));
                    return;
                }
                "PS_Melee_FinishingAttack" | "PS_Melee_PierceAttack" => {
                    if t <= CHAIN_QUICK_SECS {
                        out.push(Order::EnterState("PS_Melee_QuickAttack"));
                    }
                    return;
                }
                "PS_Melee_HackAttack" => {
                    if t <= CHAIN_WHIRL_SECS {
                        out.push(Order::EnterState("PS_Melee_WhirlAttack"));
                    }
                    return;
                }
                _ => {
                    if m.duration_ms <= TAP_MAX_MS {
                        action = if self.secondary_latch {
                            if ctx.melee.two_hander_or_tool {
                                msg::WHIRL_ATTACK
                            } else {
                                msg::QUICK_ATTACK
                            }
                        } else {
                            msg::ATTACK
                        };
                    }
                }
            }
        }
        if action != 0 && ctx.melee.can_do(action) {
            out.push(Order::Message(action));
        }
    }

    /// Port of `OnPlayerSecondaryAction` (S:62ff0): the secondary key (right mouse button).
    fn secondary(&mut self, m: &GameKeyMessage, ctx: &Context<'_>, out: &mut Vec<Order>) {
        gothic_portcov::hit!("secondary");
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
                            out.push(Order::Message(if ctx.melee.can_do(msg::PARADE) {
                                msg::PARADE
                            } else {
                                msg::JUMP_BACK
                            }));
                            self.secondary_latch = true;
                        } else if m.duration_ms > GUARD_RELEASE_MS && ctx.extended_hold {
                            // The latch (`DAT_10118b41`) stays set: the held key does not raise
                            // the guard again until it is let go (S:62ff0, 0x100633bb).
                            out.push(Order::EnterState("PS_Melee_Loop"));
                        }
                    }
                } else if m.is_release_edge() && m.duration_ms <= TAP_MAX_MS {
                    // A tap: out of the parry it is the riposte, otherwise a quick attack, or a
                    // hop back when the quick attack is not possible.
                    if ctx.state == "PS_Melee_Parade" {
                        out.push(Order::EnterState("PS_Melee_QuickAttack"));
                    } else if ctx.melee.can_do(msg::QUICK_ATTACK) {
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
        gothic_portcov::hit!("magic_input");
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
    gothic_portcov::hit!("ranged_walk_loop");
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
    gothic_portcov::hit!("ranged_aim_loop");
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
    gothic_portcov::hit!("power_aim_loop");
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

    /// A melee context where every move of the weapon is allowed.
    fn melee_ctx(state: &'static str, state_time: f32) -> Context<'static> {
        Context {
            task: Task::Melee,
            state,
            state_time,
            melee: MeleeFacts {
                can_do: u64::MAX,
                right_one_hander: true,
                ..MeleeFacts::default()
            },
            ..Context::default()
        }
    }

    /// Hold the left button `frames` x 16 ms and return the orders of the last held frame and of
    /// the release (the guard latch is set by `guard` frames of the right button first).
    fn left(input: &mut PlayerInput, ctx: &Context<'_>, frames: u32) -> (Vec<Order>, Vec<Order>) {
        let (_, held, release) = click(input, ctx, mouse::LEFT, SessionKey::Use1, frames);
        (held, release)
    }

    #[test]
    fn the_attack_key_in_melee_follows_the_original_table() {
        let ctx = melee_ctx("PS_Melee_Loop", 0.0);
        // A tap (<= 200 ms) is a plain attack, queued on the release.
        let (held, rel) = left(&mut PlayerInput::default(), &ctx, 3);
        assert!(held.is_empty());
        assert_eq!(rel, vec![Order::Message(msg::ATTACK)]);
        // Held beyond 100 ms: a power attack at once, once; the release adds nothing.
        let (held, rel) = left(&mut PlayerInput::default(), &ctx, 20);
        assert_eq!(held, vec![Order::Message(msg::POWER_ATTACK)]);
        assert!(
            rel.is_empty(),
            "a long press releases into the running state: {rel:?}"
        );
        // CanDo gates the queue entry.
        let mut no = ctx.clone();
        no.melee.can_do &= !(1u64 << msg::ATTACK);
        let (_, rel) = left(&mut PlayerInput::default(), &no, 3);
        assert!(rel.is_empty());
    }

    #[test]
    fn with_the_guard_up_the_attack_key_pierces_hacks_whirls_or_finishes() {
        let mut input = PlayerInput::default();
        // The guard latch (`DAT_10118b41`) is set by a held right button.
        let ctx = melee_ctx("PS_Melee_Loop", 0.0);
        let _ = click(&mut input, &ctx, mouse::RIGHT, SessionKey::Use2, 20);
        // The right button went up again, which clears the latch; set it as the held guard does.
        input.secondary_latch = true;
        let (held, _) = left(&mut input, &ctx, 20);
        assert_eq!(
            held,
            vec![Order::Message(msg::PIERCE_ATTACK)],
            "a one-hander pierces"
        );
        input.secondary_latch = true;
        let mut two = ctx.clone();
        two.melee.right_one_hander = false;
        let (held, _) = left(&mut input, &two, 20);
        assert_eq!(held, vec![Order::Message(msg::HACK_ATTACK)]);
        input.secondary_latch = true;
        let mut fin = ctx.clone();
        fin.melee.finishing_target = true;
        let (held, _) = left(&mut input, &fin, 20);
        assert_eq!(held, vec![Order::Message(msg::FINISHING_ATTACK)]);
        // A tap with the guard up: whirl for a two-hander, quick attack otherwise.
        input.secondary_latch = true;
        two.melee.two_hander_or_tool = true;
        let (_, rel) = left(&mut input, &two, 3);
        assert_eq!(rel, vec![Order::Message(msg::WHIRL_ATTACK)]);
        input.secondary_latch = true;
        let (_, rel) = left(&mut input, &ctx, 3);
        assert_eq!(rel, vec![Order::Message(msg::QUICK_ATTACK)]);
    }

    #[test]
    fn releasing_the_key_chains_the_moves_by_state_and_time() {
        let mut state = |s: &'static str, t: f32, one: bool| {
            let mut c = melee_ctx(s, t);
            c.melee.left_one_hander_or_torch = one;
            left(&mut PlayerInput::default(), &c, 20).1
        };
        assert_eq!(
            state("PS_Melee_PowerAttack", 0.1, false),
            vec![Order::EnterState("PS_Melee_Attack")]
        );
        assert_eq!(
            state("PS_Melee_PowerAttack", 0.4, false),
            vec![Order::EnterState("PS_Melee_Attack")]
        );
        assert_eq!(
            state("PS_Melee_PowerAttack", 0.4, true),
            vec![Order::EnterState("PS_Melee_SimpleWhirl")]
        );
        assert!(
            state("PS_Melee_PowerAttack", 0.6, true).is_empty(),
            "the full charge runs out"
        );
        assert_eq!(
            state("PS_Melee_PierceAttack", 0.5, false),
            vec![Order::EnterState("PS_Melee_QuickAttack")]
        );
        assert_eq!(
            state("PS_Melee_FinishingAttack", 0.69, false),
            vec![Order::EnterState("PS_Melee_QuickAttack")]
        );
        assert!(state("PS_Melee_PierceAttack", 0.8, false).is_empty());
        assert_eq!(
            state("PS_Melee_HackAttack", 0.1, false),
            vec![Order::EnterState("PS_Melee_WhirlAttack")]
        );
        assert!(state("PS_Melee_HackAttack", 0.3, false).is_empty());
    }

    #[test]
    fn the_right_button_held_in_melee_parries_and_a_tap_attacks_quickly() {
        let ctx = melee_ctx("PS_Melee_Loop", 0.0);
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
        stuck.melee.can_do &= !(1u64 << msg::QUICK_ATTACK);
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
