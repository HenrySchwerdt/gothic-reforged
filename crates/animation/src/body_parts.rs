//! Named body parts of one actor instance: the part list of `eCVisualAnimationFactory`.
//!
//! An actor is assembled from the base actor plus a list of named body parts (head, body,
//! hair ...). Each part has a mesh for the level of detail 0 and optionally one for the
//! second level, and a visibility flag. Names are compared case-insensitively.

/// One part of the assembled actor.
#[derive(Debug, Clone, PartialEq)]
pub struct BodyPart {
    pub name: String,
    /// Mesh/actor file of the first level of detail.
    pub lod0: String,
    /// Optional second level (`eCVisualAnimation_PS` +0xa0).
    pub lod1: Option<String>,
    pub visible: bool,
    /// Distance factor copied from the owner's LoD setting (+0x90).
    pub lod_scale: f32,
}

/// What a mutation of the part list means for the actor instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartChange {
    /// Nothing happened (unknown part, unchanged state).
    None,
    /// The set of meshes changed: the render skeleton has to be rebuilt and unused bones
    /// dropped (`RemoveNotUsedBones`).
    Rebuild,
    /// Only a visibility flag flipped.
    Visibility,
}

/// Ordered part list with the factory's add / remove / visibility rules.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BodyPartSet {
    parts: Vec<BodyPart>,
}

impl BodyPartSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn parts(&self) -> &[BodyPart] {
        &self.parts
    }

    pub fn len(&self) -> usize {
        self.parts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    fn position(&self, name: &str) -> Option<usize> {
        self.parts
            .iter()
            .position(|p| p.name.eq_ignore_ascii_case(name))
    }

    pub fn get(&self, name: &str) -> Option<&BodyPart> {
        self.position(name).map(|i| &self.parts[i])
    }

    /// Port of eCVisualAnimationFactory::AddBodyPart (Engine:0x1003a270): an existing part of
    /// the same name is removed first; the new part is visible. An empty first-level mesh
    /// name is refused (the original needs the owner's actor to exist).
    pub fn add(
        &mut self,
        name: &str,
        lod0: &str,
        lod1: Option<&str>,
        lod_scale: f32,
    ) -> PartChange {
        gothic_portcov::hit!("add");
        self.remove(name);
        if lod0.is_empty() {
            return PartChange::None;
        }
        self.parts.push(BodyPart {
            name: name.to_string(),
            lod0: lod0.to_string(),
            lod1: lod1.filter(|s| !s.is_empty()).map(str::to_string),
            visible: true,
            lod_scale,
        });
        PartChange::Rebuild
    }

    /// Port of eCVisualAnimationFactory::RemoveBodyPart (Engine:0x1003a3b0): drop the part
    /// and let the caller strip bones no remaining mesh uses.
    pub fn remove(&mut self, name: &str) -> PartChange {
        gothic_portcov::hit!("remove");
        match self.position(name) {
            Some(i) => {
                self.parts.remove(i);
                PartChange::Rebuild
            }
            None => PartChange::None,
        }
    }

    /// Port of eCVisualAnimationFactory::SetBodyPartVisible (Engine:0x1003a430).
    pub fn set_visible(&mut self, name: &str, visible: bool) -> PartChange {
        gothic_portcov::hit!("set_visible");
        match self.position(name) {
            Some(i) => {
                let changed = self.parts[i].visible != visible;
                self.parts[i].visible = visible;
                if changed {
                    PartChange::Visibility
                } else {
                    PartChange::None
                }
            }
            None => PartChange::None,
        }
    }

    /// Port of eCVisualAnimationFactory::IsBodyPartVisible (Engine:0x1003a480): false for
    /// unknown parts.
    pub fn is_visible(&self, name: &str) -> bool {
        gothic_portcov::hit!("is_visible");
        self.get(name).is_some_and(|p| p.visible)
    }

    /// Meshes to draw at a level of detail: visible parts, the second level where a part has
    /// one and `lod >= 1`, otherwise the first.
    pub fn meshes_for_lod(&self, lod: u8) -> impl Iterator<Item = &str> {
        self.parts.iter().filter(|p| p.visible).map(move |p| {
            if lod >= 1 {
                p.lod1.as_deref().unwrap_or(&p.lod0)
            } else {
                p.lod0.as_str()
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_replaces_a_part_of_the_same_name() {
        let mut s = BodyPartSet::new();
        assert_eq!(
            s.add("Slot_Body", "body_a.xact", None, 1.0),
            PartChange::Rebuild
        );
        assert_eq!(
            s.add("slot_body", "body_b.xact", Some("body_b_l.xact"), 1.0),
            PartChange::Rebuild
        );
        assert_eq!(s.len(), 1);
        assert_eq!(s.get("SLOT_BODY").unwrap().lod0, "body_b.xact");
        assert!(s.is_visible("Slot_Body"));
    }

    #[test]
    fn empty_mesh_is_refused_but_still_removes_the_old_part() {
        let mut s = BodyPartSet::new();
        s.add("Slot_Head", "head.xact", None, 1.0);
        assert_eq!(s.add("Slot_Head", "", None, 1.0), PartChange::None);
        assert!(s.is_empty());
    }

    #[test]
    fn visibility_flag_and_unknown_parts() {
        let mut s = BodyPartSet::new();
        s.add("Slot_Hair", "hair.xact", None, 1.0);
        assert_eq!(s.set_visible("slot_hair", false), PartChange::Visibility);
        assert_eq!(s.set_visible("slot_hair", false), PartChange::None);
        assert!(!s.is_visible("Slot_Hair"));
        assert_eq!(s.set_visible("nope", true), PartChange::None);
        assert!(!s.is_visible("nope"));
        assert_eq!(s.remove("nope"), PartChange::None);
    }

    #[test]
    fn lod_selection_uses_the_second_mesh_when_present() {
        let mut s = BodyPartSet::new();
        s.add("a", "a0", Some("a1"), 1.0);
        s.add("b", "b0", None, 1.0);
        s.set_visible("b", false);
        s.add("c", "c0", None, 1.0);
        assert_eq!(s.meshes_for_lod(0).collect::<Vec<_>>(), vec!["a0", "c0"]);
        assert_eq!(s.meshes_for_lod(1).collect::<Vec<_>>(), vec!["a1", "c0"]);
    }
}
