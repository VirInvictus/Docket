//! Text load and serialize: the only place normalizations happen.
//!
//! On load: CRLF and CR become LF; the trailing newline is not a
//! line; multi-line space indentation converts to tabs (the smallest
//! common leading-space run becomes the tab width, leftover leading
//! spaces are dropped); blank lines become empty note items adopting
//! the next non-empty line's depth (trailing blanks sit at top
//! level). Serialization writes depth tabs plus the body per item,
//! newline-joined, with no trailing newline. After the first load,
//! every load/serialize cycle is byte-exact.

use crate::document::Document;
use crate::item::Item;

pub fn load(text: &str) -> Document {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines: Vec<String> = normalized.split('\n').map(str::to_owned).collect();
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    normalize_space_indent(&mut lines);

    struct Raw {
        depth: u32,
        body: String,
        blank: bool,
    }
    let raw: Vec<Raw> = lines
        .into_iter()
        .map(|line| {
            let tabs = line.chars().take_while(|&c| c == '\t').count();
            let body = line[tabs..].to_owned();
            let blank = body.chars().all(char::is_whitespace);
            Raw {
                depth: tabs as u32,
                body: if blank { String::new() } else { body },
                blank,
            }
        })
        .collect();

    let mut items = Vec::with_capacity(raw.len());
    for (i, r) in raw.iter().enumerate() {
        let depth = if r.blank {
            raw[i + 1..]
                .iter()
                .find(|n| !n.blank)
                .map_or(0, |n| n.depth)
        } else {
            r.depth
        };
        items.push(Item {
            depth,
            body: r.body.clone(),
        });
    }
    Document { items }
}

pub fn serialize(doc: &Document) -> String {
    let mut out = String::new();
    for (i, item) in doc.items.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        for _ in 0..item.depth {
            out.push('\t');
        }
        out.push_str(&item.body);
    }
    out
}

/// Convert space-indented lines to tabs, multi-line documents only.
/// The smallest common leading-space run becomes the tab width; a
/// run of that width becomes one tab and the leftover spaces drop.
fn normalize_space_indent(lines: &mut [String]) {
    if lines.len() < 2 {
        return;
    }
    let width = lines
        .iter()
        .filter_map(|l| {
            let n = l.chars().take_while(|&c| c == ' ').count();
            (n > 0).then_some(n)
        })
        .min();
    let Some(width) = width else {
        return;
    };
    for line in lines.iter_mut() {
        if !line.starts_with(' ') {
            continue;
        }
        let n = line.chars().take_while(|&c| c == ' ').count();
        let tabs = n / width;
        *line = "\t".repeat(tabs) + &line[n..];
    }
}

#[cfg(test)]
mod tests {
    use super::{load, serialize};
    use crate::item::ItemKind;

    #[test]
    fn line_endings_normalize() {
        let doc = load("a:\r\n\t- b\rc\n");
        assert_eq!(doc.items.len(), 3);
        assert_eq!(serialize(&doc), "a:\n\t- b\nc");
    }

    #[test]
    fn trailing_newline_is_not_a_line_but_a_blank_line_is() {
        assert_eq!(load("a").items.len(), 1);
        assert_eq!(load("a\n").items.len(), 1);
        let doc = load("a\n\n");
        assert_eq!(doc.items.len(), 2);
        assert_eq!(doc.items[1].kind(), ItemKind::Note);
        assert_eq!(doc.items[1].body, "");
    }

    #[test]
    fn blank_lines_adopt_the_next_non_empty_depth() {
        // The blank between a: and the child adopts the child's depth.
        let doc = load("a:\n\n\t- b");
        assert_eq!(doc.items[1].depth, 1);
        // Serialized form is stable from the second pass on.
        let s1 = serialize(&doc);
        assert_eq!(Document::load(&s1).serialize(), s1);
    }

    #[test]
    fn an_indented_blank_line_normalizes_to_the_adopted_depth() {
        let doc = load("a:\n\t\nb");
        assert_eq!(doc.items[1].depth, 0);
        assert_eq!(serialize(&doc), "a:\n\nb");
    }

    #[test]
    fn trailing_blanks_sit_at_top_level() {
        let doc = load("a:\n\t- b\n\n\n");
        assert_eq!(doc.items.len(), 4);
        assert_eq!(doc.items[2].depth, 0);
        assert_eq!(doc.items[3].depth, 0);
    }

    #[test]
    fn space_indent_converts_to_tabs() {
        let doc = load("Root:\n    - one\n        - nested\nNote\n");
        assert_eq!(doc.items[1].depth, 1);
        assert_eq!(doc.items[2].depth, 2);
        assert_eq!(serialize(&doc), "Root:\n\t- one\n\t\t- nested\nNote");
    }

    #[test]
    fn mixed_space_runs_use_the_smallest_common_width() {
        // 2-space minimum: the 4-space line becomes two tabs.
        let doc = load("A:\n  - a\n    - b\n");
        assert_eq!(doc.items[1].depth, 1);
        assert_eq!(doc.items[2].depth, 2);
    }

    #[test]
    fn leftover_spaces_drop() {
        // 3 spaces in a 4-width document: no full run, so it drops
        // to a blank line.
        let doc = load("A:\n    - a\n   \n    - b\n");
        assert_eq!(doc.items[2].body, "");
        assert_eq!(doc.items[2].depth, 1);
    }

    #[test]
    fn single_line_documents_are_untouched() {
        let src = "  just a note with leading spaces";
        assert_eq!(serialize(&load(src)), src);
    }

    #[test]
    fn an_empty_file_loads_empty() {
        assert!(load("").items.is_empty());
    }

    #[test]
    fn over_indent_is_preserved() {
        let src = "A:\n\t- child\n\t\t\t- deep\n";
        let doc = load(src);
        assert_eq!(doc.items[2].depth, 3);
        assert_eq!(doc.parent_of(2), Some(1));
        assert_eq!(serialize(&doc), "A:\n\t- child\n\t\t\t- deep");
    }

    use crate::document::Document;
}
