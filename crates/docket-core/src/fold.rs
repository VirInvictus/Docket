//! Fold target selection and focus anchors: which header lines each
//! fold command touches, and which project line a focus command
//! anchors to. Pure text logic; the app maps lines to its
//! content-hash fold state.

/// The item's indentation depth in tabs.
fn depth_of(line: &str) -> usize {
    line.chars().take_while(|&c| c == '\t').count()
}

/// Every header line (an item whose next line nests deeper) as
/// (line, depth), in document order.
pub fn header_lines(text: &str) -> Vec<(usize, usize)> {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out = Vec::new();
    for i in 0..lines.len().saturating_sub(1) {
        if lines[i].trim_start_matches('\t').is_empty() {
            continue;
        }
        if depth_of(lines[i + 1]) > depth_of(lines[i]) {
            out.push((i, depth_of(lines[i])));
        }
    }
    out
}

/// Every header line at exactly `depth`: the By Level targets.
pub fn header_lines_at_level(text: &str, depth: usize) -> Vec<usize> {
    header_lines(text)
        .into_iter()
        .filter(|&(_, d)| d == depth)
        .map(|(l, _)| l)
        .collect()
}

/// Every header line inside the subtree rooted at `line`, including
/// `line` itself when it is a header: the Completely targets within
/// a branch.
pub fn subtree_header_lines(text: &str, line: usize) -> Vec<usize> {
    let end = crate::edit::subtree_end(text, line);
    header_lines(text)
        .into_iter()
        .filter(|&(l, _)| l >= line && l <= end)
        .map(|(l, _)| l)
        .collect()
}

/// Every header line in the document: the All Completely targets.
pub fn all_header_lines(text: &str) -> Vec<usize> {
    header_lines(text).into_iter().map(|(l, _)| l).collect()
}

/// The nearest enclosing project line for `line`: the line itself
/// when it is a project, else the first strictly shallower line
/// above that classifies as a project, following the ancestor chain.
/// None when the chain carries no project (a task under tasks).
pub fn enclosing_project(text: &str, line: usize) -> Option<usize> {
    use crate::classify::kind;
    use crate::item::ItemKind;
    let lines: Vec<&str> = text.split('\n').collect();
    let cur = lines.get(line)?;
    if kind(cur.trim_start_matches('\t')) == ItemKind::Project {
        return Some(line);
    }
    let mut depth = depth_of(cur);
    for i in (0..line).rev() {
        let d = depth_of(lines[i]);
        if d >= depth {
            continue;
        }
        // The first strictly shallower line above is the parent.
        if kind(lines[i].trim_start_matches('\t')) == ItemKind::Project {
            return Some(i);
        }
        depth = d;
    }
    None
}

/// The project strictly enclosing the project at `line`, following
/// the ancestor chain above it: the Focus Out target. None at top
/// level.
pub fn parent_project(text: &str, line: usize) -> Option<usize> {
    use crate::classify::kind;
    use crate::item::ItemKind;
    let lines: Vec<&str> = text.split('\n').collect();
    if lines.get(line).is_none() || line == 0 {
        return None;
    }
    let mut depth = depth_of(lines[line]);
    for i in (0..line).rev() {
        let d = depth_of(lines[i]);
        if d >= depth {
            continue;
        }
        if kind(lines[i].trim_start_matches('\t')) == ItemKind::Project {
            return Some(i);
        }
        depth = d;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "Home:\n\tErrands:\n\t\t- milk\n\t\t- call dentist @done\n\tWork:\n\t\t- ship release\n\t\t\t- write specs\n\tNotes on the week:\n\t\tThe garden.\n";

    #[test]
    fn header_lines_find_parents() {
        // Headers: Home(0,d0), Errands(1,d1), Work(4,d1),
        // ship release(5,d2), Notes(7,d1).
        let hs = header_lines(DOC);
        assert_eq!(hs, vec![(0, 0), (1, 1), (4, 1), (5, 2), (7, 1)]);
        assert_eq!(all_header_lines(DOC), vec![0, 1, 4, 5, 7]);
        assert_eq!(header_lines_at_level(DOC, 1), vec![1, 4, 7]);
        assert_eq!(header_lines_at_level(DOC, 0), vec![0]);
        assert_eq!(header_lines_at_level(DOC, 3), Vec::<usize>::new());
    }

    #[test]
    fn subtree_headers_stay_in_the_branch() {
        // Errands' subtree covers lines 1..3: itself only (the tasks
        // are leaves).
        assert_eq!(subtree_header_lines(DOC, 1), vec![1]);
        // Work's subtree includes ship release's nested header.
        assert_eq!(subtree_header_lines(DOC, 4), vec![4, 5]);
        // Home's subtree covers the whole top-level project.
        assert_eq!(subtree_header_lines(DOC, 0), vec![0, 1, 4, 5, 7]);
    }

    #[test]
    fn enclosing_project_walks_the_chain() {
        assert_eq!(enclosing_project(DOC, 0), Some(0));
        // milk (line 2) sits under Errands (project) under Home.
        assert_eq!(enclosing_project(DOC, 2), Some(1));
        // ship release (line 5) is a task; its parent Work is a
        // project.
        assert_eq!(enclosing_project(DOC, 5), Some(4));
        // write specs (line 6) ascends through the task to Work.
        assert_eq!(enclosing_project(DOC, 6), Some(4));
        // The garden note ascends through Notes on the week to Home.
        assert_eq!(enclosing_project(DOC, 8), Some(7));
    }

    #[test]
    fn enclosing_project_is_none_without_a_project_ancestor() {
        let text = "- task:\n\t\t- nested\n";
        assert_eq!(enclosing_project(text, 1), None);
    }

    #[test]
    fn parent_project_climbs_one_level() {
        assert_eq!(parent_project(DOC, 1), Some(0));
        assert_eq!(parent_project(DOC, 4), Some(0));
        // Home has no parent.
        assert_eq!(parent_project(DOC, 0), None);
    }
}

#[cfg(test)]
mod path_tests {
    #[test]
    fn a_bare_path_query_matches_everything() {
        // The focus visibility runs a match-all query scoped to the
        // focused project; the bare descendant-or-self path is it.
        use crate::{Document, Query};
        let doc = Document::load("Home:\n\t- a\n\t\t- b\nElse:\n\t- c\n");
        let q = Query::parse("//").expect("// parses");
        assert_eq!(q.run(&doc), vec![0, 1, 2, 3, 4]);
        let q2 = Query::parse("//*").expect("//* parses");
        assert_eq!(q2.run(&doc), vec![0, 1, 2, 3, 4]);
    }
}
