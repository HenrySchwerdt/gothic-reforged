//! Falling, regeneration, resource rates, stamina costs of moves, unconscious recovery and
//! area-spell damage curves [CA 5, CA 9.4, CA 9.5, CA 9.7].
use crate::actor::Actor;
use crate::config::SimConfig;
use crate::types::*;

/// Fall damage on landing after `t` seconds in the fall state: `floor(160*(t-0.9)^2)` for
/// `t > 0.9`, halved with `Perk_Acrobatic` [CA 5.1]. Examples: 1.5 s -> 57, 2 s -> 193.
pub fn fall_damage(t_secs: f32, acrobatic: bool) -> i32 {
    if t_secs <= 0.9 {
        return 0;
    }
    let d = (160.0 * (t_secs - 0.9).powi(2)).floor() as i32;
    if acrobatic { d / 2 } else { d }
}

/// What an actor is doing, for the stamina tick [CA 5.2].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Movement {
    #[default]
    Idle,
    Sprinting,
    Jumping,
    /// Using an interaction object.
    Interacting,
}

/// Environment of a regeneration tick.
#[derive(Debug, Clone, Copy, Default)]
pub struct TickEnv {
    pub movement: Movement,
    pub in_magic_barrier: bool,
    pub gui_open: bool,
    /// Zone temperature (degrees) [CA 5.2].
    pub temperature: i32,
}

/// HP change per regeneration tick (about one per second [CA 5.2, I on the interval])
/// [CA 5.2, S:b0360]. `Perk_MasterGladiator`: ExtendedContent `min(HPmax/80, 5)` but 0 while
/// in combat, otherwise a flat 5. Without the perk only sitting regenerates: `min(HPmax/100, 5)`.
/// Then -10 inside a magic barrier, -5 poisoned, -5 burning.
pub fn hp_tick(cfg: &SimConfig, a: &Actor, env: &TickEnv) -> i32 {
    if a.ai_mode == AiMode::Dead {
        return 0;
    }
    if a.is_player && cfg.extended_content && env.gui_open {
        return 0;
    }
    let hp_max = a.hp_max(cfg);
    let mut d = if a.has_perk("perk_mastergladiator") {
        if cfg.extended_content {
            if a.ai_mode == AiMode::Combat {
                0
            } else {
                (hp_max / 80).min(5)
            }
        } else {
            5
        }
    } else if a.ani_state.is_sitting() {
        (hp_max / 100).min(5)
    } else {
        0
    };
    if d == 0
        && a.ai_mode != AiMode::Combat
        && let Some(n) = cfg.variants.passive_hp_regen
    {
        d = n;
    }
    if env.in_magic_barrier {
        d -= 10;
    }
    if a.status & status::POISONED != 0 {
        d -= 5;
    }
    if a.status & status::BURNING != 0 {
        d -= 5;
    }
    d
}

/// Stamina change per tick [CA 5.2, S:b0520]: sprinting -10 (`Perk_Sprinter` -5), with
/// ExtendedContent -8 (-4); interaction -1; jumping 0; diseased +1; extreme temperature +2 only
/// with the matching resist perk; otherwise +10 [I on the precedence of the temperature clause].
pub fn stamina_tick(cfg: &SimConfig, a: &Actor, env: &TickEnv) -> i32 {
    match env.movement {
        Movement::Sprinting => {
            let sprinter = a.has_perk("perk_sprinter");
            return match (cfg.extended_content, sprinter) {
                (false, false) => -10,
                (false, true) => -5,
                (true, false) => -8,
                (true, true) => -4,
            };
        }
        Movement::Interacting => return -1,
        Movement::Jumping => return 0,
        Movement::Idle => {}
    }
    if a.species == Species::Bloodfly {
        return -1;
    }
    if a.status & status::DISEASED != 0 {
        return 1;
    }
    if env.temperature > 40 {
        return if a.has_perk("perk_resistheat") { 2 } else { 10 };
    }
    if env.temperature < -40 {
        return if a.has_perk("perk_resistcold") { 2 } else { 10 };
    }
    10
}

/// Apply one regeneration tick (HP, stamina, clamped to the maxima). Death by poison / fire
/// is the caller's decision (it sees `hp <= 0`).
pub fn apply_tick(cfg: &SimConfig, a: &mut Actor, env: &TickEnv) {
    let hp = hp_tick(cfg, a, env);
    let st = stamina_tick(cfg, a, env);
    a.hp = (a.hp + hp).min(a.hp_max(cfg));
    a.stamina = (a.stamina + st).clamp(0, a.stamina_max(cfg));
}

/// Convert the simulation's action to the script port's by name.
pub fn script_action(a: Action) -> gothic_script::enums::Action {
    gothic_script::enums::Action::from_name(&format!("{a:?}"))
        .unwrap_or(gothic_script::enums::Action::None)
}

/// Stamina cost of starting a move: the table of the ported `_AI_*` commands
/// (`gothic_script::combat::commands`, [CA 9.5]). A hack pays its second 20 when its second
/// stage begins ([`second_stage_cost`]).
pub fn stamina_cost(cfg: &SimConfig, action: Action) -> i32 {
    gothic_script::combat::commands::stamina_cost(cfg.extended_content, script_action(action))
}

/// The stamina the second stage of a move costs (a hack's overhead stage).
pub fn second_stage_cost(action: Action) -> i32 {
    gothic_script::combat::commands::second_stage_cost(script_action(action))
}

/// Seconds after which the hit window of a move opens (the `OnAI_*` thresholds of the port).
pub fn hit_window_secs(action: Action) -> f32 {
    gothic_script::combat::commands::hit_window_secs(script_action(action))
}

/// Seconds before an unconscious actor wakes: 5 (player or party), 20 (arena defeat), else 30
/// [CA 9.4, S:26f70].
pub fn unconscious_secs(by_player_party: bool, arena: bool) -> u32 {
    if by_player_party {
        5
    } else if arena {
        20
    } else {
        30
    }
}

/// Per-second recovery while unconscious: +10 HP (+15 after an arena defeat) and +10 stamina
/// [CA 9.4].
pub fn unconscious_tick(cfg: &SimConfig, a: &mut Actor, arena: bool) {
    a.hp = (a.hp + if arena { 15 } else { 10 }).min(a.hp_max(cfg));
    a.stamina = (a.stamina + 10).min(a.stamina_max(cfg));
}

/// Sweep spells: damage falls linearly from `centre` at the caster to 1 at `radius`
/// [CA 9.7]. `None` outside the radius.
pub fn sweep_damage(centre: i32, radius: f32, dist: f32) -> Option<i32> {
    if dist > radius || radius <= 0.0 {
        return None;
    }
    let t = dist / radius;
    Some(((centre as f32 - (centre - 1) as f32 * t) as i32).max(1))
}

/// Sweep spell table: (damage type, centre damage, radius, delay ms) [CA 9.7].
pub fn sweep_spell(name: &str) -> Option<(DamageType, i32, f32, u32)> {
    Some(match name.to_ascii_lowercase().as_str() {
        "heatwave" => (DamageType::Fire, 400, 1000.0, 0),
        "frostwave" => (DamageType::Ice, 200, 1000.0, 0),
        "meteor" => (DamageType::Fire, 1000, 500.0, 500),
        "lightning" => (DamageType::Lightning, 1000, 500.0, 500),
        "icebomb" => (DamageType::Missile, 600, 1000.0, 0),
        _ => return None,
    })
}

/// Hailstorm / Firerain: every 400 ms a hit chance of `33 + min(n,70)` percent where `n` is
/// the caster's INT-derived value (the doc does not name it; INT is assumed [I]); one random
/// target within 1000 takes `50 + rand(50)`; duration `8 + rand(4)` s, +50 percent with the
/// school perk [CA 9.7].
pub fn area_hit_chance_pct(n: i32) -> i32 {
    33 + n.clamp(0, 70)
}

pub fn area_duration_secs(rng_roll_0_to_3: i32, school_perk: bool) -> f32 {
    let base = 8.0 + rng_roll_0_to_3 as f32;
    if school_perk { base * 1.5 } else { base }
}

/// `GetTransformDuration` (S:582a0): `max(INT-60, 0)+60` seconds, halved for large forms, x2
/// with `Perk_Druid`, +0.5x with the water-mage perk [CA 9.7].
pub fn transform_duration_secs(int: i32, large: bool, druid: bool, watermage: bool) -> f32 {
    let mut d = ((int - 60).max(0) + 60) as f32;
    if large {
        d /= 2.0;
    }
    let mut m = 1.0;
    if druid {
        m *= 2.0;
    }
    if watermage {
        m += 0.5;
    }
    d * m
}

/// `MagicKeepStamina` / `MagicSpeedUp` duration `(StaminaMax-200)/2 + 60` seconds, min 60
/// [CA 5.2 / 9.7].
pub fn speed_up_secs(stamina_max: i32) -> i32 {
    ((stamina_max - 200) / 2 + 60).max(60)
}

/// `MagicCallAnimals` count: INT > 300 -> 3, > 200 -> 2, else 1 [CA 9.7].
pub fn call_animals_count(int: i32) -> u32 {
    if int > 300 {
        3
    } else if int > 200 {
        2
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fall_damage_examples() {
        // [CA 5.1] 1.5 s -> 57, 2 s -> 193; safe up to 0.9 s; Acrobatic halves.
        assert_eq!(fall_damage(0.9, false), 0);
        assert_eq!(fall_damage(1.5, false), 57);
        assert_eq!(fall_damage(2.0, false), 193);
        assert_eq!(fall_damage(2.0, true), 96);
        assert_eq!(fall_damage(0.0, false), 0);
    }

    #[test]
    fn hp_regeneration_rules() {
        let cfg = SimConfig::default();
        let mut h = Actor::hero(ActorId(0), &cfg);
        let env = TickEnv::default();
        // No passive regeneration without the perk [CA 5.2].
        assert_eq!(hp_tick(&cfg, &h, &env), 0);
        // Sitting: min(200/100, 5) = 2.
        h.ani_state = AniState::SitBench;
        assert_eq!(hp_tick(&cfg, &h, &env), 2);
        // Master Gladiator, extended: min(200/80, 5) = 2, zero in combat; vanilla flat 5.
        h.ani_state = AniState::Stand;
        h.perks.insert("perk_mastergladiator".into());
        assert_eq!(hp_tick(&cfg, &h, &env), 2);
        h.ai_mode = AiMode::Combat;
        assert_eq!(hp_tick(&cfg, &h, &env), 0);
        assert_eq!(hp_tick(&SimConfig::vanilla(), &h, &env), 5);
        // Poison and burning subtract; a magic barrier hurts.
        h.ai_mode = AiMode::Routine;
        h.status = status::POISONED | status::BURNING;
        assert_eq!(hp_tick(&cfg, &h, &env), 2 - 10);
        let barrier = TickEnv {
            in_magic_barrier: true,
            ..env
        };
        h.status = 0;
        assert_eq!(hp_tick(&cfg, &h, &barrier), 2 - 10);
        // GUI open suppresses the player's regeneration under ExtendedContent.
        let gui = TickEnv {
            gui_open: true,
            ..env
        };
        assert_eq!(hp_tick(&cfg, &h, &gui), 0);
    }

    #[test]
    fn passive_regen_variant_is_off_by_default() {
        let mut cfg = SimConfig::default();
        let h = Actor::hero(ActorId(0), &cfg);
        assert_eq!(hp_tick(&cfg, &h, &TickEnv::default()), 0);
        cfg.variants.passive_hp_regen = Some(1);
        assert_eq!(hp_tick(&cfg, &h, &TickEnv::default()), 1);
    }

    #[test]
    fn stamina_rates() {
        let cfg = SimConfig::default();
        let mut h = Actor::hero(ActorId(0), &cfg);
        let sprint = TickEnv {
            movement: Movement::Sprinting,
            ..TickEnv::default()
        };
        assert_eq!(stamina_tick(&cfg, &h, &sprint), -8);
        assert_eq!(stamina_tick(&SimConfig::vanilla(), &h, &sprint), -10);
        h.perks.insert("perk_sprinter".into());
        assert_eq!(stamina_tick(&cfg, &h, &sprint), -4);
        assert_eq!(stamina_tick(&SimConfig::vanilla(), &h, &sprint), -5);
        assert_eq!(stamina_tick(&cfg, &h, &TickEnv::default()), 10);
        h.status = status::DISEASED;
        assert_eq!(stamina_tick(&cfg, &h, &TickEnv::default()), 1);
        // apply_tick clamps to the maximum.
        h.status = 0;
        h.stamina = 145;
        apply_tick(&cfg, &mut h, &TickEnv::default());
        assert_eq!(h.stamina, 150);
    }

    #[test]
    fn move_costs() {
        let ext = SimConfig::default();
        let van = SimConfig::vanilla();
        assert_eq!(stamina_cost(&ext, Action::Attack), 12);
        assert_eq!(stamina_cost(&van, Action::Attack), 10);
        assert_eq!(stamina_cost(&ext, Action::PowerAttack), 20);
        assert_eq!(stamina_cost(&ext, Action::HackAttack), 20);
        assert_eq!(second_stage_cost(Action::HackAttack), 20);
        assert_eq!(stamina_cost(&ext, Action::JumpBack), 0);
        assert_eq!(stamina_cost(&van, Action::JumpBack), 10);
        assert_eq!(stamina_cost(&ext, Action::Parade), 0);
        assert_eq!(hit_window_secs(Action::PierceAttack), 0.8);
        assert_eq!(hit_window_secs(Action::QuickAttack), 0.1);
    }

    #[test]
    fn unconscious_rules() {
        let cfg = SimConfig::default();
        assert_eq!(unconscious_secs(true, false), 5);
        assert_eq!(unconscious_secs(false, true), 20);
        assert_eq!(unconscious_secs(false, false), 30);
        let mut o = Actor::npc(ActorId(1), "O", Species::Orc, 10, 30);
        o.hp = 1;
        o.stamina = 0;
        unconscious_tick(&cfg, &mut o, false);
        assert_eq!((o.hp, o.stamina), (11, 10));
        unconscious_tick(&cfg, &mut o, true);
        assert_eq!(o.hp, 26);
    }

    #[test]
    fn spell_curves() {
        // Heatwave: 400 at the caster, 1 at the edge, linear in between.
        assert_eq!(sweep_damage(400, 1000.0, 0.0), Some(400));
        assert_eq!(sweep_damage(400, 1000.0, 1000.0), Some(1));
        assert_eq!(sweep_damage(400, 1000.0, 500.0), Some(200));
        assert_eq!(sweep_damage(400, 1000.0, 1001.0), None);
        assert_eq!(
            sweep_spell("Meteor"),
            Some((DamageType::Fire, 1000, 500.0, 500))
        );
        assert_eq!(area_hit_chance_pct(100), 103);
        assert_eq!(area_hit_chance_pct(10), 43);
        assert_eq!(area_duration_secs(2, false), 10.0);
        assert_eq!(area_duration_secs(2, true), 15.0);
        // Transform duration: INT 160 -> 160 s; large -> 80; druid x2.
        assert_eq!(transform_duration_secs(160, false, false, false), 160.0);
        assert_eq!(transform_duration_secs(160, true, true, false), 160.0);
        assert_eq!(transform_duration_secs(10, false, false, true), 90.0);
        assert_eq!(speed_up_secs(150), 60);
        assert_eq!(speed_up_secs(400), 160);
        assert_eq!(call_animals_count(250), 2);
    }
}
