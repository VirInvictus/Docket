//! The document model: a flat list of items whose depths carry the
//! nesting, mirroring how the text form works.

use crate::item::Item;

/// A parsed `.taskpaper` document.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Document {
    pub items: Vec<Item>,
}

impl Document {
    /// Parse document text. Normalizations happen once here (line
    /// endings, space indentation, blank-line adoption); after that,
    /// load/serialize round-trips are byte-exact.
    pub fn load(text: &str) -> Document {
        crate::text::load(text)
    }

    /// Serialize to document text: depth tabs plus the body per
    /// item, newline-joined, with no trailing newline.
    pub fn serialize(&self) -> String {
        crate::text::serialize(self)
    }

    /// Index of the nearest preceding item with a smaller depth
    /// (the nearest-shallower parent), if any.
    pub fn parent_of(&self, index: usize) -> Option<usize> {
        let depth = self.items.get(index)?.depth;
        (0..index).rev().find(|&i| self.items[i].depth < depth)
    }
}
