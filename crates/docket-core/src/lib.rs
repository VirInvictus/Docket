//! Docket's plain-text engine: the TaskPaper-format document parser,
//! the search language, and the date system. Spec.md is the contract;
//! this crate implements it headless, with no GUI dependencies.
//!
//! The format engine (spec "Document model") lives in `tags` (tag
//! scanning), `classify` (content and item kinds), `text` (load,
//! serialize, and the one-time normalizations), and `document` (the
//! item list and nearest-shallower parenting).

pub mod classify;
pub mod document;
pub mod item;
pub mod tags;
pub mod text;

pub use document::Document;
pub use item::{Item, ItemKind};

#[cfg(test)]
mod tests {
    use super::ItemKind;

    /// The workspace version must always match the VERSION file at
    /// the repo root, the single source of truth.
    #[test]
    fn workspace_version_matches_version_file() {
        let version_file = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../VERSION"));
        assert_eq!(env!("CARGO_PKG_VERSION"), version_file.trim());
    }

    #[test]
    fn item_kinds_are_distinct() {
        assert_ne!(ItemKind::Project, ItemKind::Task);
        assert_ne!(ItemKind::Task, ItemKind::Note);
        assert_ne!(ItemKind::Note, ItemKind::Project);
    }
}
