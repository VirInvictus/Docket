//! The first-run document. Docket's own words: the format's rules,
//! written small enough to read in one screen. Tab-indented, the
//! way the format nests.

pub const WELCOME: &str = "Welcome to Docket\n\
\n\
This file is a plain-text task list. Everything you see is the\n\
file itself: no database, no lock-in, any editor can open it.\n\
The rules are four:\n\
\n\
Projects end with a colon:\n\
\n\
Errands:\n\
\tTasks start with a dash and a space:\n\
\t\t- and indenting nests them under the line above\n\
\t- mark one finished by adding @done\n\
\tNotes are any line that is not a project or a task\n\
\n\
Tags annotate any line. They can take a value:\n\
\n\
\t- file taxes @due(2027-04-15) @priority(1)\n\
\n\
The format even has a search language for tags and text:\n\
\n\
\t@priority(1) and not @done\n\
\tproject Errands @due <[d] next month\n\
\n\
(Docket's search bar arrives in an update.)\n\
\n\
Everything saves as you type. Press Ctrl+O to open a list, or\n\
Ctrl+Shift+S to give this one a name.\n";

#[cfg(test)]
mod tests {
    use super::WELCOME;
    use docket_core::Document;

    #[test]
    fn the_welcome_document_is_valid_taskpaper() {
        let doc = Document::load(WELCOME);
        // Parseable and idempotent: the first-run document must be a
        // well-formed .taskpaper file.
        let once = doc.serialize();
        assert_eq!(Document::load(&once).serialize(), once);
        // It demonstrates each of the four kinds.
        let kinds: Vec<_> = doc.items.iter().map(|i| i.kind()).collect();
        assert!(kinds.contains(&docket_core::ItemKind::Project));
        assert!(kinds.contains(&docket_core::ItemKind::Task));
        assert!(kinds.contains(&docket_core::ItemKind::Note));
    }
}
