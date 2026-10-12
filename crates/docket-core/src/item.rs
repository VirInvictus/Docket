//! The item model: one line of the document, its depth, and its body.

/// The three kinds of items a `.taskpaper` line can be. A blank line
/// loads as an empty [`ItemKind::Note`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Project,
    Task,
    Note,
}

/// One item: its nesting depth in tabs, and its body (the line text
/// without the leading tabs). The body carries everything else:
/// type syntax, text, and tags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub depth: u32,
    pub body: String,
}

impl Item {
    pub fn kind(&self) -> ItemKind {
        crate::classify::kind(&self.body)
    }

    /// The body minus its trailing tag run.
    pub fn content(&self) -> &str {
        crate::classify::content(&self.body)
    }

    /// Every tag occurrence in the body, in order.
    pub fn tags(&self) -> Vec<crate::tags::Tag<'_>> {
        crate::tags::scan(&self.body)
    }
}
