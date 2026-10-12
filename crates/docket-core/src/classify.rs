//! Item classification: the content string and the three item kinds.
//!
//! The content string is the body minus its trailing tag run (a
//! maximal run of whole tags, separated by whitespace, ending the
//! body, plus the run's trailing whitespace). The kind is computed
//! from the content: task markers first, then the project colon.

use crate::item::ItemKind;
use crate::tags;

/// The body with its trailing tag run removed. A body with no tag
/// run keeps its trailing whitespace, so `foo: ` stays a note.
pub fn content(body: &str) -> &str {
    let all = tags::scan(body);
    let Some(last) = all.last() else {
        return body;
    };
    // The run must end the body (modulo trailing whitespace).
    if !body[last.end..].chars().all(char::is_whitespace) {
        return body;
    }
    let mut cut = last.start;
    for pair in all.windows(2).rev() {
        let (prev, cur) = (&pair[0], &pair[1]);
        if body[prev.end..cur.start].chars().all(char::is_whitespace) {
            cut = prev.start;
        } else {
            break;
        }
    }
    body[..cut].trim_end()
}

/// Classify a body into its item kind.
pub fn kind(body: &str) -> ItemKind {
    let c = content(body);
    if is_task(c) {
        ItemKind::Task
    } else if c.ends_with(':') {
        ItemKind::Project
    } else {
        ItemKind::Note
    }
}

fn is_task(content: &str) -> bool {
    content.starts_with("- ") || content.starts_with("+ ") || content.starts_with("* ")
}

#[cfg(test)]
mod tests {
    use super::{content, kind};
    use crate::item::ItemKind::{Note, Project, Task};

    #[test]
    fn kinds() {
        assert_eq!(kind("- Milk"), Task);
        assert_eq!(kind("+ alternate"), Task);
        assert_eq!(kind("* starred"), Task);
        assert_eq!(kind("Groceries:"), Project);
        assert_eq!(kind("Plain note"), Note);
        assert_eq!(kind(""), Note);
    }

    #[test]
    fn markers_need_a_space() {
        assert_eq!(kind("-5 degrees"), Note);
        assert_eq!(kind("*bold* text"), Note);
        assert_eq!(kind("-"), Note);
        assert_eq!(kind("+"), Note);
    }

    #[test]
    fn trailing_tags_are_stripped_for_classification() {
        assert_eq!(kind("Project: @done"), Project);
        assert_eq!(kind("Project: @done  "), Project);
        assert_eq!(kind("- Milk @due(2026-10-14)"), Task);
        assert_eq!(content("- Milk @due(2026-10-14)"), "- Milk");
        assert_eq!(content("Groceries: @a @b(c)"), "Groceries:");
    }

    #[test]
    fn canonical_consequences() {
        // A task marker wins before the project colon.
        assert_eq!(kind("- foo:"), Task);
        // The colon must end the content: trailing whitespace with
        // no tags means the line is a note.
        assert_eq!(kind("foo: "), Note);
        assert_eq!(content("foo: "), "foo: ");
        // The colon breaks the tag boundary, so this @done is text.
        assert_eq!(kind("Project @done:"), Project);
        assert_eq!(content("Project @done:"), "Project @done:");
    }

    #[test]
    fn only_trailing_tags_leave_the_content() {
        // @y is the trailing run; @x stays in the content.
        assert_eq!(content("a @x b @y"), "a @x b");
        // A tag not at the end is not a run at all.
        assert_eq!(content("a @x b"), "a @x b");
        assert_eq!(content("buy @x"), "buy");
    }

    #[test]
    fn prose_is_untouched() {
        assert_eq!(kind("email jesse@hogbay.com"), Note);
        assert_eq!(kind("see https://example.com/@at docs"), Note);
        assert_eq!(content("email jesse@hogbay.com"), "email jesse@hogbay.com");
    }

    #[test]
    fn a_bare_tag_line_is_an_empty_content_note() {
        assert_eq!(kind("@x"), Note);
        assert_eq!(content("@x"), "");
        assert_eq!(content("@x @y"), "");
    }
}
