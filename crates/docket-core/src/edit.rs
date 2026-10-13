//! Editing operations on document text, implemented as pure
//! text-in/text-out functions so they test headless. Offsets are in
//! characters (GtkTextIter offsets are characters too, so the app
//! converts nothing).
//!
//! Policy lives in the app: which lines are visible (folds), the
//! hoist floor, and the insert anchors. This module is the
//! mechanics, transcribed from the reference application's editing
//! semantics.

/// The result of an edit: the new text and the new caret (in chars).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditResult {
    pub text: String,
    pub caret: usize,
}

struct Lines {
    chars: Vec<char>,
    /// Char index where each line starts, plus a one-past-end sentinel.
    starts: Vec<usize>,
}

impl Lines {
    fn new(text: &str) -> Self {
        let chars: Vec<char> = text.chars().collect();
        let mut starts = vec![0];
        for (i, &c) in chars.iter().enumerate() {
            if c == '\n' {
                starts.push(i + 1);
            }
        }
        starts.push(chars.len() + 1);
        Lines { chars, starts }
    }

    fn line_count(&self) -> usize {
        self.starts.len() - 1
    }

    /// The char range [start, end) of a line, excluding its newline.
    fn range(&self, line: usize) -> (usize, usize) {
        let start = self.starts[line];
        let end = (self.starts[line + 1] - 1).min(self.chars.len());
        (start, end)
    }

    fn line(&self, line: usize) -> String {
        let (s, e) = self.range(line);
        self.chars[s..e].iter().collect()
    }

    fn depth(&self, line: usize) -> i64 {
        let (s, _) = self.range(line);
        self.chars[s..].iter().take_while(|&&c| c == '\t').count() as i64
    }

    /// The caret's line and the char offset within that line.
    fn locate(&self, caret: usize) -> (usize, usize) {
        for line in 0..self.line_count() {
            let (s, e) = self.range(line);
            let end_with_nl = if e < self.chars.len() { e + 1 } else { e };
            if caret >= s && caret <= end_with_nl {
                return (line, caret - s);
            }
        }
        let last = self.line_count() - 1;
        (last, self.range(last).1)
    }

    fn join(&self) -> String {
        self.chars.iter().collect()
    }
}

fn tabs(n: i64) -> String {
    "\t".repeat(n.max(0) as usize)
}

/// The task-marker lead of a line's content: 2 chars for a dash
/// task, 0 otherwise. Only the dash form autoformats and continues,
/// exactly like the reference editor.
fn marker_lead(content: &str) -> usize {
    if content.starts_with("- ") { 2 } else { 0 }
}

/// Enter. `anchor_depth` is the depth of the next displayed line
/// after the caret's line, as the app computes it from fold state
/// (None when there is none); it implements the reference depth
/// formula, so Enter at the end of an expanded parent inserts the
/// first child and Enter at a leaf or collapsed parent inserts a
/// sibling after the branch.
///
/// Rules, in the reference order:
/// - caret at or before the marker lead of a non-empty item: a new
///   empty item inserts ABOVE, at the same depth; the caret stays
///   on the original item.
/// - caret exactly after the lead of a bare `- ` item (autoformat
///   on): the marker strips in place and the item becomes an empty
///   note; nothing is inserted.
/// - everything else splits at the caret: the tail becomes a new
///   item below, prefixed with the dash when the current line has
///   one and the tail lacks it, at the formula depth.
pub fn smart_enter(
    text: &str,
    caret: usize,
    autoformat: bool,
    anchor_depth: Option<i64>,
) -> Option<EditResult> {
    let lines = Lines::new(text);
    if lines.line_count() == 0 {
        return None;
    }
    let (line_no, caret_in_line) = lines.locate(caret);
    let depth = lines.depth(line_no);
    let line_text = lines.line(line_no);
    let content_off = line_text.chars().take_while(|&c| c == '\t').count();
    let content = &line_text[content_off..];
    let lead = if autoformat { marker_lead(content) } else { 0 };
    let (s, e) = lines.range(line_no);

    // Strip the bare marker: caret exactly after "- " on a bare
    // dash item (autoformat only).
    if autoformat && lead == 2 && caret_in_line == content_off + lead && content.trim_end() == "-" {
        let mut chars = lines.chars.clone();
        chars.drain(s + content_off..e);
        return Some(EditResult {
            text: chars.iter().collect(),
            caret: s + content_off,
        });
    }

    // New empty item above: caret at or before the marker lead of a
    // non-empty item.
    if caret_in_line <= content_off + lead && !content.is_empty() {
        let inserted: Vec<char> = format!("{}\n", tabs(depth)).chars().collect();
        let len = inserted.len();
        let mut chars = lines.chars.clone();
        chars.splice(s..s, inserted);
        return Some(EditResult {
            text: chars.iter().collect(),
            caret: caret + len,
        });
    }

    // Split at the caret. The tail (with its tags) MOVES to the new
    // item; a dash task continues the list, a note or project does
    // not.
    let new_depth = depth.max(anchor_depth.unwrap_or(depth));
    let tail: String = line_text.chars().skip(caret_in_line).collect();
    let dash = lead == 2 && !tail.starts_with("- ");
    let new_line = format!(
        "{}{}{}",
        tabs(new_depth),
        if dash { "- " } else { "" },
        tail
    );
    let mut chars = lines.chars.clone();
    // The tail leaves the original line...
    chars.drain(s + caret_in_line..e);
    // ...and rides below it as the new item.
    let inserted: Vec<char> = format!("\n{new_line}").chars().collect();
    let at = s + caret_in_line;
    chars.splice(at..at, inserted);
    let caret = at
        + 1
        + if dash {
            new_depth as usize + 2
        } else {
            new_depth as usize
        };
    Some(EditResult {
        text: chars.iter().collect(),
        caret,
    })
}

/// Shift a range of lines by `delta` tab stops, floored at `floor`.
/// Returns None when nothing changed (the caller treats that as
/// "cannot move further").
pub fn shift_lines(
    text: &str,
    first_line: usize,
    last_line: usize,
    delta: i64,
    floor: i64,
) -> Option<String> {
    let mut lines = Lines::new(text);
    if first_line > last_line || last_line >= lines.line_count() {
        return None;
    }
    let mut changed = false;
    for line in first_line..=last_line {
        let depth = lines.depth(line);
        let new_depth = (depth + delta).max(floor);
        if new_depth == depth {
            continue;
        }
        changed = true;
        let (s, e) = lines.range(line);
        let content_start = lines.chars[s..e]
            .iter()
            .position(|&c| c != '\t')
            .map(|p| s + p)
            .unwrap_or(e);
        let new_tabs: Vec<char> = tabs(new_depth).chars().collect();
        let old_len = content_start - s;
        let _ = old_len;
        lines
            .chars
            .splice(content_start..content_start, new_tabs.clone());
        // The old tabs between s and content_start go away.
        let removed = content_start - s;
        lines.chars.drain(s..content_start);
        let shift = new_tabs.len() as i64 - removed as i64;
        let _ = old_len;
        if shift != 0 {
            for st in lines.starts.iter_mut().skip(line + 1) {
                *st = (*st as i64 + shift) as usize;
            }
        }
    }
    if !changed {
        return None;
    }
    Some(lines.join())
}

/// The last line of item `line`'s subtree, inclusive.
pub fn subtree_end(text: &str, line: usize) -> usize {
    let lines = Lines::new(text);
    if line >= lines.line_count() {
        return line;
    }
    let depth = lines.depth(line);
    let mut last = line;
    for i in line + 1..lines.line_count() {
        if lines.depth(i) <= depth {
            break;
        }
        last = i;
    }
    last
}

/// The first line of the outermost ancestor of `line`.
pub fn branch_start(text: &str, line: usize) -> usize {
    let lines = Lines::new(text);
    let mut cur = line;
    loop {
        let depth = lines.depth(cur);
        let parent = (0..cur).rev().find(|&i| lines.depth(i) < depth);
        match parent {
            Some(p) => cur = p,
            None => return cur,
        }
    }
}

/// Move the block of lines so it lands immediately before line
/// `insert_before` (an original index outside the block). All other
/// lines keep their order, and the newline structure survives.
pub fn move_block(
    text: &str,
    first_line: usize,
    last_line: usize,
    insert_before: usize,
) -> Option<String> {
    let lines = Lines::new(text);
    if first_line > last_line || last_line >= lines.line_count() {
        return None;
    }
    if (first_line..=last_line + 1).contains(&insert_before) {
        return None;
    }
    let mut parts: Vec<Vec<char>> = Vec::new();
    for i in 0..lines.line_count() {
        if i == insert_before {
            push_block(&mut parts, &lines, first_line, last_line);
        }
        if i < first_line || i > last_line {
            let (s, e) = lines.range(i);
            let end_with_nl = (e + 1).min(lines.chars.len());
            parts.push(lines.chars[s..end_with_nl].to_vec());
        }
    }
    if insert_before >= lines.line_count() {
        push_block(&mut parts, &lines, first_line, last_line);
    }
    Some(parts.into_iter().flatten().collect())
}

fn push_block(parts: &mut Vec<Vec<char>>, lines: &Lines, first: usize, last: usize) {
    for l in first..=last {
        parts.push(line_chars(lines, l));
        // Every line except the block's last carries its newline
        // inside the block; the last one gets one when more lines
        // follow in the parts list (joined at the end).
        if l == last {
            // The newline between the block and what follows is
            // supplied by the next pushed line via join; nothing to
            // add here because lines() below re-joins with the
            // stored newlines.
        }
    }
}

fn line_chars(lines: &Lines, line: usize) -> Vec<char> {
    let (s, e) = lines.range(line);
    let end_with_nl = (e + 1).min(lines.chars.len());
    lines.chars[s..end_with_nl].to_vec()
}

/// Toggle `@done` on one line. Present (any value): the tag is
/// removed entirely, including the single space before it. Absent:
/// ` @done` or ` @done(stamp)` is appended at the end of the line.
pub fn toggle_done(text: &str, line: usize, stamp: Option<&str>) -> Option<String> {
    let lines = Lines::new(text);
    if line >= lines.line_count() {
        return None;
    }
    let target = lines.line(line);
    let bytes = target.as_bytes();
    let mut found: Option<(usize, usize)> = None; // (tag_start, tag_end) bytes
    for i in 0..bytes.len() {
        if bytes[i] == b'@' && (i == 0 || bytes[i - 1] == b' ' || bytes[i - 1] == b'\t') {
            let rest = &target[i + 1..];
            if !rest.starts_with("done") {
                continue;
            }
            let after = &rest[4..];
            // A longer word ("donely") is not the tag.
            if after
                .chars()
                .next()
                .is_some_and(|c| c.is_alphanumeric() || c == '-' || c == '_')
            {
                continue;
            }
            let mut end = i + 5;
            if after.starts_with('(')
                && let Some(close) = after.find(')')
            {
                end = i + 5 + close + 1;
            }
            found = Some((i, end));
            break;
        }
    }
    let new_line = match found {
        Some((at, end)) => {
            let start = if at > 0 && (bytes[at - 1] == b' ' || bytes[at - 1] == b'\t') {
                at - 1
            } else {
                at
            };
            format!("{}{}", &target[..start], &target[end..])
        }
        None => {
            let stamp_part = stamp.map(|s| format!("({s})")).unwrap_or_default();
            let joiner = if target.is_empty() || target.ends_with(' ') || target.ends_with('\t') {
                ""
            } else {
                " "
            };
            format!("{target}{joiner}@done{stamp_part}")
        }
    };
    let (s, e) = lines.range(line);
    let mut chars = lines.chars.clone();
    let new_chars: Vec<char> = new_line.chars().collect();
    chars.splice(s..e, new_chars);
    Some(chars.iter().collect())
}

/// Duplicate a block of lines directly below itself.
pub fn duplicate_block(text: &str, first_line: usize, last_line: usize) -> Option<String> {
    let mut lines = Lines::new(text);
    if first_line > last_line || last_line >= lines.line_count() {
        return None;
    }
    let (_, e_last) = lines.range(last_line);
    let insert_at = (e_last + 1).min(lines.chars.len());
    let (s, _) = lines.range(first_line);
    let block: Vec<char> = lines.chars[s..insert_at].to_vec();
    lines.chars.splice(insert_at..insert_at, block);
    Some(lines.join())
}

/// Group a block under a fresh empty parent: the parent lands as the
/// previous sibling of the first line, at the first line's depth,
/// and the block shifts one level deeper. Returns the new text and
/// the parent line's index.
pub fn group_lines(text: &str, first_line: usize, last_line: usize) -> Option<(String, usize)> {
    let mut lines = Lines::new(text);
    if first_line > last_line || last_line >= lines.line_count() {
        return None;
    }
    let depth = lines.depth(first_line);
    let at = lines.range(first_line).0;
    let parent: Vec<char> = format!("{}\n", tabs(depth)).chars().collect();
    lines.chars.splice(at..at, parent);
    Some((
        shift_lines(&lines.join(), first_line + 1, last_line + 1, 1, 0)?,
        first_line,
    ))
}

/// Prepare pasted text for insertion at a depth: normalize line
/// endings, drop the trailing newline, and shift the whole block so
/// its first line lands at `target_depth`.
pub fn prepare_paste(clipboard: &str, target_depth: i64) -> String {
    let normalized = clipboard.replace("\r\n", "\n").replace('\r', "\n");
    let trimmed = normalized.strip_suffix('\n').unwrap_or(&normalized);
    if trimmed.is_empty() {
        return String::new();
    }
    let count = trimmed.split('\n').count();
    let first_depth = trimmed
        .split('\n')
        .next()
        .map(|l| l.chars().take_while(|&c| c == '\t').count() as i64)
        .unwrap_or(0);
    let delta = target_depth - first_depth;
    if delta == 0 {
        return trimmed.to_string();
    }
    shift_lines(trimmed, 0, count - 1, delta, 0).unwrap_or_else(|| trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "Project:\n\t- one\n\t- two @a\n\t- three\nNote line\n";

    fn pos(text: &str, needle: &str, extra: usize) -> usize {
        let chars: Vec<char> = text.chars().collect();
        let pat: Vec<char> = needle.chars().collect();
        for start in 0..chars.len() {
            if chars[start..].starts_with(&pat) {
                return start + extra;
            }
        }
        panic!("needle {needle:?} not found");
    }

    #[test]
    fn enter_after_a_task_continues_the_list() {
        let caret = pos(DOC, "- one", 5);
        let r = smart_enter(DOC, caret, true, Some(1)).unwrap();
        assert_eq!(
            r.text,
            "Project:\n\t- one\n\t- \n\t- two @a\n\t- three\nNote line\n"
        );
        assert_eq!(r.caret, r.text.find("\t- \n").unwrap() + 3);
    }

    #[test]
    fn enter_at_end_of_project_inserts_a_child_task() {
        let caret = pos(DOC, "Project:", 8);
        let r = smart_enter(DOC, caret, true, Some(1)).unwrap();
        // A project continues with an empty NOTE, not a task.
        assert!(r.text.contains("Project:\n\t\n\t- one"));
    }

    #[test]
    fn enter_at_end_of_a_note_inserts_a_note() {
        let caret = pos(DOC, "Note line", 9);
        let r = smart_enter(DOC, caret, true, Some(0)).unwrap();
        assert!(r.text.contains("Note line\n\n"));
    }

    #[test]
    fn enter_at_the_lead_inserts_an_empty_item_above() {
        let caret = pos(DOC, "- one", 0);
        let r = smart_enter(DOC, caret, true, None).unwrap();
        assert!(r.text.contains("\n\t\n\t- one"));
        assert_eq!(r.caret, caret + 2);
    }

    #[test]
    fn enter_after_the_bare_dash_strips_the_marker() {
        let text = "P:\n\t- \n\t- two\n";
        let caret = pos(text, "- ", 2);
        let r = smart_enter(text, caret, true, None).unwrap();
        assert_eq!(r.text, "P:\n\t\n\t- two\n");
        // The caret parks at the end of the emptied item.
        assert_eq!(r.caret, 4);
    }

    #[test]
    fn enter_splits_mid_content_with_tags_traveling() {
        let caret = pos(DOC, "- two @a", 5); // after "two"
        let r = smart_enter(DOC, caret, true, Some(1)).unwrap();
        assert!(r.text.contains("\t- two\n\t-  @a\n"));
    }

    #[test]
    fn shift_lines_moves_and_floors() {
        assert_eq!(
            shift_lines(DOC, 1, 2, 1, 0).unwrap(),
            "Project:\n\t\t- one\n\t\t- two @a\n\t- three\nNote line\n"
        );
        assert!(shift_lines(DOC, 1, 1, -1, 1).is_none());
    }

    #[test]
    fn toggle_done_adds_removes_and_replaces() {
        assert_eq!(toggle_done("\t- one", 0, None).unwrap(), "\t- one @done");
        assert_eq!(toggle_done("\t- one @done", 0, None).unwrap(), "\t- one");
        assert_eq!(
            toggle_done("\t- one", 0, Some("2026-10-12")).unwrap(),
            "\t- one @done(2026-10-12)"
        );
        // Present means remove, whatever the old value was; the
        // fresh stamp only applies when the tag is being set (the
        // all-have-else-set rule for selections lives in the app).
        assert_eq!(
            toggle_done("\t- one @done @a", 0, None).unwrap(),
            "\t- one @a"
        );
        assert_eq!(
            toggle_done("\t- donely", 0, Some("2026-10-12")).unwrap(),
            "\t- donely @done(2026-10-12)"
        );
    }

    #[test]
    fn duplicate_block_copies_below() {
        assert_eq!(
            duplicate_block(DOC, 1, 2).unwrap(),
            "Project:\n\t- one\n\t- two @a\n\t- one\n\t- two @a\n\t- three\nNote line\n"
        );
    }

    #[test]
    fn group_inserts_parent_and_shifts() {
        let (text, parent_line) = group_lines(DOC, 1, 2).unwrap();
        assert_eq!(parent_line, 1);
        assert_eq!(
            text,
            "Project:\n\t\n\t\t- one\n\t\t- two @a\n\t- three\nNote line\n"
        );
    }

    #[test]
    fn paste_shifts_to_the_target_depth() {
        assert_eq!(prepare_paste("- a\n\t- b\n", 1), "\t- a\n\t\t- b");
        assert_eq!(prepare_paste("\t- a\n", 0), "- a");
        assert_eq!(prepare_paste("- a\n- b\n", 0), "- a\n- b");
        assert_eq!(prepare_paste("\n", 0), "");
    }

    #[test]
    fn move_block_splices_without_duplicates() {
        let r = move_block(DOC, 3, 3, 1).unwrap();
        assert_eq!(r, "Project:\n\t- three\n\t- one\n\t- two @a\nNote line\n");
        let r = move_block(DOC, 1, 2, 5).unwrap();
        assert_eq!(r, "Project:\n\t- three\nNote line\n\t- one\n\t- two @a\n");
    }

    #[test]
    fn subtree_and_branch_bounds() {
        let nested = "A:\n\tB:\n\t\t- x\n\t- y\n";
        assert_eq!(subtree_end(nested, 0), 3);
        assert_eq!(subtree_end(nested, 1), 2);
        assert_eq!(subtree_end(nested, 2), 2);
        assert_eq!(branch_start(nested, 2), 0);
    }
}
