//! Bone attachments of the visual animation property set (E04): items, weapons and effects that
//! follow a bone of the character (`eCActorAttachment`, `eCVisualAnimation_PS::AddAttachment`).
//!
//! Plain bookkeeping, generic over the entity id. The caller supplies bone lookup and the node
//! matrices and applies the returned world matrices.
use bevy::math::Affine3A;

/// One attachment: the entity follows `bone` with `offset`.
#[derive(Clone, Debug)]
pub struct Attachment<E> {
    pub entity: E,
    pub bone: String,
    /// Offset from the bone (identity when none was given).
    pub offset: Affine3A,
    /// Cached node of the bone; reset by [`AttachmentSet::relink`].
    node: Option<u16>,
}

/// What a frame of attachment processing did.
#[derive(Debug)]
pub struct ProcessResult<E> {
    /// New world matrix per attached entity.
    pub placed: Vec<(E, Affine3A)>,
    /// Entities whose bone does not exist on the skeleton ("Bone not found").
    pub missing_bone: Vec<E>,
}

/// Persistent and one-shot attachments of a character.
#[derive(Clone, Debug)]
pub struct AttachmentSet<E> {
    persistent: Vec<Attachment<E>>,
    once: Vec<Attachment<E>>,
}

impl<E> Default for AttachmentSet<E> {
    fn default() -> Self {
        Self {
            persistent: Vec::new(),
            once: Vec::new(),
        }
    }
}

impl<E: Copy + PartialEq> AttachmentSet<E> {
    /// Port of eCVisualAnimation_PS::AddAttachment (Engine:0x10030da0): any persistent
    /// attachment of the same entity is removed first, then the new one is appended, so adding
    /// twice never duplicates and the last bone wins.
    pub fn add(&mut self, entity: E, bone: &str, offset: Option<Affine3A>) {
        gothic_portcov::hit!("add");
        self.remove(entity);
        self.persistent.push(Attachment {
            entity,
            bone: bone.to_owned(),
            offset: offset.unwrap_or(Affine3A::IDENTITY),
            node: None,
        });
    }

    /// Port of eCVisualAnimation_PS::AddOnceAttachment (Engine:0x10030f50): like [`add`] but
    /// the attachment is placed a single time. The entity's own rendering and processing are
    /// switched off by the caller (the item is drawn through the character).
    ///
    /// [`add`]: AttachmentSet::add
    pub fn add_once(&mut self, entity: E, bone: &str) {
        gothic_portcov::hit!("add_once");
        // The original only searches the persistent list here.
        self.remove(entity);
        self.once.push(Attachment {
            entity,
            bone: bone.to_owned(),
            offset: Affine3A::IDENTITY,
            node: None,
        });
    }

    /// Port of eCVisualAnimation_PS::RemoveAttachment (Engine:0x10030e20): removes the
    /// persistent attachment of `entity`, keeping the order of the others.
    pub fn remove(&mut self, entity: E) -> bool {
        gothic_portcov::hit!("remove");
        match self.persistent.iter().position(|a| a.entity == entity) {
            Some(i) => {
                self.persistent.remove(i);
                true
            }
            None => false,
        }
    }

    /// Port of eCVisualAnimation_PS::RemoveOnceAttachment (Engine:0x10030fe0).
    pub fn remove_once(&mut self, entity: E) -> bool {
        gothic_portcov::hit!("remove_once");
        match self.once.iter().position(|a| a.entity == entity) {
            Some(i) => {
                self.once.remove(i);
                true
            }
            None => false,
        }
    }

    /// Port of ClearAttachments (Engine:0x10030bc0).
    pub fn clear(&mut self) {
        gothic_portcov::hit!("clear");
        self.persistent.clear();
    }

    /// Port of ClearOnceAttachments (Engine:0x10030cb0).
    pub fn clear_once(&mut self) {
        gothic_portcov::hit!("clear_once");
        self.once.clear();
    }

    /// Port of RelinkAttachments (Engine:0x10031110): forgets the cached bone nodes (the
    /// skeleton changed, for instance after a head was attached).
    pub fn relink(&mut self) {
        gothic_portcov::hit!("relink");
        for a in self.persistent.iter_mut().chain(&mut self.once) {
            a.node = None;
        }
    }

    pub fn persistent(&self) -> &[Attachment<E>] {
        &self.persistent
    }

    pub fn once(&self) -> &[Attachment<E>] {
        &self.once
    }

    pub fn is_empty(&self) -> bool {
        self.persistent.is_empty() && self.once.is_empty()
    }

    pub fn contains(&self, entity: E) -> bool {
        self.persistent.iter().any(|a| a.entity == entity)
    }

    /// Port of the attachment part of OnProcess (Engine:0x10032d40, `eCActorAttachment::Process`
    /// 0x1002a5b0 and `ProcessOnce` 0x1002a820): the one-shot list is flushed first (each entry
    /// placed once and dropped), then every persistent attachment is placed at
    /// `actor_world * node_world * offset`. `node_of` resolves a bone name, `node_world` gives
    /// the node's matrix in actor space. `is_valid` drops attachments whose entity vanished.
    pub fn process(
        &mut self,
        actor_world: Affine3A,
        node_of: impl Fn(&str) -> Option<u16>,
        node_world: impl Fn(u16) -> Affine3A,
        is_valid: impl Fn(E) -> bool,
    ) -> ProcessResult<E> {
        gothic_portcov::hit!("process");
        let mut result = ProcessResult {
            placed: Vec::new(),
            missing_bone: Vec::new(),
        };
        for a in std::mem::take(&mut self.once) {
            match node_of(&a.bone) {
                Some(n) => result
                    .placed
                    .push((a.entity, actor_world * node_world(n) * a.offset)),
                None => result.missing_bone.push(a.entity),
            }
        }
        self.persistent.retain(|a| is_valid(a.entity));
        for a in &mut self.persistent {
            if a.node.is_none() {
                a.node = node_of(&a.bone);
            }
            match a.node {
                Some(n) => result
                    .placed
                    .push((a.entity, actor_world * node_world(n) * a.offset)),
                None => result.missing_bone.push(a.entity),
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::Vec3;

    fn node_world(n: u16) -> Affine3A {
        Affine3A::from_translation(Vec3::new(f32::from(n), 0.0, 0.0))
    }

    fn lookup(name: &str) -> Option<u16> {
        match name {
            "Slot_RightHand" => Some(5),
            "Slot_Back" => Some(2),
            _ => None,
        }
    }

    #[test]
    fn adding_twice_keeps_one_attachment_with_the_last_bone() {
        let mut set = AttachmentSet::<u32>::default();
        set.add(7, "Slot_Back", None);
        set.add(8, "Slot_Back", None);
        set.add(7, "Slot_RightHand", None);
        assert_eq!(set.persistent().len(), 2);
        // Re-added entity moves to the end with the new bone.
        assert_eq!(set.persistent()[1].entity, 7);
        assert_eq!(set.persistent()[1].bone, "Slot_RightHand");
        assert!(set.remove(8));
        assert!(!set.remove(8));
    }

    #[test]
    fn placement_is_actor_times_node_times_offset() {
        let mut set = AttachmentSet::<u32>::default();
        set.add(
            1,
            "Slot_RightHand",
            Some(Affine3A::from_translation(Vec3::Y * 2.0)),
        );
        let actor = Affine3A::from_translation(Vec3::Z * 100.0);
        let r = set.process(actor, lookup, node_world, |_| true);
        assert_eq!(r.placed.len(), 1);
        let t = r.placed[0].1.translation;
        assert!((Vec3::from(t) - Vec3::new(5.0, 2.0, 100.0)).length() < 1e-5);
    }

    #[test]
    fn once_attachments_flush_and_missing_bones_are_reported() {
        let mut set = AttachmentSet::<u32>::default();
        set.add_once(1, "Slot_Back");
        set.add_once(2, "Slot_Nowhere");
        let r = set.process(Affine3A::IDENTITY, lookup, node_world, |_| true);
        assert_eq!(r.placed.len(), 1);
        assert_eq!(r.missing_bone, vec![2]);
        assert!(set.once().is_empty(), "one-shot list is flushed");
        // Persistent attachments with an invalid entity are dropped.
        set.add(3, "Slot_Back", None);
        let r = set.process(Affine3A::IDENTITY, lookup, node_world, |e| e != 3);
        assert!(r.placed.is_empty());
        assert!(set.persistent().is_empty());
    }

    #[test]
    fn relink_forgets_cached_nodes() {
        let mut set = AttachmentSet::<u32>::default();
        set.add(1, "Slot_Back", None);
        set.process(Affine3A::IDENTITY, lookup, node_world, |_| true);
        assert!(set.persistent()[0].node.is_some());
        set.relink();
        assert!(set.persistent()[0].node.is_none());
        // Adding as once-attachment removes the same entity's persistent one.
        set.add_once(1, "Slot_Back");
        assert!(!set.contains(1));
    }
}
