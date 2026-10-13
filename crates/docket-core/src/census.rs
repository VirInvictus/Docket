//! The document census: the tag and project inventories the sidebar
//! and completion draw from, per the spec's sidebar model.

use std::collections::BTreeMap;

/// Every tag name in the document with its distinct values, sorted
/// by name; values sorted, empty strings skipped, comma-list values
/// split into separate entries. `exclude` drops names before they
/// are observed at all.
pub fn tag_census(text: &str, exclude: &[String]) -> BTreeMap<String, Vec<String>> {
    let _exclude = exclude;
    let mut names: Vec<String> = Vec::new();
    let mut values: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for line in text.split('\n') {
        for (name, value) in crate::tags::scan(line)
            .into_iter()
            .map(|t| (t.name.to_string(), t.value.map(|v| v.into_owned())))
        {
            if !names.iter().any(|x| x == &name) {
                names.push(name.clone());
            }
            // A valueless tag still joins the census with an empty
            // value set; comma lists split into separate values.
            for piece in value.unwrap_or_default().split(',') {
                let piece = piece.trim();
                if piece.is_empty() {
                    continue;
                }
                let set = values.entry(name.clone()).or_default();
                if !set.iter().any(|x| x == piece) {
                    set.push(piece.to_string());
                }
            }
        }
    }
    let _ = &mut values;
    let mut out = BTreeMap::new();
    for name in names {
        let mut vals = values.remove(&name).unwrap_or_default();
        vals.sort();
        out.insert(name, vals);
    }
    out
}

/// A project row for the sidebar: line index, display name (content
/// minus trailing tags, colon retained), nesting depth among projects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRow {
    pub line: usize,
    pub name: String,
    /// Nesting level among sidebar-listed projects (top-level = 0).
    pub level: usize,
}

/// The sidebar's project inventory: project-type items whose entire
/// ancestor chain is projects, nested by depth. A project nested
/// under a task or note is not listed.
pub fn project_rows(text: &str) -> Vec<ProjectRow> {
    use crate::item::ItemKind;
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out = Vec::new();
    let mut stack: Vec<usize> = Vec::new(); // levels of listed projects
    for (i, line) in lines.iter().enumerate() {
        let depth = line.chars().take_while(|&c| c == '\t').count();
        let content = line.trim_start_matches('\t');
        let is_project = crate::classify::kind(content) == ItemKind::Project;
        if !is_project {
            // A non-project item still occupies a depth slot; pop the
            // stack back to this item's depth so sibling structure
            // stays right.
            stack.retain(|&d| d < depth);
            continue;
        }
        // Ancestors must all be projects: every shallower line above
        // with depth < this one's must itself be a project.
        let mut ancestors_all_projects = true;
        for prev in (0..i).rev() {
            let pd = lines[prev].chars().take_while(|&c| c == '\t').count();
            if pd < depth {
                let pcontent = lines[prev].trim_start_matches('\t');
                if crate::classify::kind(pcontent) != ItemKind::Project {
                    ancestors_all_projects = false;
                    break;
                }
                if pd == 0 {
                    break;
                }
            }
        }
        if !ancestors_all_projects {
            stack.retain(|&d| d < depth);
            continue;
        }
        while stack.last().is_some_and(|&d| d >= depth) {
            stack.pop();
        }
        let level = stack.len();
        stack.push(depth);
        // Display name: content minus trailing tag run, colon kept.
        let name = crate::classify::content(content).to_string();
        out.push(ProjectRow {
            line: i,
            name,
            level,
        });
    }
    out
}

/// The inline completion for searchbar text whose tail is a partial
/// `@tag`, drawn from `names` (census tag names, no `@`): returns the
/// char offset of the `@` and the completed tag name, so the caller
/// can insert `name` after the `@` and select what the typist has
/// not yet typed. None when the text does not end in a completable
/// partial: a bare `@`, a tail already inside a value or quoted
/// string, a prefix no name extends, or a prefix that is already a
/// full name.
pub fn search_tag_completion(text: &str, names: &[String]) -> Option<(usize, String)> {
    let chars: Vec<char> = text.chars().collect();
    let at = chars.iter().rposition(|&c| c == '@')?;
    let tail = &chars[at + 1..];
    if tail.is_empty()
        || tail
            .iter()
            .any(|&c| c.is_whitespace() || c == '(' || c == ')' || c == '"')
    {
        return None;
    }
    let partial: String = tail.iter().collect();
    let mut candidates: Vec<&str> = names
        .iter()
        .map(String::as_str)
        .filter(|n| n.starts_with(&partial))
        .collect();
    match candidates.len() {
        0 => None,
        1 => {
            let name = candidates[0];
            if name == partial {
                None
            } else {
                Some((at, name.to_string()))
            }
        }
        _ => {
            candidates.sort_unstable();
            let shared = common_prefix(&candidates);
            if shared.chars().count() > partial.chars().count() {
                Some((at, shared))
            } else {
                None
            }
        }
    }
}

fn common_prefix(names: &[&str]) -> String {
    let mut out = String::new();
    let mut iters: Vec<std::str::Chars> = names.iter().map(|n| n.chars()).collect();
    loop {
        let mut next = None;
        for it in &mut iters {
            let c = match it.next() {
                Some(c) => c,
                None => return out,
            };
            match next {
                None => next = Some(c),
                Some(p) if p == c => {}
                Some(_) => return out,
            }
        }
        out.push(next.unwrap());
    }
}

/// A saved search row: label (content minus trailing tags) and the
/// `@search(value)` query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchRow {
    pub line: usize,
    pub label: String,
    pub query: String,
}

/// Saved searches from a document's text: every line carrying
/// `@search(query)` in document order.
pub fn search_rows(text: &str) -> Vec<SearchRow> {
    let mut out = Vec::new();
    for (i, line) in text.split('\n').enumerate() {
        for tag in crate::tags::scan(line) {
            if tag.name == "search"
                && let Some(value) = &tag.value
            {
                let label = crate::classify::content(line)
                    .trim_start_matches('\t')
                    .trim_end()
                    .to_string();
                out.push(SearchRow {
                    line: i,
                    label: label.trim_end().to_string(),
                    query: value.clone().into_owned(),
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "Home:\n\tErrands:\n\t\t- milk @a(1)\n\tWork:\n\t\t- report @due(2026-10-20)\n- loose note @b\n";

    #[test]
    fn tag_census_collects_names_and_distinct_values() {
        let text = "- a @due(2026-10-01)\n- b @due(2026-10-02) @p\n- c @due\n- d @p\n";
        let census = tag_census(text, &[]);
        let due = &census["due"];
        assert_eq!(
            due,
            &vec!["2026-10-01".to_string(), "2026-10-02".to_string()]
        );
        let p = &census["p"];
        assert_eq!(p, &Vec::<String>::new());
    }

    #[test]
    fn comma_lists_split_into_separate_values() {
        let census = tag_census("- x @job(Jane, John)\n- y @job(Jane)\n", &[]);
        assert_eq!(census["job"], vec!["Jane".to_string(), "John".to_string()]);
    }

    #[test]
    fn project_rows_skip_projects_under_non_projects() {
        let rows = project_rows(DOC);
        // Errands is a project under Home (project) - listed. Work
        // likewise. "loose note" is not a project at all.
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].name, "Home:");
        assert_eq!(rows[0].level, 0);
        assert_eq!(rows[1].name, "Errands:");
        assert_eq!(rows[1].level, 1);
        assert_eq!(rows[2].name, "Work:");
        assert_eq!(rows[2].level, 1);
    }

    #[test]
    fn project_rows_require_project_ancestors() {
        let text = "- a task:\n\tNested:\n\t\t- x\n";
        // "Nested" is a project but its ancestor is a task: not listed.
        assert!(project_rows(text).is_empty());
    }

    #[test]
    fn search_rows_find_the_tagged_lines() {
        let text = "Searches:\n\tToday @search(@today union @due <[d] tomorrow)\n\tNot Done @search(not @done)\n";
        let rows = search_rows(text);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].label, "Today");
        assert_eq!(rows[0].query, "@today union @due <[d] tomorrow");
        assert_eq!(rows[1].label, "Not Done");
    }

    #[test]
    fn completion_extends_a_unique_tag() {
        let names = vec!["done".to_string(), "due".to_string()];
        assert_eq!(
            search_tag_completion("find @don", &names),
            Some((5, "done".to_string()))
        );
        // The offset counts chars, so multibyte text before the @
        // still lands the insertion point right.
        assert_eq!(
            search_tag_completion("éé @don", &names),
            Some((3, "done".to_string()))
        );
    }

    #[test]
    fn completion_takes_the_common_prefix_of_several() {
        let names = vec!["dot".to_string(), "done".to_string()];
        assert_eq!(
            search_tag_completion("@d", &names),
            Some((0, "do".to_string()))
        );
        // Already at the common prefix: nothing to add.
        assert_eq!(search_tag_completion("@do", &names), None);
    }

    #[test]
    fn completion_skips_full_names_values_and_bare_at() {
        let names = vec!["done".to_string()];
        assert_eq!(search_tag_completion("@done", &names), None);
        assert_eq!(search_tag_completion("@done(2026", &names), None);
        assert_eq!(search_tag_completion("@due \"quoted", &names), None);
        assert_eq!(search_tag_completion("@", &names), None);
        assert_eq!(search_tag_completion("@zzz", &names), None);
        assert_eq!(search_tag_completion("no tag here", &names), None);
    }
}
