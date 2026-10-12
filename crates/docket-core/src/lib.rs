//! Docket's plain-text engine: the TaskPaper-format document parser,
//! the search language, and the date system. Spec.md is the contract;
//! this crate implements it headless, with no GUI dependencies.

/// The three kinds of items a `.taskpaper` line can be. A blank line
/// is a group separator, not an item. Seeded from the spec's document
/// model; Phase 1 grows the full item tree around it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Project,
    Task,
    Note,
}

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
