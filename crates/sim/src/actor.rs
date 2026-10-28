//! The simulated character record and everything derived from it: attributes, hit points,
//! stamina, protection (NPCs derive all of it from their level [CA 2.13]; the player keeps
//! stored attributes plus equipment).
use crate::brain::Brain;
use crate::config::SimConfig;
use crate::item::{DamageDef, Equipment};
use crate::types::*;
use gothic_gameplay::inventory::{Inventory, StatKind};
use gothic_gameplay::stats::{self, Stat, StatMods};
use std::collections::BTreeSet;

/// Player attribute record (`PSPlayerMemory`) [CA 2.13 "New-game player values"].
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerAttrs {
    pub str: i32,
    pub dex: i32,
    pub int: i32,
    pub smithing: i32,
    pub theft: i32,
    pub alchemy: i32,
    pub hp_max: i32,
    pub mana_max: i32,
    pub stamina_max: i32,
}

impl PlayerAttrs {
    /// `OnGameStartUp` values: HP 200, mana 100, stamina 150 (ExtendedContent) or 100,
    /// STR 100, DEX 100, INT 0, trades 10 [CA 2.13, V S:ad180].
    pub fn new_game(cfg: &SimConfig) -> Self {
        Self {
            str: 100,
            dex: 100,
            int: 0,
            smithing: 10,
            theft: 10,
            alchemy: 10,
            hp_max: 200,
            mana_max: 100,
            stamina_max: if cfg.extended_content { 150 } else { 100 },
        }
    }
}

/// `LastFightAgainstPlayer` [CA 9.1, 10.4]: Cancel 3 and Running 4 are read from the binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FightVsPlayer {
    #[default]
    None,
    Running,
    Cancel,
    Lost,
    Won,
}

#[derive(Debug, Clone)]
pub struct Actor {
    pub id: ActorId,
    pub name: String,
    pub is_player: bool,
    /// Transformed player uses the NPC formulas [CA 2.5, 2.13].
    pub transformed: bool,
    pub species: Species,
    pub class: NpcClass,
    pub alignment: Alignment,
    pub npc_type: NpcType,
    pub enclave: Option<String>,
    /// `Level` / `LevelMax` properties (NPCs); the player's level lives in the gameplay state.
    pub level: i32,
    pub level_max: i32,
    pub base: PlayerAttrs,
    /// Temporary modifiers of worn equipment and consumables (`Modifier` and
    /// `MaximumModifier` of the player's attributes, see `gothic_gameplay::stats`).
    pub stats: StatMods,
    pub hp: i32,
    pub stamina: i32,
    pub mana: i32,
    pub equipment: Equipment,
    pub inventory: Inventory,
    /// Lower-case skill/perk names (`perk_1h_2`).
    pub perks: BTreeSet<String>,
    pub ai_mode: AiMode,
    pub action: Action,
    pub ani_state: AniState,
    /// `StatePosition` of the attacker, read by the dual-wield rules [CA 2.3].
    pub state_position: i32,
    pub status: u32,
    /// Essential NPCs cannot be killed (`CanBeKilled` false) [CA 4.3, I].
    pub immortal: bool,
    pub boss: bool,
    /// Summoned party member (party type 5) [CA 2.5, 3].
    pub summoned: bool,
    pub party_leader: Option<ActorId>,
    pub target: Option<ActorId>,
    pub current_attacker: Option<ActorId>,
    pub last_attacker: Option<ActorId>,
    pub attack_reason: AttackReason,
    pub defeated_by_player: bool,
    pub last_player_crime: Crime,
    pub last_fight_vs_player: FightVsPlayer,
    pub natural_damage: Option<DamageDef>,
    /// Override of the optimal-move chance (default: `LevelMax`) [CA 9.3]; used for bots.
    pub combat_skill_pct: Option<i32>,
    pub combat_state_active: bool,
    /// The `AttitudeToPlayer` property: the NPC's own default stance towards the player.
    pub base_attitude_to_player: Attitude,
    /// Routine name (`SetRoutine`) [CA 11.1].
    pub routine_name: String,
    /// Weapon drawn (weapon mode ready) [CA 10.2 `AssessPlayerWeapon`].
    pub drawn: bool,
    /// `Entity::SetLookAtTarget`: whom the head follows (the animation's look-at constraint).
    pub look_at: Option<ActorId>,
    /// Driven by the brain. NPCs yes; the hero only when used as a headless bot.
    pub ai_controlled: bool,
    pub brain: Brain,
}

impl Actor {
    pub fn npc(id: ActorId, name: &str, species: Species, level: i32, level_max: i32) -> Self {
        Self {
            id,
            name: name.into(),
            is_player: false,
            transformed: false,
            species,
            class: NpcClass::None,
            alignment: Alignment::None,
            npc_type: NpcType::Personal,
            enclave: None,
            level,
            level_max,
            base: PlayerAttrs {
                str: 0,
                dex: 0,
                int: 0,
                smithing: 0,
                theft: 0,
                alchemy: 0,
                hp_max: 0,
                mana_max: 0,
                stamina_max: 0,
            },
            stats: StatMods::default(),
            hp: 0,
            stamina: 0,
            mana: 0,
            equipment: Equipment::default(),
            look_at: None,
            inventory: Inventory::new(),
            perks: BTreeSet::new(),
            ai_mode: AiMode::Routine,
            action: Action::None,
            ani_state: AniState::Stand,
            state_position: 0,
            status: 0,
            immortal: false,
            boss: false,
            summoned: false,
            party_leader: None,
            target: None,
            current_attacker: None,
            last_attacker: None,
            attack_reason: AttackReason::None,
            defeated_by_player: false,
            last_player_crime: Crime::None,
            last_fight_vs_player: FightVsPlayer::None,
            natural_damage: None,
            combat_skill_pct: None,
            combat_state_active: true,
            base_attitude_to_player: Attitude::None,
            routine_name: "Start".into(),
            drawn: false,
            ai_controlled: true,
            brain: Brain::default(),
        }
    }

    /// The player character with new-game values [CA 2.13].
    pub fn hero(id: ActorId, cfg: &SimConfig) -> Self {
        let mut a = Self::npc(id, "PC_Hero", Species::Human, 1, 1);
        a.is_player = true;
        a.ai_controlled = false;
        a.base = PlayerAttrs::new_game(cfg);
        a.hp = a.base.hp_max;
        a.mana = a.base.mana_max;
        a.stamina = 100;
        a
    }

    /// Set HP/stamina/mana to their maxima (`RefreshHitPoints`, `Respawn`) [CA 2.13].
    pub fn refresh(&mut self, cfg: &SimConfig) {
        self.hp = self.hp_max(cfg);
        self.stamina = self.stamina_max(cfg);
        self.mana = self.mana_max();
    }

    pub fn has_perk(&self, perk: &str) -> bool {
        self.perks.contains(&perk.to_ascii_lowercase())
    }

    /// In parade mode (action Parade or AniState Parade) [CA 2.9].
    pub fn is_parading(&self) -> bool {
        self.action == Action::Parade || self.ani_state == AniState::Parade
    }

    pub fn is_alive(&self) -> bool {
        !matches!(self.ai_mode, AiMode::Dead)
    }
    pub fn is_down_or_dead(&self) -> bool {
        matches!(self.ai_mode, AiMode::Dead | AiMode::Down)
    }

    /// NPC formulas apply to every non-player and to a transformed player [CA 2.13].
    pub fn uses_npc_formulas(&self) -> bool {
        !self.is_player || self.transformed
    }

    /// `GetLevel` (S:36cb0): +10 on hard, -10 on easy, minimum 1; never for the player [CA 2.13].
    pub fn level(&self, cfg: &SimConfig) -> i32 {
        if self.is_player {
            self.level.max(1)
        } else {
            (self.level + cfg.difficulty.level_delta()).max(1)
        }
    }

    /// `GetLevelMax` (S:36db0) [CA 2.13].
    pub fn level_max(&self, cfg: &SimConfig) -> i32 {
        if self.is_player {
            self.level.max(1)
        } else {
            (self.level_max + cfg.difficulty.level_delta()).max(1)
        }
    }

    /// `CurrentLevel` (S:36b40): transformed player `(LevelMax+Level)/2`; NPC
    /// `min(GetLevel + Level, GetLevelMax)` (the second term is the raw `Level` property, as
    /// resolved by the `gothic-script` port).
    pub fn current_level(&self, cfg: &SimConfig) -> i32 {
        if self.is_player && self.transformed {
            ((self.level_max(cfg) + self.level(cfg)) / 2).max(1)
        } else if self.is_player {
            self.level(cfg)
        } else {
            let raw = if cfg.current_level_adds_raw {
                self.level.max(0)
            } else {
                0
            };
            (self.level(cfg) + raw).min(self.level_max(cfg))
        }
    }

    /// Strength: `10 + trunc(1.5 * CurrentLevel)` for NPCs [CA 2.13, S:33540].
    pub fn strength(&self, cfg: &SimConfig) -> i32 {
        if self.uses_npc_formulas() {
            10 + (3 * self.current_level(cfg)) / 2
        } else {
            self.base.str + self.equipment.modifier_sum("STR") + self.stats.modifier("STR")
        }
    }
    pub fn dexterity(&self, cfg: &SimConfig) -> i32 {
        if self.uses_npc_formulas() {
            10 + (3 * self.current_level(cfg)) / 2
        } else {
            self.base.dex + self.equipment.modifier_sum("DEX") + self.stats.modifier("DEX")
        }
    }
    pub fn intelligence(&self, cfg: &SimConfig) -> i32 {
        if self.uses_npc_formulas() {
            10 + (3 * self.current_level(cfg)) / 2
        } else {
            self.base.int + self.equipment.modifier_sum("INT") + self.stats.modifier("INT")
        }
    }
    /// Theft skill (NPC `10 + trunc(0.5*L)`, S:339f0) [CA 2.13].
    pub fn theft(&self, cfg: &SimConfig) -> i32 {
        if self.uses_npc_formulas() {
            10 + self.current_level(cfg) / 2
        } else {
            self.base.theft + self.equipment.modifier_sum("THF") + self.stats.modifier("THF")
        }
    }

    /// Named attribute lookup for requirement checks.
    pub fn attribute(&self, cfg: &SimConfig, tag: &str) -> i32 {
        match tag.to_ascii_uppercase().as_str() {
            "STR" => self.strength(cfg),
            "DEX" => self.dexterity(cfg),
            "INT" => self.intelligence(cfg),
            "THF" => self.theft(cfg),
            _ => 0,
        }
    }

    /// Max hit points: NPC `max(100, 20*LevelMax)`, 1 for ambient creatures [CA 2.13, S:35660].
    pub fn hp_max(&self, cfg: &SimConfig) -> i32 {
        if self.uses_npc_formulas() {
            if self.species.is_ambient() {
                1
            } else {
                (20 * self.level_max(cfg)).max(100)
            }
        } else {
            self.base.hp_max + self.stats.max_modifier("HP")
        }
    }

    /// Max stamina: NPC `max(100, 10*LevelMax)`, doubled for non-humanoids [CA 2.13, S:36440].
    pub fn stamina_max(&self, cfg: &SimConfig) -> i32 {
        if self.uses_npc_formulas() {
            let v = (10 * self.level_max(cfg)).max(100);
            if self.species.is_humanoid() { v } else { 2 * v }
        } else {
            self.base.stamina_max + self.stats.max_modifier("SP")
        }
    }

    /// NPC mana is the constant 1000 [CA 2.13, S:35d40].
    pub fn mana_max(&self) -> i32 {
        if self.uses_npc_formulas() {
            1000
        } else {
            self.base.mana_max + self.stats.max_modifier("MP")
        }
    }

    /// `GetProtection` [CA 2.7]. NPC/transformed: `min(80, trunc(1.5*LevelMax))`; player:
    /// equipment value (/5 and capped at 80 with ExtendedContent) plus the armor-perk rules.
    /// The `+10` Fire/Ice perk bonus is not modelled [CA 2.7, I].
    pub fn protection(&self, cfg: &SimConfig, ty: DamageType) -> i32 {
        if self.uses_npc_formulas() {
            return ((3 * self.level_max(cfg)) / 2).min(80);
        }
        let sum = self.equipment.protection_sum(ty) + self.stats.modifier(ty.protection_tag());
        let chest = self.equipment.body.as_ref().map_or(0, |b| b.protection(ty));
        let robe = self.equipment.wears_robe();
        let resist = match ty {
            DamageType::Fire if self.has_perk("perk_resistheat") => 10,
            DamageType::Ice if self.has_perk("perk_resistcold") => 10,
            _ => 0,
        };
        if cfg.extended_content {
            // Only Blades and Impact (Physics and None read Impact) are scaled down; the
            // other four channels are taken as they are (`gothic-script` GetProtection*).
            let scaled = matches!(
                ty,
                DamageType::Blade | DamageType::Impact | DamageType::Physics | DamageType::None
            );
            let mut p = if scaled { sum / 5 } else { sum }.min(80);
            if robe && self.has_perk("perk_lightarmor") {
                p += chest * 20 / 100;
            } else if !robe && self.has_perk("perk_heavyarmor") {
                p += chest * 5 / 100;
            }
            p + resist
        } else {
            let mut p = sum;
            if robe && self.has_perk("perk_lightarmor") {
                p *= 2;
            } else if !robe && self.has_perk("perk_heavyarmor") {
                p += p / 4;
            }
            p.min(90) + resist
        }
    }

    /// The attribute / pool `tag` as the player memory holds it: base values from the actor, the
    /// temporary modifiers from [`Actor::stats`].
    fn stat_of(&self, tag: &str) -> Stat {
        let mut s = match tag.to_ascii_uppercase().as_str() {
            "HP" => Stat::pool(self.hp, self.base.hp_max),
            "MP" => Stat::pool(self.mana, self.base.mana_max),
            "SP" => Stat::pool(self.stamina, self.base.stamina_max),
            "STR" => Stat::attribute(self.base.str),
            "DEX" => Stat::attribute(self.base.dex),
            "INT" => Stat::attribute(self.base.int),
            "THF" => Stat::attribute(self.base.theft),
            "ALC" => Stat::attribute(self.base.alchemy),
            "SMT" => Stat::attribute(self.base.smithing),
            _ => Stat::attribute(0),
        };
        self.stats.load(tag, &mut s);
        if s.pool {
            // the pool's current value is kept whole in the actor
            let value = match tag.to_ascii_uppercase().as_str() {
                "HP" => self.hp,
                "MP" => self.mana,
                _ => self.stamina,
            };
            s.base = value - s.modifier;
        }
        s
    }

    fn store_stat(&mut self, tag: &str, s: &Stat) {
        self.stats.store(tag, s);
        match tag.to_ascii_uppercase().as_str() {
            "HP" => {
                self.hp = s.value();
                self.base.hp_max = s.base_max;
            }
            "MP" => {
                self.mana = s.value();
                self.base.mana_max = s.base_max;
            }
            "SP" => {
                self.stamina = s.value();
                self.base.stamina_max = s.base_max;
            }
            "STR" => self.base.str = s.base,
            "DEX" => self.base.dex = s.base,
            "INT" => self.base.int = s.base,
            "THF" => self.base.theft = s.base,
            "ALC" => self.base.alchemy = s.base,
            "SMT" => self.base.smithing = s.base,
            _ => {}
        }
    }

    /// Port of the player memory entry points (`ApplyMod` Game:0x14efa0, `UnapplyMod` 0x14f0a0,
    /// `Consume` 0x14ed60): apply an item modifier event to the hero. Characters that use
    /// the NPC formulas have no attribute store and ignore it.
    pub fn apply_stat(&mut self, kind: StatKind, tag: &str, op: i32, value: i32) -> bool {
        gothic_portcov::hit!("apply_stat");
        if self.uses_npc_formulas() {
            return false;
        }
        let mut s = self.stat_of(tag);
        let ok = match kind {
            StatKind::Apply => stats::apply_mod(&mut s, tag, value, op),
            StatKind::Unapply => stats::unapply_mod(&mut s, tag, value, op),
            StatKind::Consume => stats::consume(&mut s, value, op),
        };
        self.store_stat(tag, &s);
        ok
    }

    /// `IsGuard`: enclave-type NPC with an alignment [CA 10.4, I on the enclave match].
    pub fn is_guard(&self) -> bool {
        self.npc_type == NpcType::Enclave && self.alignment != Alignment::None
    }

    /// `CanBeKilled` [CA 4.3, I].
    pub fn can_be_killed(&self) -> bool {
        !self.immortal
    }

    /// Optimal-move chance in percent: `LevelMax`, Zombie 85, +10/-10 on hard/easy,
    /// clamped to 0..=100 [CA 9.3].
    pub fn optimal_move_pct(&self, cfg: &SimConfig) -> i32 {
        if let Some(p) = self.combat_skill_pct {
            return p.clamp(0, 100);
        }
        let mut p = if self.species == Species::Zombie {
            85
        } else {
            self.level_max(cfg)
        };
        // `level_max` already includes the +-10 difficulty shift for non-zombies; the doc
        // lists the shift on the final percentage, so only add it for the forced Zombie value.
        if self.species == Species::Zombie {
            p += cfg.difficulty.level_delta();
        }
        p.clamp(0, 100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Difficulty;
    use crate::item::{ItemDef, Modifier};

    fn orc(level: i32, lmax: i32) -> Actor {
        Actor::npc(ActorId(1), "Orc", Species::Orc, level, lmax)
    }

    #[test]
    fn npc_attributes_from_level() {
        // [CA 2.13] Level 5 / LevelMax 30: CurrentLevel = min(GetLevel + Level, LevelMax) = 10
        // (the gothic-script reading of S:36b40), STR = 10 + trunc(15) = 25, HP = max(100, 600),
        // stamina = max(100, 300), protection = min(80, 45).
        let cfg = SimConfig::default();
        let o = orc(5, 30);
        assert_eq!(o.strength(&cfg), 25);
        assert_eq!(o.hp_max(&cfg), 600);
        assert_eq!(o.stamina_max(&cfg), 300);
        assert_eq!(o.mana_max(), 1000);
        assert_eq!(o.protection(&cfg, DamageType::Blade), 45);
        // Level 1: STR 11 (trunc 1.5), HP floor 100.
        let l1 = orc(1, 1);
        assert_eq!(l1.strength(&cfg), 11);
        assert_eq!(l1.hp_max(&cfg), 100);
        // Non-humanoids have double stamina; ambient creatures 1 HP.
        let w = Actor::npc(ActorId(2), "Wolf", Species::Wolf, 5, 5);
        assert_eq!(w.stamina_max(&cfg), 200);
        let g = Actor::npc(ActorId(3), "Gull", Species::Seagull, 1, 1);
        assert_eq!(g.hp_max(&cfg), 1);
        // Protection caps at 80 (LevelMax 60 -> 90 -> 80).
        assert_eq!(orc(60, 60).protection(&cfg, DamageType::Fire), 80);
    }

    #[test]
    fn difficulty_shifts_levels() {
        let hard = SimConfig {
            difficulty: Difficulty::Hard,
            ..SimConfig::default()
        };
        let easy = SimConfig {
            difficulty: Difficulty::Easy,
            ..SimConfig::default()
        };
        let o = orc(10, 30);
        assert_eq!(o.level(&hard), 20);
        assert_eq!(o.level_max(&easy), 20);
        assert_eq!(orc(5, 5).level(&easy), 1, "minimum level is 1");
        // The player is never adjusted.
        let mut h = Actor::hero(ActorId(0), &hard);
        h.level = 7;
        assert_eq!(h.level(&hard), 7);
    }

    #[test]
    fn player_protection_extended_vs_vanilla() {
        let ext = SimConfig::default();
        let van = SimConfig::vanilla();
        let mut h = Actor::hero(ActorId(0), &ext);
        h.equipment.equip(ItemDef {
            use_type: Some("Body".into()),
            modifiers: vec![Modifier {
                tag: "PROT_BLADE".into(),
                op: 0,
                value: 60,
            }],
            ..ItemDef::default()
        });
        // ExtendedContent: 60 / 5 = 12; vanilla keeps 60.
        assert_eq!(h.protection(&ext, DamageType::Blade), 12);
        assert_eq!(h.protection(&van, DamageType::Blade), 60);
        // HeavyArmor: vanilla +25 percent = 75; extended +5 percent of 60 = +3.
        h.perks.insert("perk_heavyarmor".into());
        assert_eq!(h.protection(&van, DamageType::Blade), 75);
        assert_eq!(h.protection(&ext, DamageType::Blade), 15);
        // Vanilla cap 90.
        h.equipment.body.as_mut().unwrap().modifiers[0].value = 200;
        assert_eq!(h.protection(&van, DamageType::Blade), 90);
    }

    #[test]
    fn hero_new_game_values() {
        let cfg = SimConfig::default();
        let h = Actor::hero(ActorId(0), &cfg);
        assert_eq!((h.hp, h.mana, h.stamina), (200, 100, 100));
        assert_eq!(h.stamina_max(&cfg), 150);
        assert_eq!(h.strength(&cfg), 100);
        assert_eq!(
            Actor::hero(ActorId(0), &SimConfig::vanilla()).stamina_max(&SimConfig::vanilla()),
            100
        );
    }

    #[test]
    fn optimal_move_chance() {
        let cfg = SimConfig::default();
        assert_eq!(orc(10, 30).optimal_move_pct(&cfg), 30);
        let mut z = Actor::npc(ActorId(1), "Z", Species::Zombie, 5, 5);
        assert_eq!(z.optimal_move_pct(&cfg), 85);
        z.level_max = 200;
        assert_eq!(orc(1, 200).optimal_move_pct(&cfg), 100);
    }
}
