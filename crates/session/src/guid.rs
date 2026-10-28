//! How an entity reference finds its entity: by the 16-byte GUID, never by the whole 20 bytes.
//!
//! Evidence (Engine.dll / SharedBase, decompile under the gitignored `.local/research/engine`):
//! `eCEntityProxy::ResolveEntity` (0x1022e470) looks the entity up with the proxy's `bCPropertyID`
//! (proxy + 8) as the key, `eCEntityProxy::Compare` (0x1022e250) is `bCPropertyID::operator==`
//! (0x1000ea00), and that compares exactly four dwords (offsets 0, 4, 8 and 0xc, 16 bytes); the
//! fifth dword of the 20-byte stored value (the "validity word", `PROPERTY_STREAMS.md`) is not part
//! of the identity. `bCGuid::operator==` (0x100025b0 -> 0x10002120) compares 16 bytes as well.
//! The characters of the world data refer to their routine points with a tail that differs from the
//! entity's own, so an exact match on the 40 hex digits misses points that the engine finds.
use std::collections::HashMap;

/// Hex digits of the identity part of a persistent id (16 bytes).
pub const GUID_HEX_DIGITS: usize = 32;

/// The identity part of a persistent id given as hex (20 bytes = 40 digits, or the bare 16 bytes).
pub fn guid_key(id: &str) -> String {
    id.chars()
        .take(GUID_HEX_DIGITS)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Entities by the identity of their id. The first entity registered under an identity wins
/// (instances of one template share the 16 bytes only by accident of the export, never by design).
#[derive(Debug, Clone)]
pub struct GuidIndex<T> {
    map: HashMap<String, T>,
}

impl<T> Default for GuidIndex<T> {
    fn default() -> Self {
        Self {
            map: HashMap::new(),
        }
    }
}

impl<T> GuidIndex<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register `value` for `id`; returns false when the identity was taken already.
    pub fn insert(&mut self, id: &str, value: T) -> bool {
        match self.map.entry(guid_key(id)) {
            std::collections::hash_map::Entry::Vacant(v) => {
                v.insert(value);
                true
            }
            std::collections::hash_map::Entry::Occupied(_) => false,
        }
    }

    /// The entity a reference (`bCPropertyID`, 20 bytes) points to.
    pub fn get(&self, id: &str) -> Option<&T> {
        self.map.get(&guid_key(id))
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tail_is_not_part_of_the_identity() {
        let mut idx = GuidIndex::new();
        assert!(idx.insert("93F5866CFAC12744845E80AEF1734AC100000000", 1));
        assert_eq!(idx.get("93f5866cfac12744845e80aef1734ac1deadbeef"), Some(&1));
        assert_eq!(idx.get("93f5866cfac12744845e80aef1734ac1"), Some(&1));
        assert_eq!(idx.get("93f5866cfac12744845e80aef1734ac2"), None);
        assert!(!idx.insert("93f5866cfac12744845e80aef1734ac100000001", 2));
    }
}
