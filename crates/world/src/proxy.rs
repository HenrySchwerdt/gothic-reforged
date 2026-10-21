//! Property storage and proxies: how data references entities and edits properties at runtime
//! (`eCEntityProxy`, `eCPropertyProxy`, `eCPropertyProxyCollection`, `eCPropertySetProxy`).
//!
//! * [`EntityProxy`] stores an entity *id* (GUID), not a pointer, and resolves it lazily through
//!   the scene's id index; the resolved entity is cached.
//! * [`PropertyProxy`] holds an expression `[Entity.]Set.Property <op> "value"` that scripts and
//!   data use to change one property of one property set of an entity (`Execute`).
//! * [`PropertyBook`] is the property-set store those proxies and the template patcher edit.
use crate::pvs::EntityId;
use crate::scene::SceneAdmin;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A property value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PropValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
}

impl PropValue {
    /// Parse `text` as the type of `like` (the property keeps its type when edited).
    fn parse_like(like: &Self, text: &str) -> Option<Self> {
        Some(match like {
            Self::Bool(_) => match text.to_ascii_lowercase().as_str() {
                "true" | "1" => Self::Bool(true),
                "false" | "0" => Self::Bool(false),
                _ => return None,
            },
            Self::Int(_) => Self::Int(text.parse().ok()?),
            Self::Float(_) => Self::Float(text.parse().ok()?),
            Self::Str(_) => Self::Str(text.to_owned()),
        })
    }
}

/// One property set of an entity (`eCEntityPropertySet`): a class name and its properties.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PropertySet {
    pub class: String,
    pub props: BTreeMap<String, PropValue>,
}

impl PropertySet {
    pub fn new(class: &str) -> Self {
        Self {
            class: class.to_owned(),
            props: BTreeMap::new(),
        }
    }
    pub fn with(mut self, name: &str, v: PropValue) -> Self {
        self.props.insert(name.to_owned(), v);
        self
    }
}

/// Property sets by entity.
#[derive(Debug, Clone, Default)]
pub struct PropertyBook {
    sets: BTreeMap<EntityId, Vec<PropertySet>>,
}

impl PropertyBook {
    pub fn new() -> Self {
        Self::default()
    }

    /// `eCEntity::GetPropertySet(name)`.
    pub fn set(&self, e: EntityId, class: &str) -> Option<&PropertySet> {
        self.sets
            .get(&e)?
            .iter()
            .find(|s| s.class.eq_ignore_ascii_case(class))
    }
    pub fn set_mut(&mut self, e: EntityId, class: &str) -> Option<&mut PropertySet> {
        self.sets
            .get_mut(&e)?
            .iter_mut()
            .find(|s| s.class.eq_ignore_ascii_case(class))
    }
    pub fn sets(&self, e: EntityId) -> &[PropertySet] {
        self.sets.get(&e).map_or(&[], Vec::as_slice)
    }
    /// `eCEntity::AddPropertySet`: replaces a set of the same class.
    pub fn add(&mut self, e: EntityId, set: PropertySet) {
        let v = self.sets.entry(e).or_default();
        if let Some(old) = v
            .iter_mut()
            .find(|s| s.class.eq_ignore_ascii_case(&set.class))
        {
            *old = set;
        } else {
            v.push(set);
        }
    }
    /// `eCEntity::RemovePropertySet`.
    pub fn remove(&mut self, e: EntityId, class: &str) -> bool {
        let Some(v) = self.sets.get_mut(&e) else {
            return false;
        };
        let n = v.len();
        v.retain(|s| !s.class.eq_ignore_ascii_case(class));
        v.len() != n
    }
    /// The entity is gone.
    pub fn clear(&mut self, e: EntityId) {
        self.sets.remove(&e);
    }
}

/// Port of `eCEntityProxy` (Engine:0x1022e340..0x1022e500): a reference to an entity by id.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EntityProxy {
    guid: String,
    cached: Option<EntityId>,
}

impl EntityProxy {
    pub fn from_guid(guid: &str) -> Self {
        Self {
            guid: guid.to_owned(),
            cached: None,
        }
    }

    /// `SetEntity`: point at a live entity whose id (GUID) is `guid`.
    pub fn set_entity(&mut self, e: EntityId, guid: &str) {
        self.cached = Some(e);
        self.guid = guid.to_owned();
    }

    pub fn guid(&self) -> &str {
        &self.guid
    }

    /// Port of `eCEntityProxy::ResolveEntity` (Engine:0x1022e470) and the cached read of
    /// `operator eCEntity*`: the cached entity while it is registered, else the id lookup.
    pub fn resolve(&mut self, scene: &SceneAdmin) -> Option<EntityId> {
        if let Some(c) = self.cached.filter(|c| scene.is_registered(*c)) {
            return Some(c);
        }
        self.cached = None;
        if self.guid.is_empty() {
            return None;
        }
        self.cached = scene.entity_by_guid(&self.guid);
        self.cached
    }
}

/// `eEOperandValue`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operand {
    Set,
    Add,
    Sub,
    Mul,
    Div,
}

/// A parsed expression of a property proxy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertyExpr {
    /// Entity name; empty = the proxy's owner entity.
    pub entity: String,
    pub set: String,
    pub property: String,
    pub op: Operand,
    pub value: String,
}

/// Why an expression was not applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProxyError {
    /// No operator or value (the original `Execute` accepts this and does nothing).
    Empty,
    BadOperator(String),
    NoEntity(String),
    NoSet(String),
    NoProperty(String),
    BadValue(String),
}

impl PropertyExpr {
    /// Port of `eCPropertyProxy::SplitExpression` (Engine:0x1022ea80) and `SplitAddress`
    /// (0x1022eb70). The expression is `address op "value"`; the address splits at its last two
    /// dots into `[entity.]set.property`.
    pub fn parse(expr: &str) -> Result<Self, ProxyError> {
        let (addr, rest) = match expr.find(['+', '-', '*', '/', '=']) {
            Some(i) => (expr[..i].trim(), &expr[i..]),
            None => return Err(ProxyError::Empty),
        };
        let (op_text, value) = match rest.find('"') {
            Some(q) => (
                rest[..q].trim(),
                rest[q + 1..].replace('"', " ").trim().to_owned(),
            ),
            None => return Err(ProxyError::Empty),
        };
        let op = match op_text {
            "=" => Operand::Set,
            "+=" => Operand::Add,
            "-=" => Operand::Sub,
            "*=" => Operand::Mul,
            "/=" => Operand::Div,
            other => return Err(ProxyError::BadOperator(other.to_owned())),
        };
        let (head, property) = addr.rsplit_once('.').unwrap_or(("", addr));
        let (entity, set) = match head.rfind('.') {
            Some(i) if i > 0 => (&head[..i], &head[i + 1..]),
            _ => ("", head),
        };
        Ok(Self {
            entity: entity.trim().to_owned(),
            set: set.trim().to_owned(),
            property: property.trim().to_owned(),
            op,
            value,
        })
    }
}

/// Port of `eCPropertyProxy` (Engine:0x1022e800..): an expression bound to an owner entity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertyProxy {
    pub expr: String,
}

impl PropertyProxy {
    pub fn new(expr: &str) -> Self {
        Self {
            expr: expr.to_owned(),
        }
    }

    /// Port of `eCPropertyProxy::Execute` (Engine:0x1022e800) with `SetValue`: parse, find the
    /// entity (by name, or the owner), find the set and the property and apply the operator.
    /// An empty expression, or one without operator, does nothing and succeeds.
    pub fn execute(
        &self,
        scene: &SceneAdmin,
        book: &mut PropertyBook,
        owner: EntityId,
    ) -> Result<(), ProxyError> {
        if self.expr.trim().is_empty() {
            return Ok(());
        }
        let e = match PropertyExpr::parse(&self.expr) {
            Ok(e) => e,
            Err(ProxyError::Empty) => return Ok(()),
            Err(err) => return Err(err),
        };
        let target = if e.entity.is_empty() {
            owner
        } else {
            scene
                .entity_by_name(&e.entity)
                .ok_or_else(|| ProxyError::NoEntity(e.entity.clone()))?
        };
        let set = book
            .set_mut(target, &e.set)
            .ok_or_else(|| ProxyError::NoSet(e.set.clone()))?;
        let slot = set
            .props
            .get_mut(&e.property)
            .ok_or_else(|| ProxyError::NoProperty(e.property.clone()))?;
        apply(slot, e.op, &e.value)
    }
}

fn apply(slot: &mut PropValue, op: Operand, text: &str) -> Result<(), ProxyError> {
    let bad = || ProxyError::BadValue(text.to_owned());
    let new = PropValue::parse_like(slot, text).ok_or_else(bad)?;
    *slot = match (op, &*slot, new) {
        (Operand::Set, _, n) => n,
        (Operand::Add, PropValue::Int(a), PropValue::Int(b)) => PropValue::Int(a + b),
        (Operand::Sub, PropValue::Int(a), PropValue::Int(b)) => PropValue::Int(a - b),
        (Operand::Mul, PropValue::Int(a), PropValue::Int(b)) => PropValue::Int(a * b),
        (Operand::Div, PropValue::Int(a), PropValue::Int(b)) if b != 0 => PropValue::Int(a / b),
        (Operand::Add, PropValue::Float(a), PropValue::Float(b)) => PropValue::Float(a + b),
        (Operand::Sub, PropValue::Float(a), PropValue::Float(b)) => PropValue::Float(a - b),
        (Operand::Mul, PropValue::Float(a), PropValue::Float(b)) => PropValue::Float(a * b),
        (Operand::Div, PropValue::Float(a), PropValue::Float(b)) if b != 0.0 => {
            PropValue::Float(a / b)
        }
        _ => return Err(bad()),
    };
    Ok(())
}

/// Port of `eCPropertyProxyCollection`: the proxies of one owner, run in order.
#[derive(Debug, Clone, Default)]
pub struct PropertyProxyCollection {
    proxies: Vec<PropertyProxy>,
}

impl PropertyProxyCollection {
    pub fn add(&mut self, p: PropertyProxy) {
        self.proxies.push(p);
    }
    /// `RemoveProxy`.
    pub fn remove(&mut self, index: usize) -> bool {
        if index < self.proxies.len() {
            self.proxies.remove(index);
            true
        } else {
            false
        }
    }
    pub fn len(&self) -> usize {
        self.proxies.len()
    }
    pub fn is_empty(&self) -> bool {
        self.proxies.is_empty()
    }
    /// Execute all proxies; a failing one does not stop the others. Returns the failures.
    pub fn execute_all(
        &self,
        scene: &SceneAdmin,
        book: &mut PropertyBook,
        owner: EntityId,
    ) -> Vec<(usize, ProxyError)> {
        self.proxies
            .iter()
            .enumerate()
            .filter_map(|(i, p)| p.execute(scene, book, owner).err().map(|e| (i, e)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world() -> (SceneAdmin, PropertyBook, EntityId, EntityId) {
        let mut scene = SceneAdmin::new();
        let mut book = PropertyBook::new();
        let (a, b) = (EntityId(1), EntityId(2));
        scene.register(a, "Owner");
        scene.register(b, "Door");
        book.add(
            b,
            PropertySet::new("gCInteraction_PS")
                .with("Locked", PropValue::Bool(true))
                .with("Hits", PropValue::Int(3))
                .with("Speed", PropValue::Float(1.5)),
        );
        (scene, book, a, b)
    }

    #[test]
    fn expressions_split_into_entity_set_property_operator_value() {
        let e = PropertyExpr::parse("Door.gCInteraction_PS.Hits += \"2\"").unwrap();
        assert_eq!(e.entity, "Door");
        assert_eq!(e.set, "gCInteraction_PS");
        assert_eq!(e.property, "Hits");
        assert_eq!(e.op, Operand::Add);
        assert_eq!(e.value, "2");
        let own = PropertyExpr::parse("gCInteraction_PS.Locked = \"false\"").unwrap();
        assert!(own.entity.is_empty());
        assert_eq!(PropertyExpr::parse("nothing"), Err(ProxyError::Empty));
    }

    #[test]
    fn execute_edits_a_named_entity_or_the_owner() {
        let (scene, mut book, a, b) = world();
        PropertyProxy::new("Door.gCInteraction_PS.Hits += \"2\"")
            .execute(&scene, &mut book, a)
            .unwrap();
        PropertyProxy::new("Door.gCInteraction_PS.Speed *= \"2\"")
            .execute(&scene, &mut book, a)
            .unwrap();
        PropertyProxy::new("gCInteraction_PS.Locked = \"false\"")
            .execute(&scene, &mut book, b)
            .unwrap();
        let s = book.set(b, "gCInteraction_PS").unwrap();
        assert_eq!(s.props["Hits"], PropValue::Int(5));
        assert_eq!(s.props["Speed"], PropValue::Float(3.0));
        assert_eq!(s.props["Locked"], PropValue::Bool(false));
        assert_eq!(
            PropertyProxy::new("Door.Nope.X = \"1\"").execute(&scene, &mut book, a),
            Err(ProxyError::NoSet("Nope".into()))
        );
        assert_eq!(
            PropertyProxy::new("Ghost.A.B = \"1\"").execute(&scene, &mut book, a),
            Err(ProxyError::NoEntity("Ghost".into()))
        );
        assert!(PropertyProxy::new("").execute(&scene, &mut book, a).is_ok());
    }

    #[test]
    fn entity_proxy_resolves_by_id_and_drops_dead_entities() {
        let mut scene = SceneAdmin::new();
        scene.register(EntityId(7), "Chest");
        scene.register_guid(EntityId(7), "ABCD-01");
        let mut p = EntityProxy::from_guid("abcd-01");
        assert_eq!(p.resolve(&scene), Some(EntityId(7)));
        scene.unregister(EntityId(7));
        assert_eq!(p.resolve(&scene), None);
        scene.register(EntityId(9), "Chest");
        scene.register_guid(EntityId(9), "abcd-01");
        assert_eq!(p.resolve(&scene), Some(EntityId(9)));
    }

    #[test]
    fn collection_runs_all_and_reports_failures() {
        let (scene, mut book, a, b) = world();
        let mut c = PropertyProxyCollection::default();
        c.add(PropertyProxy::new("Door.gCInteraction_PS.Hits = \"9\""));
        c.add(PropertyProxy::new("Door.Missing.Hits = \"1\""));
        c.add(PropertyProxy::new("Door.gCInteraction_PS.Hits -= \"4\""));
        let failed = c.execute_all(&scene, &mut book, a);
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].0, 1);
        assert_eq!(
            book.set(b, "gCInteraction_PS").unwrap().props["Hits"],
            PropValue::Int(5)
        );
    }
}
