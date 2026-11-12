//! Compact numeric ids and the string→id registries that the RON content resolves into.
//!
//! Content files use readable string ids (`"raw_iron"`). At load time every string is resolved
//! once into a `u16` id. Gameplay code only ever handles the numeric ids, which keeps the wire
//! format and the chunk palette small.

use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};

/// Compact numeric id for a piece of content (block, item, recipe, machine, tech node).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub struct ContentId(pub u16);

impl ContentId {
    /// The id used for "nothing" in palettes and slots.
    pub const NONE: Self = Self(u16::MAX);

    /// Returns the raw index.
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

impl fmt::Display for ContentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// A bidirectional string ↔ numeric id table. `NONE` is never assigned.
#[derive(Debug, Clone, Default)]
pub struct IdTable {
    names: Vec<String>,
    lookup: HashMap<String, ContentId>,
}

impl IdTable {
    /// Resolves a name, assigning the next free id if it is new.
    pub fn intern(&mut self, name: &str) -> ContentId {
        if let Some(id) = self.lookup.get(name) {
            return *id;
        }
        let id = ContentId(u16::try_from(self.names.len()).expect("content id space exhausted"));
        assert_ne!(id, ContentId::NONE, "content id space exhausted");
        self.names.push(name.to_owned());
        self.lookup.insert(name.to_owned(), id);
        id
    }

    /// Looks a name up without assigning an id.
    pub fn get(&self, name: &str) -> Option<ContentId> {
        self.lookup.get(name).copied()
    }

    /// Returns the name for an id.
    pub fn name(&self, id: ContentId) -> Option<&str> {
        self.names.get(id.index()).map(String::as_str)
    }

    /// Number of interned names.
    pub fn len(&self) -> usize {
        self.names.len()
    }

    /// Whether the table is empty.
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// Iterates all `(id, name)` pairs in id order.
    pub fn iter(&self) -> impl Iterator<Item = (ContentId, &str)> {
        self.names
            .iter()
            .enumerate()
            .map(|(index, name)| (ContentId(index as u16), name.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intern_is_idempotent_and_stable() {
        let mut table = IdTable::default();
        let a = table.intern("iron_ingot");
        let b = table.intern("coal");
        assert_eq!(a, table.intern("iron_ingot"));
        assert_ne!(a, b);
        assert_eq!(table.name(a), Some("iron_ingot"));
        assert_eq!(table.len(), 2);
    }
}
