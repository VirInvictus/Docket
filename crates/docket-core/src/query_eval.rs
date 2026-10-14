//! Query evaluation over a document.
//!
//! Results are item indices in document order. The failure
//! semantics are the spec's: a missing or unparseable value makes a
//! comparison false rather than erroring, an invalid regex matches
//! nothing, and an unparseable query is a parse-time error that the
//! caller reports while showing no results. Evaluation itself
//! cannot fail: every axis in the grammar is implemented.

use crate::dates;
use crate::document::Document;
use crate::item::ItemKind;
use crate::query::{AttrRef, Axis, Cmp, Node, Pred, Query, Rel, Slice, ValPart};

/// The running selection a step evaluates against. `Root` is the
/// whole document (or, when scoped, the focused subtree); a
/// count() path enters at its own item.
#[derive(Debug, Clone)]
enum Sel {
    Root,
    Items(Vec<usize>),
}

impl Query {
    /// Run the query against the whole document; results are item
    /// indices in document order.
    pub fn run(&self, doc: &Document) -> Vec<usize> {
        self.run_scoped_at(doc, None, dates::now())
    }

    /// Run with an explicit reference instant for relative-date
    /// keywords and durations; the refresh command passes a fresh
    /// one, tests pass fixed ones.
    pub fn run_at(&self, doc: &Document, now: i64) -> Vec<usize> {
        self.run_scoped_at(doc, None, now)
    }

    /// Run inside a focus or hoist scope: relative paths anchor at
    /// the focused item's subtree (the focused row included),
    /// absolute paths anchor at the document root. `None` means no
    /// scope: the whole document.
    pub fn run_scoped_at(&self, doc: &Document, focus: Option<usize>, now: i64) -> Vec<usize> {
        let mut items = eval_node(self.root(), doc, Sel::Root, focus, now);
        items.sort_unstable();
        items.dedup();
        items
    }
}

fn eval_node(node: &Node, doc: &Document, sel: Sel, focus: Option<usize>, now: i64) -> Vec<usize> {
    match node {
        Node::Path { steps, absolute } => {
            // A relative path in a scope anchors at the focused
            // item; an absolute path always starts at the root.
            let entry = match (sel, focus) {
                (Sel::Root, Some(f)) if !*absolute => {
                    Sel::Items(vec![f.min(doc.items.len().saturating_sub(1))])
                }
                (sel, _) => sel,
            };
            // Inside a scope the focused row itself belongs to the
            // universe, so the implicit first axis is
            // descendant-or-self.
            let scoped = focus.is_some();
            eval_steps(steps, doc, entry, *absolute, scoped, now)
        }
        Node::Union(l, r) => {
            let mut a = eval_node(l, doc, sel.clone(), focus, now);
            a.extend(eval_node(r, doc, sel, focus, now));
            a.sort_unstable();
            a.dedup();
            a
        }
        Node::Except(l, r) => {
            let a = eval_node(l, doc, sel.clone(), focus, now);
            let b = eval_node(r, doc, sel, focus, now);
            a.into_iter().filter(|i| !b.contains(i)).collect()
        }
        Node::Intersect(l, r) => {
            let a = eval_node(l, doc, sel.clone(), focus, now);
            let b = eval_node(r, doc, sel, focus, now);
            a.into_iter().filter(|i| b.contains(i)).collect()
        }
        Node::Group(inner, slice) => {
            let items = eval_node(inner, doc, sel, focus, now);
            match slice {
                Some(s) => apply_slice(items, s),
                None => items,
            }
        }
    }
}

fn eval_steps(
    steps: &[crate::query::Step],
    doc: &Document,
    sel: Sel,
    absolute: bool,
    scoped: bool,
    now: i64,
) -> Vec<usize> {
    let mut cur = sel;
    let mut result: Option<Vec<usize>> = None;
    for (n, step) in steps.iter().enumerate() {
        let axis = step.axis.unwrap_or(if n == 0 && scoped && !absolute {
            // The focused row belongs to the scoped universe.
            Axis::DescendantOrSelf
        } else if n == 0 {
            Axis::Descendant
        } else {
            Axis::Child
        });
        let mut selected = match &cur {
            Sel::Root => apply_axis(axis, Sel::Root, doc),
            Sel::Items(items) => {
                let mut all = Vec::new();
                for &i in items {
                    all.extend(apply_axis(axis, Sel::Items(vec![i]), doc));
                }
                all.sort_unstable();
                all.dedup();
                all
            }
        };
        if let Some(ty) = step.ty {
            selected.retain(|&i| doc.items[i].kind() == ty);
        }
        if let Some(pred) = &step.pred {
            selected.retain(|&i| eval_pred(pred, i, doc, now));
        }
        if let Some(slice) = &step.slice {
            selected = apply_slice(selected, slice);
        }
        cur = Sel::Items(selected.clone());
        result = Some(selected);
    }
    result.unwrap_or_default()
}

fn apply_axis(axis: Axis, sel: Sel, doc: &Document) -> Vec<usize> {
    match (axis, sel) {
        (Axis::Descendant | Axis::DescendantOrSelf, Sel::Root) => (0..doc.items.len()).collect(),
        (Axis::Child, Sel::Root) => (0..doc.items.len())
            .filter(|&i| doc.items[i].depth == 0)
            .collect(),
        (Axis::SelfAxis | Axis::Parent | Axis::Ancestor, Sel::Root) => Vec::new(),
        (Axis::AncestorOrSelf, Sel::Root) => (0..doc.items.len()).collect(),
        (Axis::Following | Axis::Preceding, Sel::Root) => Vec::new(),
        (Axis::FollowingSibling | Axis::PrecedingSibling, Sel::Root) => Vec::new(),
        (Axis::Descendant, Sel::Items(items)) => {
            items.into_iter().flat_map(|i| subtree(doc, i)).collect()
        }
        (Axis::DescendantOrSelf, Sel::Items(items)) => {
            let mut out = items.clone();
            out.extend(items.into_iter().flat_map(|i| subtree(doc, i)));
            out
        }
        (Axis::Child, Sel::Items(items)) => {
            items.into_iter().flat_map(|i| children(doc, i)).collect()
        }
        (Axis::SelfAxis, Sel::Items(items)) => items,
        (Axis::Parent, Sel::Items(items)) => {
            items.into_iter().flat_map(|i| doc.parent_of(i)).collect()
        }
        (Axis::Ancestor, Sel::Items(items)) => items
            .into_iter()
            .flat_map(|i| {
                let mut out = Vec::new();
                let mut cur = doc.parent_of(i);
                while let Some(a) = cur {
                    out.push(a);
                    cur = doc.parent_of(a);
                }
                out
            })
            .collect(),
        (Axis::AncestorOrSelf, Sel::Items(items)) => items
            .into_iter()
            .flat_map(|i| {
                let mut out = vec![i];
                let mut cur = doc.parent_of(i);
                while let Some(a) = cur {
                    out.push(a);
                    cur = doc.parent_of(a);
                }
                out
            })
            .collect(),
        (Axis::Following, Sel::Items(items)) => items
            .into_iter()
            .flat_map(|i| {
                // Everything after the item's subtree closes.
                let end = subtree(doc, i).last().map_or(i + 1, |&last| last + 1);
                (end..doc.items.len()).collect::<Vec<_>>()
            })
            .collect(),
        (Axis::Preceding, Sel::Items(items)) => {
            // Everything before the item, ancestors excluded.
            items
                .into_iter()
                .flat_map(|i| {
                    let mut ancestors = Vec::new();
                    let mut cur = doc.parent_of(i);
                    while let Some(a) = cur {
                        ancestors.push(a);
                        cur = doc.parent_of(a);
                    }
                    (0..i)
                        .filter(|j| !ancestors.contains(j))
                        .collect::<Vec<_>>()
                })
                .collect()
        }
        (Axis::FollowingSibling, Sel::Items(items)) => items
            .into_iter()
            .flat_map(|i| match doc.parent_of(i) {
                Some(p) => children(doc, p).into_iter().filter(|&c| c > i).collect(),
                None => Vec::new(),
            })
            .collect(),
        (Axis::PrecedingSibling, Sel::Items(items)) => items
            .into_iter()
            .flat_map(|i| match doc.parent_of(i) {
                Some(p) => children(doc, p).into_iter().filter(|&c| c < i).collect(),
                None => Vec::new(),
            })
            .collect(),
    }
}

/// All items strictly inside item `i`'s subtree, in document order.
fn subtree(doc: &Document, i: usize) -> Vec<usize> {
    let depth = doc.items[i].depth;
    let mut out = Vec::new();
    for (j, item) in doc.items.iter().enumerate().skip(i + 1) {
        if item.depth <= depth {
            break;
        }
        out.push(j);
    }
    out
}

/// Direct children of item `i`.
fn children(doc: &Document, i: usize) -> Vec<usize> {
    let depth = doc.items[i].depth;
    let target = depth + 1;
    let mut out = Vec::new();
    let mut j = i + 1;
    while j < doc.items.len() {
        let d = doc.items[j].depth;
        if d <= depth {
            break;
        }
        if d == target {
            out.push(j);
            // Skip the child's whole subtree before seeking the next.
            j = subtree(doc, j).last().map_or(j + 1, |&last| last + 1);
        } else {
            j += 1;
        }
    }
    out
}

fn eval_pred(pred: &Pred, idx: usize, doc: &Document, now: i64) -> bool {
    match pred {
        Pred::Group(inner) => eval_pred(inner, idx, doc, now),
        Pred::And(l, r) => eval_pred(l, idx, doc, now) && eval_pred(r, idx, doc, now),
        Pred::Or(l, r) => eval_pred(l, idx, doc, now) || eval_pred(r, idx, doc, now),
        Pred::Not(inner) => !eval_pred(inner, idx, doc, now),
        Pred::Cmp(cmp) => eval_cmp(cmp, idx, doc, now),
    }
}

/// Resolve a built-in or tag attribute on an item. `@text` is the
/// full body, `@type` the kind word, `@id` the runtime index; any
/// other name is a tag attribute (first occurrence wins; the
/// reserved names never resolve as tags).
fn resolve_attr(idx: usize, name: &str, doc: &Document) -> Option<String> {
    let body = &doc.items[idx].body;
    match name {
        "text" => Some(body.clone()),
        "type" => Some(
            match doc.items[idx].kind() {
                ItemKind::Project => "project",
                ItemKind::Task => "task",
                ItemKind::Note => "note",
            }
            .to_string(),
        ),
        "id" => Some(idx.to_string()),
        other => crate::tags::attribute(body, other).map(|v| v.into_owned()),
    }
}

fn eval_cmp(cmp: &Cmp, idx: usize, doc: &Document, now: i64) -> bool {
    // Existence test: an attribute with no relation and no value.
    if cmp.rel.is_none() && cmp.value.is_empty() {
        return match &cmp.attr {
            None => true,
            Some(AttrRef::Name(n)) => resolve_attr(idx, n, doc).is_some(),
            Some(AttrRef::Count(_)) => true,
        };
    }

    let (rel, mods) = cmp
        .rel
        .as_ref()
        .map_or((Rel::Contains, ""), |(r, m)| (*r, m.as_str()));

    let left = match &cmp.attr {
        None => Some(doc.items[idx].body.clone()),
        Some(AttrRef::Name(n)) => resolve_attr(idx, n, doc),
        Some(AttrRef::Count(path)) => {
            let n = eval_node(path, doc, Sel::Items(vec![idx]), None, now).len();
            Some(n.to_string())
        }
    };
    // A missing value on either side makes the comparison false.
    let Some(left) = left else {
        return false;
    };
    let Some(right) = resolve_value(&cmp.value, idx, doc, now) else {
        return false;
    };

    apply_rel(left, right, rel, mods, now)
}

fn resolve_value(parts: &[ValPart], idx: usize, doc: &Document, now: i64) -> Option<String> {
    if parts.is_empty() {
        return Some(String::new());
    }
    let mut out = Vec::with_capacity(parts.len());
    for part in parts {
        match part {
            ValPart::Str(s) => out.push(s.clone()),
            ValPart::Attr(name) => out.push(resolve_attr(idx, name, doc)?),
            ValPart::Count(path) => {
                let n = eval_node(path, doc, Sel::Items(vec![idx]), None, now).len();
                out.push(n.to_string());
            }
        }
    }
    Some(out.join(" "))
}

#[derive(Debug, Clone)]
enum Coerced {
    Str(String),
    Num(f64),
    Date(i64),
}

fn coerce(s: &str, mods: &str, now: i64) -> Coerced {
    if mods.contains('d') {
        // An unparseable date compares false: NaN does exactly that.
        return match dates::parse(s, now) {
            Some(d) => Coerced::Date(d.0),
            None => Coerced::Num(f64::NAN),
        };
    }
    if mods.contains('n') {
        return Coerced::Num(js_parse_float(s));
    }
    if mods.contains('s') {
        return Coerced::Str(s.to_string());
    }
    Coerced::Str(s.to_lowercase())
}

/// JavaScript-style `parseFloat`: the leading numeric prefix, or NaN.
fn js_parse_float(s: &str) -> f64 {
    let t = s.trim_start();
    let b = t.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let int_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let frac_start = i;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i == int_start && i == frac_start {
        return f64::NAN;
    }
    let mut end = i;
    if end < b.len() && (b[end] == b'e' || b[end] == b'E') {
        let mut j = end + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        let ds = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j > ds {
            end = j;
        }
    }
    t[..end].parse().unwrap_or(f64::NAN)
}

fn apply_rel(left: String, right: String, rel: Rel, mods: &str, now: i64) -> bool {
    // `like` parses and never matches: the documented quirk.
    if rel == Rel::Like {
        return false;
    }
    if mods.contains('l') {
        // List compare: both sides split on commas and normalize by
        // the case modifier; contains becomes membership, equality
        // becomes set equality.
        let norm = |x: &str| {
            if mods.contains('s') {
                x.to_string()
            } else {
                x.to_lowercase()
            }
        };
        let ls: Vec<String> = left.split(',').map(norm).collect();
        let rs: Vec<String> = right.split(',').map(norm).collect();
        let without_l: String = mods.chars().filter(|c| *c != 'l').collect();
        let mut a: Vec<&str> = ls.iter().map(String::as_str).collect();
        let mut b: Vec<&str> = rs.iter().map(String::as_str).collect();
        a.sort_unstable();
        b.sort_unstable();
        return match rel {
            Rel::Eq => a == b,
            Rel::Ne => a != b,
            Rel::Contains => rs.iter().any(|r| ls.iter().any(|l| l == r)),
            Rel::Beginswith => ls.iter().any(|l| rs.iter().any(|r| l.starts_with(r))),
            Rel::Endswith => ls.iter().any(|l| rs.iter().any(|r| l.ends_with(r))),
            Rel::Matches => ls
                .iter()
                .any(|l| rs.iter().any(|r| regex_match(l, r, &without_l))),
            Rel::Like => false,
            Rel::Lt | Rel::Le | Rel::Gt | Rel::Ge => ls.iter().any(|l| {
                rs.iter()
                    .any(|r| apply_rel(l.clone(), r.clone(), rel, &without_l, now))
            }),
        };
    }

    match (coerce(&left, mods, now), coerce(&right, mods, now)) {
        (Coerced::Num(a), Coerced::Num(b)) => match rel {
            Rel::Eq => a == b,
            Rel::Ne => a != b,
            Rel::Lt => a < b,
            Rel::Le => a <= b,
            Rel::Gt => a > b,
            Rel::Ge => a >= b,
            // The string relations do not apply to numbers.
            _ => false,
        },
        (Coerced::Date(a), Coerced::Date(b)) => match rel {
            Rel::Eq => a == b,
            Rel::Ne => a != b,
            Rel::Lt => a < b,
            Rel::Le => a <= b,
            Rel::Gt => a > b,
            Rel::Ge => a >= b,
            _ => false,
        },
        // A date or number that failed to parse compares false.
        (Coerced::Num(_), _) | (_, Coerced::Num(_)) => false,
        (Coerced::Date(_), _) | (_, Coerced::Date(_)) => false,
        (Coerced::Str(a), Coerced::Str(b)) => match rel {
            Rel::Eq => a == b,
            Rel::Ne => a != b,
            Rel::Lt => a < b,
            Rel::Le => a <= b,
            Rel::Gt => a > b,
            Rel::Ge => a >= b,
            Rel::Contains => a.contains(&b),
            Rel::Beginswith => a.starts_with(&b),
            Rel::Endswith => a.ends_with(&b),
            Rel::Matches => regex_match(&a, &b, mods),
            Rel::Like => false,
        },
    }
}

/// `matches` with the failure semantics: an invalid pattern matches
/// nothing. Case handling rides the default `[i]` coercion.
fn regex_match(text: &str, pattern: &str, mods: &str) -> bool {
    let (t, p) = if mods.contains('s') {
        (text.to_string(), pattern.to_string())
    } else {
        (text.to_lowercase(), pattern.to_lowercase())
    };
    crate::regex::Regex::new(&p)
        .ok()
        .is_some_and(|re| re.is_match(&t))
}

fn apply_slice(items: Vec<usize>, slice: &Slice) -> Vec<usize> {
    let n = items.len() as i64;
    match slice {
        Slice::Single(k) => {
            if n == 0 {
                return Vec::new();
            }
            let i = if *k < 0 {
                (n + *k).max(0)
            } else {
                (*k).min(n - 1)
            };
            vec![items[i as usize]]
        }
        Slice::Range { start, end } => {
            let s = match start.unwrap_or(0).clamp(-n, n) {
                v if v < 0 => v + n,
                v => v,
            };
            let e = match end.unwrap_or(n).clamp(-n, n) {
                v if v < 0 => v + n,
                v => v,
            };
            let (s, e) = (s.clamp(0, n), e.clamp(0, n));
            if e <= s {
                Vec::new()
            } else {
                items
                    .into_iter()
                    .skip(s as usize)
                    .take((e - s) as usize)
                    .collect()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::document::Document;
    use crate::query::Query;

    fn doc(text: &str) -> Document {
        Document::load(text)
    }

    fn run(q: &str, d: &Document) -> Vec<usize> {
        Query::parse(q).unwrap().run(d)
    }

    #[test]
    fn defaults_apply() {
        let d = doc("Project:\n\t- buy socks\n\tnote about SOCKS\n");
        assert_eq!(run("socks", &d), vec![1, 2]);
        assert_eq!(run("@text contains socks", &d), vec![1, 2]);
        assert_eq!(run("contains[s] socks", &d), vec![1]);
    }

    #[test]
    fn missing_values_compare_false() {
        let d = doc("- a @due(2026-10-14)\n- b @nope\n");
        assert_eq!(run("@due", &d), vec![0]);
        assert_eq!(run("@due contains 2026", &d), vec![0]);
        assert_eq!(run("@due <[d] 2027", &d), vec![0]);
        assert_eq!(run("@nope <[d] 2027", &d), Vec::<usize>::new());
    }

    #[test]
    fn numeric_modifier_parses_prefixes_and_zero_padding() {
        let d = doc("- a @est(01)\n- b @est(2x)\n- c @est(x)\n");
        assert_eq!(run("@est =[n] 1", &d), vec![0]);
        assert_eq!(run("@est =[n] 2", &d), vec![1]);
        // "x" has no numeric prefix: NaN compares false, even to zero.
        assert_eq!(run("@est =[n] 0", &d), Vec::<usize>::new());
    }

    #[test]
    fn list_modifier_splits_commas() {
        let d = doc("- a @job(Jane,John)\n- b @job(Johny)\n");
        assert_eq!(run("@job contains[l] John", &d), vec![0]);
        assert_eq!(run("@job contains John", &d), vec![0, 1]);
    }

    #[test]
    fn like_never_matches() {
        let d = doc("- engine work\n");
        assert_eq!(run("@text like engine", &d), Vec::<usize>::new());
    }

    #[test]
    fn matches_uses_regex_and_fails_closed() {
        let d = doc("- abc123\n- nope\n");
        assert_eq!(run("@text matches \"a.c\\d+\"", &d), vec![0]);
        assert_eq!(run("@text matches \"[\"", &d), Vec::<usize>::new());
    }

    #[test]
    fn type_and_builtin_attributes() {
        let d = doc("P:\n\t- t\n\tnote\n");
        assert_eq!(run("project", &d), vec![0]);
        assert_eq!(run("@type = task", &d), vec![1]);
        assert_eq!(run("@id = 2", &d), vec![2]);
    }

    #[test]
    fn slicing_clamps_and_counts_from_the_end() {
        let d = doc("- a\n- b\n- c\n");
        assert_eq!(run("task[300]", &d), vec![2]);
        assert_eq!(run("task[-300]", &d), vec![0]);
        assert_eq!(run("task[0:1]", &d), vec![0]);
        assert_eq!(run("task[0:-3]", &d), Vec::<usize>::new());
        assert_eq!(run("task[2:]", &d), vec![2]);
        assert_eq!(run("task[:2]", &d), vec![0, 1]);
    }

    #[test]
    fn set_operations() {
        let d = doc("P:\n\t- a @today\n\t- b @due(2026-10-31)\n\t- c @done\n");
        assert_eq!(run("@today union @due", &d), vec![1, 2]);
        assert_eq!(run("* except task", &d), vec![0]);
        assert_eq!(run("@today intersect @due", &d), Vec::<usize>::new());
    }

    #[test]
    fn count_relative_to_the_item() {
        let d = doc("P:\n\t- only child\nQ:\n\t- a\n\t- b\n");
        assert_eq!(run("count(.*) = 1", &d), vec![0]);
        assert_eq!(run("count(*) = 0", &d), vec![1, 3, 4]);
    }

    #[test]
    fn attribute_references_as_values() {
        let d = doc(
            "- a @start(2026-01-01) @due(2026-01-01)\n- b @start(2026-01-01) @due(2026-02-02)\n",
        );
        assert_eq!(run("@start = @due", &d), vec![0]);
    }

    #[test]
    fn multi_step_paths_chain_the_grammar_axes() {
        // / is descendant, so P's slash-chain reaches the deep item
        // through C without naming it.
        let d = doc("P:\n\tC:\n\t\t- deep\n\t- flat\n");
        assert_eq!(run("P/*//*", &d), vec![1, 2, 3]);
        // "." is the child separator: P's children matching "flat".
        assert_eq!(run("P.child::flat", &d), vec![3]);
        assert_eq!(run("P.child::*", &d), vec![1, 3]);
        // "/." mid-path is a descendant step then a child step.
        assert_eq!(run("P/.", &d), vec![2]);
    }

    #[test]
    fn exotic_axes() {
        let d = doc("A:\n\t- one\n\t- two\nB:\n\t- three\nC:\n");
        // items: 0 A, 1 one, 2 two, 3 B, 4 three, 5 C
        assert_eq!(run("two/ancestor::*", &d), vec![0]);
        assert_eq!(run("two/ancestor-or-self::*", &d), vec![0, 2]);
        assert_eq!(run("one/following-sibling::*", &d), vec![2]);
        assert_eq!(run("two/preceding-sibling::*", &d), vec![1]);
        // following: everything after two's subtree closes.
        assert_eq!(run("two/following::*", &d), vec![3, 4, 5]);
        // preceding: everything before two, ancestors excluded.
        assert_eq!(run("two/preceding::*", &d), vec![1]);
        assert_eq!(run("three/ancestor::*", &d), vec![3]);
    }

    #[test]
    fn scope_anchors_relative_paths_at_the_focus() {
        let d = doc("A:\n\t- a @x\nB:\n\t- b @x\n");
        // Unscoped: both @x items.
        assert_eq!(run("@x", &d), vec![1, 3]);
        // Scoped at A: the universe is A's subtree including the
        // focused row; only the item actually carrying @x matches.
        assert_eq!(
            Query::parse("@x").unwrap().run_scoped_at(&d, Some(0), 0),
            vec![1]
        );
        // Scoped at B likewise.
        assert_eq!(
            Query::parse("@x").unwrap().run_scoped_at(&d, Some(3), 0),
            vec![3]
        );
        // Absolute paths ignore the scope.
        assert_eq!(
            Query::parse("//*").unwrap().run_scoped_at(&d, Some(0), 0),
            (0..4).collect::<Vec<_>>()
        );
    }
}
