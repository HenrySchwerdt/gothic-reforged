//! Trigger runtime: enter / leave volumes, filters and the chain that ends in script or quest
//! events (`eCTrigger_PS`, `eCEventFilter_PS`, `eCTriggerList_PS`, `gCEventScript_PS`).
//!
//! A trigger is a property set on an entity. The physics side reports contacts
//! (`OnTouch` / `OnUntouch` / `OnIntersect`), the damage system `OnDamage`, other triggers
//! `OnTrigger` / `OnUntrigger`. `ActivateTrigger` checks `CanBeActivatedNow` (enabled, activation
//! counts, event filters, retrigger wait, velocity), optionally delays the firing, then delivers
//! a trigger event to the named target entities (or to the source when no name is set).
//! `DeactivateTrigger` delivers the matching untrigger. A target that carries a trigger of its
//! own continues the chain; any other delivery, and every script trigger, is returned to the
//! host as a [`TriggerOutput`] (a script to run, a quest or sector change to apply, ...).
//!
//! Times are seconds on the caller's clock (`eCApplication` frame time). Persistence works on
//! names, not ids: see [`TriggerSnapshot`].
use crate::entity::EntityTree;
use crate::pvs::EntityId;
use crate::scene::SceneAdmin;
use bevy::math::Vec3;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// `eEEventType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventType {
    Trigger,
    Untrigger,
    Touch,
    Untouch,
    Intersect,
    Damage,
}

impl EventType {
    /// The event `OnProcess` sends when the auto-untrigger time ran out (`None` for events that
    /// have no counterpart).
    fn counterpart(self) -> Option<Self> {
        match self {
            Self::Trigger => Some(Self::Untrigger),
            Self::Untrigger => Some(Self::Trigger),
            Self::Touch => Some(Self::Untouch),
            Self::Untouch => Some(Self::Touch),
            Self::Intersect | Self::Damage => None,
        }
    }
}

/// Kind of contact that caused a touch (`eCContactIterator+0x38`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ContactKind {
    #[default]
    Unknown,
    NavZone,
    NavPath,
    PrefPath,
    Shape,
}

impl ContactKind {
    /// The `TouchType` string a fired trigger exposes (`SHAPE`, `NAVZONE`, ...).
    pub fn label(self) -> &'static str {
        match self {
            Self::Unknown => "UNKNOWN",
            Self::NavZone => "NAVZONE",
            Self::NavPath => "NAVPATH",
            Self::PrefPath => "PREFPATH",
            Self::Shape => "SHAPE",
        }
    }
}

/// What the physics reported for a touch (`eCContactIterator`, reduced to what triggers read).
#[derive(Debug, Clone, Default)]
pub struct Contact {
    pub kind: ContactKind,
    /// Shape groups of the touching shapes (`GetShapeGroup`).
    pub shape_groups: Vec<u16>,
    /// Property-set type of the touched shape's owner (0 = unknown).
    pub shape_ps_type: u16,
}

/// Entity class a trigger's recognition switches look at (`eCDynamicEntity` vfunc +0x15c).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EntityClass {
    #[default]
    Player,
    Npc,
    Object,
}

/// What a trigger does besides delivering events.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub enum TriggerKind {
    /// Plain `eCTrigger_PS`.
    #[default]
    Basic,
    /// `eCEventFilter_PS`: turns a trigger into another event on the named entities.
    Filter { out: EventType, damage: i32 },
    /// `eCTriggerList_PS`: up to ten targets with delays, fired in a mode.
    List(Box<TriggerList>),
    /// `gCEventScript_PS`: the activation runs a script (host side).
    Script { script: String },
}

/// `eETriggerListProcessType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ListMode {
    /// All targets in order, honouring their delays.
    #[default]
    All,
    /// The next target on every activation.
    Sequence,
    /// A random target different from the last.
    Random,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct TriggerList {
    pub mode: ListMode,
    pub targets: [String; 10],
    pub delays: [f32; 10],
    /// Index of the next target (`+0x13c`).
    pub cursor: usize,
}

/// The properties of one trigger.
#[derive(Debug, Clone, PartialEq)]
pub struct TriggerDef {
    pub kind: TriggerKind,
    /// Entities that receive the event; empty = the source entity.
    pub target: String,
    /// `Trigger(name)` fires this trigger when the name matches.
    pub trigger_name: String,
    pub enabled: bool,
    /// Remaining activations; negative = unlimited, 0 = used up.
    pub max_activations: i32,
    /// Activations per source entity; 0 or negative = unlimited.
    pub max_per_entity: i32,
    pub fire_delay: f32,
    pub retrigger_wait: f32,
    /// Send the matching untrigger this many seconds after firing (0 = never).
    pub auto_untrigger: f32,
    pub damage_threshold: i32,
    /// Minimum speed of the trigger's own entity (cm/s) for activation.
    pub velocity_threshold: f32,
    pub send_untrigger: bool,
    pub react_trigger: bool,
    pub react_touch: bool,
    pub react_damage: bool,
    pub react_intersect: bool,
    pub react_processing_range: bool,
    pub react_cache_in: bool,
    pub recognize_players: bool,
    pub recognize_npcs: bool,
    pub recognize_objects: bool,
    /// Entity name that is recognised regardless of class.
    pub recognize_name: String,
    /// Required property-set type of a touched shape's owner (0 = any).
    pub recognize_ps_type: u16,
    /// Required shape group (0 = any).
    pub recognize_shape_group: u16,
    /// Required contact kind (`Unknown` = any).
    pub recognize_touch: ContactKind,
}

impl TriggerDef {
    /// A usable trigger: enabled, unlimited, reacts to touch and trigger events, recognises the
    /// player. The original's property defaults are not verified; data overrides all of them.
    pub fn new(target: &str) -> Self {
        Self {
            kind: TriggerKind::Basic,
            target: target.to_owned(),
            trigger_name: String::new(),
            enabled: true,
            max_activations: -1,
            max_per_entity: -1,
            fire_delay: 0.0,
            retrigger_wait: 0.0,
            auto_untrigger: 0.0,
            damage_threshold: 0,
            velocity_threshold: 0.0,
            send_untrigger: true,
            react_trigger: true,
            react_touch: true,
            react_damage: false,
            react_intersect: false,
            react_processing_range: false,
            react_cache_in: false,
            recognize_players: true,
            recognize_npcs: false,
            recognize_objects: false,
            recognize_name: String::new(),
            recognize_ps_type: 0,
            recognize_shape_group: 0,
            recognize_touch: ContactKind::Unknown,
        }
    }
}

/// Something the host has to act on.
#[derive(Debug, Clone, PartialEq)]
pub enum TriggerOutput {
    /// A trigger delivered `event` to `target`, which has no trigger of its own (a door, a
    /// mover, a light, a spawner ...). `owner` is the trigger's entity, `inflictor` the source.
    Delivered {
        event: EventType,
        target: EntityId,
        owner: EntityId,
        inflictor: Option<EntityId>,
    },
    /// A script trigger fired.
    Script {
        script: String,
        event: EventType,
        owner: EntityId,
        inflictor: Option<EntityId>,
        /// How it was touched (`TouchType`).
        touch: ContactKind,
    },
}

#[derive(Debug, Clone)]
struct Pending {
    due: f64,
    source: Option<EntityId>,
    event: EventType,
    contact: ContactKind,
}

#[derive(Debug, Clone)]
struct TriggerPs {
    key: String,
    def: TriggerDef,
    /// Delayed firing waiting for its time (`+0x18`, `+0xc4`).
    pending: Option<Pending>,
    /// Earliest time of the next activation (`+0xbc`).
    retrigger_due: f64,
    /// When to send the counterpart event (`+0xc0`).
    untrigger_due: f64,
    running: Option<(EventType, Option<EntityId>)>,
    /// Source entity name -> activations (`+0x58` / `+0x64`).
    counts: BTreeMap<String, u16>,
    /// Timers of a trigger list: (due, target index, source).
    list_timer: Option<(f64, Option<EntityId>)>,
}

/// Maximum depth of trigger-to-trigger delivery (guards against authored loops).
const MAX_CHAIN: u32 = 16;

/// Read-only world data the triggers consult.
pub struct TriggerEnv<'a> {
    pub tree: &'a EntityTree,
    pub scene: &'a SceneAdmin,
    /// Class of an entity for the recognition switches.
    pub class_of: &'a dyn Fn(EntityId) -> EntityClass,
    /// Linear velocity (cm/s) of an entity.
    pub velocity_of: &'a dyn Fn(EntityId) -> Vec3,
}

fn player_class(_: EntityId) -> EntityClass {
    EntityClass::Player
}
fn no_velocity(_: EntityId) -> Vec3 {
    Vec3::ZERO
}

impl<'a> TriggerEnv<'a> {
    /// Environment with the original's fallbacks: every entity is class `Player`, nothing moves.
    pub fn new(tree: &'a EntityTree, scene: &'a SceneAdmin) -> Self {
        Self {
            tree,
            scene,
            class_of: &player_class,
            velocity_of: &no_velocity,
        }
    }
}

/// All triggers of the world.
#[derive(Debug, Default)]
pub struct TriggerRuntime {
    triggers: BTreeMap<EntityId, TriggerPs>,
    out: Vec<TriggerOutput>,
    rng: u32,
}

impl TriggerRuntime {
    pub fn new() -> Self {
        Self {
            rng: 0x2545_F491,
            ..Self::default()
        }
    }

    /// Attach a trigger to an entity. `key` is the stable name used by saves (entity GUID or
    /// document + name).
    pub fn add(&mut self, owner: EntityId, key: &str, def: TriggerDef) {
        self.triggers.insert(
            owner,
            TriggerPs {
                key: key.to_owned(),
                def,
                pending: None,
                retrigger_due: f64::NEG_INFINITY,
                untrigger_due: f64::INFINITY,
                running: None,
                counts: BTreeMap::new(),
                list_timer: None,
            },
        );
    }

    /// The property set left with its entity (`OnPropertySetRemoved`).
    pub fn remove(&mut self, owner: EntityId) -> bool {
        self.triggers.remove(&owner).is_some()
    }

    pub fn len(&self) -> usize {
        self.triggers.len()
    }
    pub fn is_empty(&self) -> bool {
        self.triggers.is_empty()
    }
    pub fn def(&self, owner: EntityId) -> Option<&TriggerDef> {
        self.triggers.get(&owner).map(|t| &t.def)
    }
    pub fn def_mut(&mut self, owner: EntityId) -> Option<&mut TriggerDef> {
        self.triggers.get_mut(&owner).map(|t| &mut t.def)
    }

    /// Take the outputs gathered since the last call.
    pub fn take_output(&mut self) -> Vec<TriggerOutput> {
        std::mem::take(&mut self.out)
    }

    /// Port of `eCTrigger_PS::OnTouch` (Engine:0x1007da90).
    pub fn on_touch(
        &mut self,
        env: &TriggerEnv,
        now: f64,
        owner: EntityId,
        source: EntityId,
        contact: &Contact,
    ) {
        if self.triggers.get(&owner).is_some_and(|t| t.def.react_touch) {
            self.activate(env, now, owner, Some(source), EventType::Touch, contact, 0);
        }
    }

    /// Port of `eCTrigger_PS::OnUntouch` (Engine:0x1007dab0).
    pub fn on_untouch(&mut self, env: &TriggerEnv, now: f64, owner: EntityId, source: EntityId) {
        if self.triggers.get(&owner).is_some_and(|t| t.def.react_touch) {
            self.deactivate(env, now, owner, Some(source), EventType::Untouch, 0);
        }
    }

    /// Port of `eCTrigger_PS::OnIntersect` (Engine:0x1007da70).
    pub fn on_intersect(
        &mut self,
        env: &TriggerEnv,
        now: f64,
        owner: EntityId,
        source: EntityId,
        contact: &Contact,
    ) {
        if self
            .triggers
            .get(&owner)
            .is_some_and(|t| t.def.react_intersect)
        {
            self.activate(
                env,
                now,
                owner,
                Some(source),
                EventType::Intersect,
                contact,
                0,
            );
        }
    }

    /// Port of `eCTrigger_PS::OnTrigger` (Engine:0x1007da00): another trigger fired at this
    /// entity.
    pub fn on_trigger(
        &mut self,
        env: &TriggerEnv,
        now: f64,
        owner: EntityId,
        source: Option<EntityId>,
    ) {
        self.deliver(env, now, owner, source, EventType::Trigger, 0);
    }

    /// Port of `eCTrigger_PS::OnUntrigger` (Engine:0x1007da50).
    pub fn on_untrigger(
        &mut self,
        env: &TriggerEnv,
        now: f64,
        owner: EntityId,
        source: Option<EntityId>,
    ) {
        self.deliver(env, now, owner, source, EventType::Untrigger, 0);
    }

    /// Port of `eCTrigger_PS::OnDamage` (Engine:0x1007dad0): damage above the threshold fires.
    pub fn on_damage(
        &mut self,
        env: &TriggerEnv,
        now: f64,
        owner: EntityId,
        attacker: EntityId,
        damage: i32,
    ) {
        let Some(t) = self.triggers.get(&owner) else {
            return;
        };
        if t.def.react_damage && damage > 0 && t.def.damage_threshold < damage {
            self.activate(
                env,
                now,
                owner,
                Some(attacker),
                EventType::Damage,
                &Contact::default(),
                0,
            );
        }
    }

    /// Port of `eCTrigger_PS::Trigger(name)` (Engine:0x1007da20): fire the triggers whose
    /// `trigger_name` equals `name`.
    pub fn trigger_by_name(&mut self, env: &TriggerEnv, now: f64, name: &str) {
        let owners: Vec<EntityId> = self
            .triggers
            .iter()
            .filter(|(_, t)| t.def.trigger_name == name)
            .map(|(o, _)| *o)
            .collect();
        for o in owners {
            self.deliver(env, now, o, None, EventType::Trigger, 0);
        }
    }

    /// Port of `eCTrigger_PS::OnEnterProcessingRange` (Engine:0x1007e120): triggers that react
    /// to the processing range fire for their own entity.
    pub fn on_enter_processing_range(&mut self, env: &TriggerEnv, now: f64, owner: EntityId) {
        if self
            .triggers
            .get(&owner)
            .is_some_and(|t| t.def.react_processing_range)
        {
            self.activate(
                env,
                now,
                owner,
                Some(owner),
                EventType::Trigger,
                &Contact::default(),
                0,
            );
        }
    }

    /// Port of `eCTrigger_PS::OnCacheIn` (Engine:0x1007e160).
    pub fn on_cache_in(&mut self, env: &TriggerEnv, now: f64, owner: EntityId) {
        if self
            .triggers
            .get(&owner)
            .is_some_and(|t| t.def.react_cache_in)
        {
            self.activate(
                env,
                now,
                owner,
                Some(owner),
                EventType::Trigger,
                &Contact::default(),
                0,
            );
        }
    }

    /// Dispatch of the event handlers of the trigger flavours: a plain trigger reacts to
    /// `OnTrigger`, a filter turns the incoming event into its output event.
    fn deliver(
        &mut self,
        env: &TriggerEnv,
        now: f64,
        owner: EntityId,
        source: Option<EntityId>,
        ev: EventType,
        depth: u32,
    ) {
        let Some(t) = self.triggers.get(&owner) else {
            return;
        };
        if let TriggerKind::Filter { out, damage } = t.def.kind.clone() {
            self.filter(env, now, owner, source, ev, out, damage, depth);
            return;
        }
        if !t.def.react_trigger {
            return;
        }
        match ev {
            EventType::Untrigger => {
                self.deactivate(env, now, owner, source, EventType::Untrigger, depth)
            }
            _ => self.activate(env, now, owner, source, ev, &Contact::default(), depth),
        }
    }

    /// Port of `eCEventFilter_PS::OnTrigger` (Engine:0x10077740) and its siblings: the named
    /// entities get the configured event instead.
    #[allow(clippy::too_many_arguments)]
    fn filter(
        &mut self,
        env: &TriggerEnv,
        now: f64,
        owner: EntityId,
        source: Option<EntityId>,
        _incoming: EventType,
        out: EventType,
        damage: i32,
        depth: u32,
    ) {
        let target = self
            .triggers
            .get(&owner)
            .map(|t| t.def.target.clone())
            .unwrap_or_default();
        for e in env.scene.entities_by_name(&target).to_vec() {
            match out {
                EventType::Trigger => {
                    self.send(env, now, owner, e, source, EventType::Trigger, depth)
                }
                EventType::Untrigger => {
                    self.send(env, now, owner, e, source, EventType::Untrigger, depth)
                }
                EventType::Touch | EventType::Untouch | EventType::Intersect => {
                    self.send(env, now, owner, e, source, out, depth);
                }
                EventType::Damage => {
                    if let Some(s) = source {
                        if self.triggers.contains_key(&e) {
                            self.on_damage(env, now, e, s, damage);
                        } else {
                            self.out.push(TriggerOutput::Delivered {
                                event: out,
                                target: e,
                                owner,
                                inflictor: source,
                            });
                        }
                    }
                }
            }
        }
    }

    /// Port of `eCTrigger_PS::CanBeActivatedNow` (Engine:0x1007ce70).
    fn can_be_activated(
        &self,
        env: &TriggerEnv,
        now: f64,
        owner: EntityId,
        source: Option<EntityId>,
        contact: &Contact,
    ) -> bool {
        let Some(t) = self.triggers.get(&owner) else {
            return false;
        };
        let d = &t.def;
        if !d.enabled || d.max_activations == 0 {
            return false;
        }
        if d.recognize_touch != ContactKind::Unknown && d.recognize_touch != contact.kind {
            return false;
        }
        if t.pending.is_some() {
            return false;
        }
        if d.velocity_threshold != 0.0 {
            let v = (env.velocity_of)(owner);
            if v.length_squared() < d.velocity_threshold * d.velocity_threshold {
                return false;
            }
        }
        if now < t.retrigger_due {
            return false;
        }
        if d.max_per_entity > 0
            && let Some(name) = source
                .and_then(|s| env.tree.get(s))
                .map(|e| e.name.as_str())
            && t.counts
                .get(name)
                .is_some_and(|c| i32::from(*c) >= d.max_per_entity)
        {
            return false;
        }
        if contact.kind == ContactKind::Shape {
            if d.recognize_ps_type != 0
                && contact.shape_ps_type != 0
                && contact.shape_ps_type != d.recognize_ps_type
            {
                return false;
            }
            if d.recognize_shape_group != 0
                && !contact.shape_groups.contains(&d.recognize_shape_group)
            {
                return false;
            }
        }
        if let Some(s) = source {
            let class = if env.tree.get(s).is_some_and(|e| e.is_dynamic()) {
                (env.class_of)(s)
            } else {
                EntityClass::Player
            };
            let by_class = (d.recognize_players && class == EntityClass::Player)
                || (d.recognize_npcs && class == EntityClass::Npc)
                || (d.recognize_objects && class == EntityClass::Object);
            let by_name = !d.recognize_name.is_empty()
                && env.tree.get(s).is_some_and(|e| e.name == d.recognize_name);
            if !by_class && !by_name {
                return false;
            }
        }
        true
    }

    /// Port of `eCTrigger_PS::ActivateTrigger` (Engine:0x1007d0f0).
    #[allow(clippy::too_many_arguments)]
    fn activate(
        &mut self,
        env: &TriggerEnv,
        now: f64,
        owner: EntityId,
        source: Option<EntityId>,
        ev: EventType,
        contact: &Contact,
        depth: u32,
    ) {
        if depth > MAX_CHAIN || !self.can_be_activated(env, now, owner, source, contact) {
            return;
        }
        let delay = self.triggers[&owner].def.fire_delay;
        if delay > 0.0 {
            if let Some(t) = self.triggers.get_mut(&owner) {
                t.pending = Some(Pending {
                    due: now + f64::from(delay),
                    source,
                    event: ev,
                    contact: contact.kind,
                });
            }
            return;
        }
        self.fire(env, now, owner, source, ev, contact.kind, depth);
    }

    /// The body of `ActivateTrigger` after the optional delay.
    #[allow(clippy::too_many_arguments)]
    fn fire(
        &mut self,
        env: &TriggerEnv,
        now: f64,
        owner: EntityId,
        source: Option<EntityId>,
        ev: EventType,
        touch: ContactKind,
        depth: u32,
    ) {
        let (kind, target) = {
            let t = &self.triggers[&owner];
            (t.def.kind.clone(), t.def.target.clone())
        };
        match kind {
            TriggerKind::List(_) => self.list_activate(env, now, owner, source, depth),
            TriggerKind::Script { script } => {
                self.out.push(TriggerOutput::Script {
                    script,
                    event: ev,
                    owner,
                    inflictor: source,
                    touch,
                });
                self.send_to_targets(env, now, owner, &target, source, EventType::Trigger, depth);
            }
            _ => self.send_to_targets(env, now, owner, &target, source, EventType::Trigger, depth),
        }
        // Book-keeping after the delivery.
        let name = source.and_then(|s| env.tree.get(s)).map(|e| e.name.clone());
        let Some(t) = self.triggers.get_mut(&owner) else {
            return;
        };
        t.running = Some((ev, source));
        if t.def.max_activations > 0 {
            t.def.max_activations -= 1;
        }
        if t.def.max_per_entity > 0
            && let Some(n) = name
        {
            *t.counts.entry(n).or_insert(0) += 1;
        }
        t.retrigger_due = now + f64::from(t.def.retrigger_wait);
        if t.def.auto_untrigger > 0.0 {
            t.untrigger_due = now + f64::from(t.def.auto_untrigger);
        }
    }

    /// Port of `eCTrigger_PS::DeactivateTrigger` (Engine:0x1007d6e0): send the untrigger.
    fn deactivate(
        &mut self,
        env: &TriggerEnv,
        now: f64,
        owner: EntityId,
        source: Option<EntityId>,
        ev: EventType,
        depth: u32,
    ) {
        let Some(t) = self.triggers.get(&owner) else {
            return;
        };
        if depth > MAX_CHAIN || t.pending.is_some() || !t.def.enabled || !t.def.send_untrigger {
            return;
        }
        let target = t.def.target.clone();
        self.send_to_targets(
            env,
            now,
            owner,
            &target,
            source,
            EventType::Untrigger,
            depth,
        );
        if let Some(t) = self.triggers.get_mut(&owner) {
            t.running = Some((ev, source));
            t.untrigger_due = f64::INFINITY;
        }
    }

    /// Deliver to the entities named `target`, or to the source when the name is empty.
    #[allow(clippy::too_many_arguments)]
    fn send_to_targets(
        &mut self,
        env: &TriggerEnv,
        now: f64,
        owner: EntityId,
        target: &str,
        source: Option<EntityId>,
        ev: EventType,
        depth: u32,
    ) {
        if target.is_empty() {
            if let Some(s) = source {
                self.send(env, now, owner, s, source, ev, depth);
            }
            return;
        }
        for e in env.scene.entities_by_name(target).to_vec() {
            self.send(env, now, owner, e, source, ev, depth);
        }
    }

    /// Deliver one event to one entity: a trigger of its own continues the chain, otherwise the
    /// host gets it.
    #[allow(clippy::too_many_arguments)]
    fn send(
        &mut self,
        env: &TriggerEnv,
        now: f64,
        owner: EntityId,
        target: EntityId,
        inflictor: Option<EntityId>,
        ev: EventType,
        depth: u32,
    ) {
        if target != owner && self.triggers.contains_key(&target) {
            match ev {
                EventType::Untrigger => {
                    self.deliver(env, now, target, inflictor, EventType::Untrigger, depth + 1)
                }
                EventType::Trigger => {
                    self.deliver(env, now, target, inflictor, EventType::Trigger, depth + 1)
                }
                other => {
                    let c = Contact::default();
                    let t = &self.triggers[&target];
                    let reacts = match other {
                        EventType::Touch | EventType::Untouch => t.def.react_touch,
                        EventType::Intersect => t.def.react_intersect,
                        _ => false,
                    };
                    if reacts {
                        if other == EventType::Untouch {
                            self.deactivate(env, now, target, inflictor, other, depth + 1);
                        } else {
                            self.activate(env, now, target, inflictor, other, &c, depth + 1);
                        }
                    }
                }
            }
        } else {
            self.out.push(TriggerOutput::Delivered {
                event: ev,
                target,
                owner,
                inflictor,
            });
        }
    }

    // ---- trigger lists -------------------------------------------------------------------

    /// Port of `eCTriggerList_PS::TriggerTarget` / `DoTriggering` (Engine:0x10084220 /
    /// 0x10084040).
    fn list_activate(
        &mut self,
        env: &TriggerEnv,
        now: f64,
        owner: EntityId,
        source: Option<EntityId>,
        depth: u32,
    ) {
        let Some(TriggerKind::List(l)) = self.triggers.get(&owner).map(|t| t.def.kind.clone())
        else {
            return;
        };
        let mut cursor = l.cursor.min(9);
        match l.mode {
            ListMode::All => {
                while cursor < 10 {
                    if !l.targets[cursor].is_empty() {
                        let delay = l.delays[cursor];
                        if delay > 0.0 {
                            // The rest continues from the timer (`OnTimer`).
                            self.set_cursor(owner, cursor);
                            if let Some(t) = self.triggers.get_mut(&owner) {
                                t.list_timer = Some((now + f64::from(delay), source));
                            }
                            return;
                        }
                        self.send_to_targets(
                            env,
                            now,
                            owner,
                            &l.targets[cursor],
                            source,
                            EventType::Trigger,
                            depth,
                        );
                    }
                    cursor += 1;
                }
                self.set_cursor(owner, 0);
            }
            ListMode::Sequence => {
                // First non-empty entry at or after the cursor, wrapping once.
                for step in 0..10 {
                    let i = (cursor + step) % 10;
                    if !l.targets[i].is_empty() {
                        self.send_to_targets(
                            env,
                            now,
                            owner,
                            &l.targets[i],
                            source,
                            EventType::Trigger,
                            depth,
                        );
                        self.set_cursor(owner, (i + 1) % 10);
                        break;
                    }
                }
            }
            ListMode::Random => {
                let filled: Vec<usize> = (0..10).filter(|i| !l.targets[*i].is_empty()).collect();
                if !filled.is_empty() {
                    let others: Vec<usize> =
                        filled.iter().copied().filter(|i| *i != cursor).collect();
                    let pool = if others.is_empty() { &filled } else { &others };
                    let pick = pool[self.next_rand() as usize % pool.len()];
                    self.send_to_targets(
                        env,
                        now,
                        owner,
                        &l.targets[pick],
                        source,
                        EventType::Trigger,
                        depth,
                    );
                    self.set_cursor(owner, pick);
                }
            }
        }
    }

    fn set_cursor(&mut self, owner: EntityId, c: usize) {
        if let Some(TriggerKind::List(l)) = self.triggers.get_mut(&owner).map(|t| &mut t.def.kind) {
            l.cursor = c;
        }
    }

    fn next_rand(&mut self) -> u32 {
        // xorshift32: deterministic and good enough for picking a list entry.
        let mut x = self.rng.max(1);
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        x
    }

    // ---- per-frame ----------------------------------------------------------------------

    /// Port of `eCTrigger_PS::OnProcess` (Engine:0x1007db00) and `eCTriggerList_PS::OnTimer`:
    /// delayed firings, auto-untriggers and list timers whose time has come.
    pub fn process(&mut self, env: &TriggerEnv, now: f64) {
        let owners: Vec<EntityId> = self.triggers.keys().copied().collect();
        for o in owners {
            let Some(t) = self.triggers.get(&o) else {
                continue;
            };
            let auto = t.running.and_then(|(ev, src)| {
                (t.def.auto_untrigger > 0.0 && t.untrigger_due < now)
                    .then(|| ev.counterpart().map(|c| (c, src)))
                    .flatten()
            });
            let due = t.pending.as_ref().is_some_and(|p| p.due <= now);
            let timer = t.list_timer.filter(|(d, _)| *d <= now);
            if let Some((ev, src)) = auto {
                if let Some(t) = self.triggers.get_mut(&o) {
                    t.untrigger_due = f64::INFINITY;
                }
                self.deactivate(env, now, o, src, ev, 0);
            }
            if due && let Some(p) = self.triggers.get_mut(&o).and_then(|t| t.pending.take()) {
                self.fire(env, now, o, p.source, p.event, p.contact, 0);
            }
            if let Some((_, src)) = timer {
                if let Some(t) = self.triggers.get_mut(&o) {
                    t.list_timer = None;
                }
                // The delayed entry fires, the rest of the list continues.
                if let Some(TriggerKind::List(l)) =
                    self.triggers.get(&o).map(|t| t.def.kind.clone())
                {
                    let c = l.cursor.min(9);
                    self.send_to_targets(env, now, o, &l.targets[c], src, EventType::Trigger, 0);
                    if l.mode == ListMode::All {
                        self.set_cursor(o, c + 1);
                        self.list_activate(env, now, o, src, 0);
                    }
                }
            }
        }
    }

    // ---- persistence --------------------------------------------------------------------

    /// Save image of all triggers: what a loaded game needs to continue (enabled flag, counters
    /// and list cursors). Pending delays and running timers are not saved; a delayed firing
    /// that was in flight is dropped.
    pub fn snapshot(&self, now: f64) -> TriggerSnapshot {
        TriggerSnapshot {
            triggers: self
                .triggers
                .values()
                .map(|t| TriggerSave {
                    key: t.key.clone(),
                    enabled: t.def.enabled,
                    max_activations: t.def.max_activations,
                    counts: t.counts.clone(),
                    retrigger_left: (t.retrigger_due - now).max(0.0) as f32,
                    cursor: match &t.def.kind {
                        TriggerKind::List(l) => l.cursor,
                        _ => 0,
                    },
                })
                .collect(),
        }
    }

    /// Re-apply a save image to the triggers that exist now; keys without a trigger are
    /// returned.
    pub fn restore(&mut self, snap: &TriggerSnapshot, now: f64) -> Vec<String> {
        let mut unknown = Vec::new();
        for s in &snap.triggers {
            let Some(t) = self.triggers.values_mut().find(|t| t.key == s.key) else {
                unknown.push(s.key.clone());
                continue;
            };
            t.def.enabled = s.enabled;
            t.def.max_activations = s.max_activations;
            t.counts = s.counts.clone();
            t.retrigger_due = now + f64::from(s.retrigger_left);
            t.pending = None;
            t.list_timer = None;
            t.running = None;
            t.untrigger_due = f64::INFINITY;
            if let TriggerKind::List(l) = &mut t.def.kind {
                l.cursor = s.cursor.min(9);
            }
        }
        unknown
    }
}

/// Persisted state of one trigger.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TriggerSave {
    pub key: String,
    pub enabled: bool,
    pub max_activations: i32,
    /// Source entity name -> activations.
    pub counts: BTreeMap<String, u16>,
    pub retrigger_left: f32,
    pub cursor: usize,
}

/// Persisted state of all triggers.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TriggerSnapshot {
    pub triggers: Vec<TriggerSave>,
}

#[cfg(test)]
mod tests {
    use super::*;

    struct W {
        tree: EntityTree,
        scene: SceneAdmin,
    }

    impl W {
        fn new() -> Self {
            Self {
                tree: EntityTree::new(),
                scene: SceneAdmin::new(),
            }
        }
        fn entity(&mut self, name: &str, dynamic: bool) -> EntityId {
            let id = self.tree.create(name, dynamic);
            self.scene.register(id, name);
            id
        }
        fn env(&self) -> TriggerEnv<'_> {
            TriggerEnv::new(&self.tree, &self.scene)
        }
    }

    fn delivered(rt: &mut TriggerRuntime) -> Vec<(EventType, EntityId)> {
        rt.take_output()
            .into_iter()
            .filter_map(|o| match o {
                TriggerOutput::Delivered { event, target, .. } => Some((event, target)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn trigger_fires_on_enter_and_unfires_on_leave() {
        let mut w = W::new();
        let volume = w.entity("Gate_Volume", false);
        let door = w.entity("Gate_Door", true);
        let hero = w.entity("PC_Hero", true);
        let mut rt = TriggerRuntime::new();
        rt.add(volume, "gate", TriggerDef::new("Gate_Door"));
        rt.on_touch(&w.env(), 1.0, volume, hero, &Contact::default());
        assert_eq!(delivered(&mut rt), vec![(EventType::Trigger, door)]);
        rt.on_untouch(&w.env(), 2.0, volume, hero);
        assert_eq!(delivered(&mut rt), vec![(EventType::Untrigger, door)]);
    }

    #[test]
    fn filters_counts_and_retrigger_wait_gate_the_activation() {
        let mut w = W::new();
        let volume = w.entity("V", false);
        let door = w.entity("D", true);
        let hero = w.entity("PC_Hero", true);
        let npc = w.entity("Hamlar", true);
        let mut rt = TriggerRuntime::new();
        let mut d = TriggerDef::new("D");
        d.max_activations = 2;
        d.retrigger_wait = 5.0;
        rt.add(volume, "v", d);
        let class = move |e: EntityId| {
            if e == npc {
                EntityClass::Npc
            } else {
                EntityClass::Player
            }
        };
        let env = TriggerEnv {
            class_of: &class,
            ..TriggerEnv::new(&w.tree, &w.scene)
        };
        // An NPC is not recognised.
        rt.on_touch(&env, 0.0, volume, npc, &Contact::default());
        assert!(delivered(&mut rt).is_empty());
        rt.on_touch(&env, 0.0, volume, hero, &Contact::default());
        assert_eq!(delivered(&mut rt).len(), 1);
        // Inside the retrigger wait.
        rt.on_touch(&env, 3.0, volume, hero, &Contact::default());
        assert!(delivered(&mut rt).is_empty());
        rt.on_touch(&env, 6.0, volume, hero, &Contact::default());
        assert_eq!(delivered(&mut rt), vec![(EventType::Trigger, door)]);
        // The activation budget (2) is used up.
        rt.on_touch(&env, 20.0, volume, hero, &Contact::default());
        assert!(delivered(&mut rt).is_empty());
    }

    #[test]
    fn fire_delay_and_auto_untrigger_run_from_process() {
        let mut w = W::new();
        let volume = w.entity("V", false);
        let door = w.entity("D", true);
        let hero = w.entity("PC_Hero", true);
        let mut rt = TriggerRuntime::new();
        let mut d = TriggerDef::new("D");
        d.fire_delay = 2.0;
        d.auto_untrigger = 3.0;
        rt.add(volume, "v", d);
        rt.on_touch(&w.env(), 0.0, volume, hero, &Contact::default());
        assert!(delivered(&mut rt).is_empty());
        rt.process(&w.env(), 1.0);
        assert!(delivered(&mut rt).is_empty());
        rt.process(&w.env(), 2.0);
        assert_eq!(delivered(&mut rt), vec![(EventType::Trigger, door)]);
        rt.process(&w.env(), 4.0);
        assert!(delivered(&mut rt).is_empty());
        rt.process(&w.env(), 5.5);
        assert_eq!(delivered(&mut rt), vec![(EventType::Untrigger, door)]);
    }

    #[test]
    fn script_trigger_reports_the_script_and_touch_type() {
        let mut w = W::new();
        let volume = w.entity("Loc_Trigger", false);
        let hero = w.entity("PC_Hero", true);
        let mut rt = TriggerRuntime::new();
        let mut d = TriggerDef::new("");
        d.kind = TriggerKind::Script {
            script: "OnEnterArdea".into(),
        };
        rt.add(volume, "loc", d);
        let c = Contact {
            kind: ContactKind::Shape,
            ..Contact::default()
        };
        rt.on_touch(&w.env(), 0.0, volume, hero, &c);
        let out = rt.take_output();
        assert!(out.iter().any(|o| matches!(o,
            TriggerOutput::Script { script, touch: ContactKind::Shape, inflictor: Some(i), .. }
            if script == "OnEnterArdea" && *i == hero)));
    }

    #[test]
    fn filter_and_list_chain_through_triggers() {
        let mut w = W::new();
        let volume = w.entity("V", false);
        let list = w.entity("L", false);
        let a = w.entity("A", true);
        let b = w.entity("B", true);
        let hero = w.entity("PC_Hero", true);
        let mut rt = TriggerRuntime::new();
        rt.add(volume, "v", TriggerDef::new("L"));
        let mut ld = TriggerDef::new("");
        let mut l = TriggerList {
            mode: ListMode::Sequence,
            ..TriggerList::default()
        };
        l.targets[0] = "A".into();
        l.targets[1] = "B".into();
        ld.kind = TriggerKind::List(Box::new(l));
        rt.add(list, "l", ld);
        for t in [1.0, 2.0, 3.0] {
            rt.on_touch(&w.env(), t, volume, hero, &Contact::default());
        }
        let got: Vec<EntityId> = delivered(&mut rt).into_iter().map(|d| d.1).collect();
        assert_eq!(got, vec![a, b, a]);
    }

    #[test]
    fn snapshot_restores_counters_by_key() {
        let mut w = W::new();
        let volume = w.entity("V", false);
        let _door = w.entity("D", true);
        let hero = w.entity("PC_Hero", true);
        let mut rt = TriggerRuntime::new();
        let mut d = TriggerDef::new("D");
        d.max_activations = 1;
        rt.add(volume, "v", d.clone());
        rt.on_touch(&w.env(), 0.0, volume, hero, &Contact::default());
        let snap = rt.snapshot(0.0);
        // A fresh runtime after a load: the one-shot trigger stays used up.
        let mut rt2 = TriggerRuntime::new();
        rt2.add(volume, "v", d);
        let snap2 = snap.clone();
        assert!(rt2.restore(&snap2, 0.0).is_empty());
        rt2.on_touch(&w.env(), 1.0, volume, hero, &Contact::default());
        assert!(rt2.take_output().is_empty());
        assert!(
            rt2.restore(
                &TriggerSnapshot {
                    triggers: vec![TriggerSave {
                        key: "x".into(),
                        enabled: true,
                        max_activations: -1,
                        counts: BTreeMap::new(),
                        retrigger_left: 0.0,
                        cursor: 0
                    }]
                },
                0.0
            ) == vec!["x".to_owned()]
        );
    }
}
