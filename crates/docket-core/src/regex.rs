//! A small backtracking regex engine, stdlib-only, for the query
//! language's `matches` relation.
//!
//! Supported subset: literals, `.`, character classes `[...]` with
//! ranges and negation, the class shorthands `\d \D \w \W \s \S`,
//! escaped punctuation, anchors `^` and `$`, groups `(...)` (also
//! `(?:...)`), alternation `|`, and the quantifiers `* + ?` and
//! `{m}` `{m,}` `{m,n}` with optional lazy `?`. No captures, no
//! lookaround, no `\b`. An unsupported or malformed pattern is a
//! construction error; the query layer turns that into "matches
//! nothing" per the spec's failure semantics.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegexError;

#[derive(Debug, Clone)]
enum Ast {
    Char(char),
    Any,
    Class {
        neg: bool,
        items: Vec<ClassItem>,
    },
    Start,
    End,
    Concat(Vec<Ast>),
    Alt(Vec<Ast>),
    Repeat {
        node: Box<Ast>,
        min: u32,
        max: Option<u32>,
        lazy: bool,
    },
}

#[derive(Debug, Clone)]
enum ClassItem {
    Char(char),
    Range(char, char),
    Digit,
    NotDigit,
    Word,
    NotWord,
    Space,
    NotSpace,
}

pub struct Regex {
    ast: Ast,
}

/// Backtracking is exponential in the worst case, and queries run
/// live on the UI thread, so every search carries a step budget.
/// Exhausting it fails the whole search: a pathological pattern
/// degrades to "matched nothing" instead of hanging the app.
const STEP_BUDGET: u64 = 1_000_000;

struct Parser<'a> {
    chars: &'a [char],
    pos: usize,
}

impl Regex {
    pub fn new(pattern: &str) -> Result<Regex, RegexError> {
        let chars: Vec<char> = pattern.chars().collect();
        let mut p = Parser {
            chars: &chars,
            pos: 0,
        };
        let ast = p.parse_alt()?;
        if p.pos != p.chars.len() {
            return Err(RegexError);
        }
        Ok(Regex { ast })
    }

    /// Unanchored search: does the pattern match anywhere? A search
    /// that exhausts the step budget fails closed.
    pub fn is_match(&self, text: &str) -> bool {
        let chars: Vec<char> = text.chars().collect();
        let budget = std::cell::Cell::new(STEP_BUDGET);
        let matched =
            (0..=chars.len()).any(|start| match_node(&self.ast, &chars, start, &|_| true, &budget));
        matched && budget.get() > 0
    }
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }
    fn bump(&mut self) -> Option<char> {
        let c = self.peek();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }
    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn parse_alt(&mut self) -> Result<Ast, RegexError> {
        let mut branches = vec![self.parse_concat()?];
        while self.eat('|') {
            branches.push(self.parse_concat()?);
        }
        Ok(if branches.len() == 1 {
            branches.pop().unwrap()
        } else {
            Ast::Alt(branches)
        })
    }

    fn parse_concat(&mut self) -> Result<Ast, RegexError> {
        let mut nodes = Vec::new();
        while let Some(c) = self.peek() {
            if c == '|' || c == ')' {
                break;
            }
            let atom = self.parse_atom()?;
            nodes.push(self.parse_quantifier(atom)?);
        }
        Ok(match nodes.len() {
            0 => Ast::Concat(Vec::new()),
            1 => nodes.pop().unwrap(),
            _ => Ast::Concat(nodes),
        })
    }

    fn parse_quantifier(&mut self, atom: Ast) -> Result<Ast, RegexError> {
        let (min, max) = match self.peek() {
            Some('*') => {
                self.pos += 1;
                (0, None)
            }
            Some('+') => {
                self.pos += 1;
                (1, None)
            }
            Some('?') => {
                self.pos += 1;
                (0, Some(1))
            }
            Some('{') => {
                let save = self.pos;
                self.pos += 1;
                match self.parse_braces() {
                    Some(bounds) => bounds,
                    None if self.pos > save + 1 && self.chars.contains(&'}') => {
                        return Err(RegexError);
                    }
                    None => {
                        // A literal `{` when it is not a valid quantifier.
                        self.pos = save;
                        return Ok(atom);
                    }
                }
            }
            _ => return Ok(atom),
        };
        let lazy = self.eat('?');
        Ok(Ast::Repeat {
            node: Box::new(atom),
            min,
            max,
            lazy,
        })
    }

    fn parse_braces(&mut self) -> Option<(u32, Option<u32>)> {
        let min = self.parse_number()?;
        let max = if self.eat(',') {
            if self.peek() == Some('}') {
                None
            } else {
                Some(self.parse_number()?)
            }
        } else {
            Some(min)
        };
        if !self.eat('}') {
            return None;
        }
        if max.is_some_and(|m| m < min) {
            // Malformed bounds: consume through `}` and signal the
            // quantifier branch to error.
            while self.pos < self.chars.len() && self.chars[self.pos] != '}' {
                self.pos += 1;
            }
            self.pos += 1;
            return None;
        }
        Some((min, max))
    }

    fn parse_number(&mut self) -> Option<u32> {
        let start = self.pos;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.pos += 1;
        }
        if start == self.pos {
            return None;
        }
        self.chars[start..self.pos]
            .iter()
            .collect::<String>()
            .parse()
            .ok()
    }

    fn parse_atom(&mut self) -> Result<Ast, RegexError> {
        match self.bump() {
            None => Err(RegexError),
            Some('(') => {
                // (?:...) is accepted; captures are not tracked.
                if self.chars[self.pos..].starts_with(&['?', ':']) {
                    self.pos += 2;
                }
                let inner = self.parse_alt()?;
                if !self.eat(')') {
                    return Err(RegexError);
                }
                Ok(inner)
            }
            Some('[') => self.parse_class(),
            Some('^') => Ok(Ast::Start),
            Some('$') => Ok(Ast::End),
            Some('.') => Ok(Ast::Any),
            Some('*') | Some('+') | Some('?') => Err(RegexError),
            Some('\\') => {
                let c = self.bump().ok_or(RegexError)?;
                Ok(match c {
                    'd' => class_of(ClassItem::Digit, false),
                    'D' => class_of(ClassItem::NotDigit, true),
                    'w' => class_of(ClassItem::Word, false),
                    'W' => class_of(ClassItem::NotWord, true),
                    's' => class_of(ClassItem::Space, false),
                    'S' => class_of(ClassItem::NotSpace, true),
                    'n' => Ast::Char('\n'),
                    'r' => Ast::Char('\r'),
                    't' => Ast::Char('\t'),
                    other => Ast::Char(other),
                })
            }
            Some(c) => Ok(Ast::Char(c)),
        }
    }

    fn parse_class(&mut self) -> Result<Ast, RegexError> {
        let neg = self.eat('^');
        let mut items = Vec::new();
        loop {
            let c = self.bump().ok_or(RegexError)?;
            if c == ']' && !items.is_empty() {
                break;
            }
            let item = match c {
                '\\' => {
                    let e = self.bump().ok_or(RegexError)?;
                    match e {
                        'd' => ClassItem::Digit,
                        'D' => ClassItem::NotDigit,
                        'w' => ClassItem::Word,
                        'W' => ClassItem::NotWord,
                        's' => ClassItem::Space,
                        'S' => ClassItem::NotSpace,
                        'n' => ClassItem::Char('\n'),
                        'r' => ClassItem::Char('\r'),
                        't' => ClassItem::Char('\t'),
                        other => ClassItem::Char(other),
                    }
                }
                plain => {
                    // Range? a-z, but `-` at the ends is literal.
                    if self.peek() == Some('-')
                        && self.chars.get(self.pos + 1).is_some_and(|&n| n != ']')
                    {
                        self.pos += 1;
                        let hi = self.bump().ok_or(RegexError)?;
                        ClassItem::Range(plain, hi)
                    } else {
                        ClassItem::Char(plain)
                    }
                }
            };
            items.push(item);
        }
        Ok(Ast::Class { neg, items })
    }
}

fn class_of(item: ClassItem, neg: bool) -> Ast {
    Ast::Class {
        neg,
        items: vec![item],
    }
}

fn class_has(class: &Ast, c: char) -> bool {
    let Ast::Class { neg, items } = class else {
        return false;
    };
    let hit = items.iter().any(|item| match item {
        ClassItem::Char(x) => *x == c,
        ClassItem::Range(lo, hi) => *lo <= c && c <= *hi,
        ClassItem::Digit => c.is_ascii_digit(),
        ClassItem::NotDigit => !c.is_ascii_digit(),
        ClassItem::Word => c.is_alphanumeric() || c == '_',
        ClassItem::NotWord => !(c.is_alphanumeric() || c == '_'),
        ClassItem::Space => c.is_whitespace(),
        ClassItem::NotSpace => !c.is_whitespace(),
    });
    hit != *neg
}

fn match_node(
    node: &Ast,
    chars: &[char],
    pos: usize,
    k: &dyn Fn(usize) -> bool,
    budget: &std::cell::Cell<u64>,
) -> bool {
    // Charge one step per node visit; an empty budget prunes the
    // whole search branch.
    let left = budget.get();
    if left == 0 {
        return false;
    }
    budget.set(left - 1);
    match node {
        Ast::Char(c) => chars.get(pos) == Some(c) && k(pos + 1),
        Ast::Any => pos < chars.len() && chars[pos] != '\n' && k(pos + 1),
        Ast::Class { .. } => pos < chars.len() && class_has(node, chars[pos]) && k(pos + 1),
        Ast::Start => pos == 0 && k(pos),
        Ast::End => pos == chars.len() && k(pos),
        Ast::Concat(nodes) => match_seq(nodes, chars, pos, k, budget),
        Ast::Alt(branches) => branches
            .iter()
            .any(|b| match_node(b, chars, pos, k, budget)),
        Ast::Repeat {
            node,
            min,
            max,
            lazy,
        } => rep(node, chars, pos, 0, *min, *max, *lazy, k, budget),
    }
}

fn match_seq(
    nodes: &[Ast],
    chars: &[char],
    pos: usize,
    k: &dyn Fn(usize) -> bool,
    budget: &std::cell::Cell<u64>,
) -> bool {
    match nodes.split_first() {
        None => k(pos),
        Some((first, rest)) => match_node(
            first,
            chars,
            pos,
            &|p| match_seq(rest, chars, p, k, budget),
            budget,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn rep(
    node: &Ast,
    chars: &[char],
    pos: usize,
    count: u32,
    min: u32,
    max: Option<u32>,
    lazy: bool,
    k: &dyn Fn(usize) -> bool,
    budget: &std::cell::Cell<u64>,
) -> bool {
    let can_more = max.is_none_or(|m| count < m);
    let more = || -> bool {
        can_more
            && match_node(
                node,
                chars,
                pos,
                &|p| {
                    // Zero-width progress guard: stop a repetition
                    // that matched nothing, once the minimum is met.
                    p != pos && rep(node, chars, p, count + 1, min, max, lazy, k, budget)
                },
                budget,
            )
    };
    if count < min {
        return more();
    }
    match lazy {
        true => k(pos) || more(),
        false => more() || k(pos),
    }
}

#[cfg(test)]
mod tests {
    use super::Regex;

    fn m(pattern: &str, text: &str) -> bool {
        Regex::new(pattern).unwrap().is_match(text)
    }

    #[test]
    fn literals_and_anchors() {
        assert!(m("abc", "xxabcyy"));
        assert!(!m("abc", "abx"));
        assert!(m("^abc$", "abc"));
        assert!(!m("^abc$", "xabcy"));
        assert!(m("^Work", "Work: stuff"));
    }

    #[test]
    fn dot_star_plus_question() {
        assert!(m("a.c", "abc"));
        assert!(!m("a.c", "ac"));
        assert!(m("ab*c", "ac"));
        assert!(m("ab+c", "abbc"));
        assert!(!m("ab+c", "ac"));
        assert!(m("colou?r", "color"));
    }

    #[test]
    fn braces() {
        assert!(m("a{3}", "aaa"));
        assert!(!m("a{3}", "aa"));
        assert!(m("a{2,}", "aaaa"));
        assert!(m("a{2,3}", "aa"));
        assert!(!m("^a{2,3}$", "a"));
        // A brace that is not a quantifier is a literal.
        assert!(m("a{,}", "a{,}"));
    }

    #[test]
    fn classes() {
        assert!(m("[b-z]ay", "hay"));
        assert!(!m("[^b-z]ay", "hay"));
        assert!(m("[a-c]x", "bx"));
        assert!(m("\\d+", "abc123"));
        assert!(m("\\d\\d-\\d\\d", "due 10-14!"));
        assert!(m("[\\d\\s]", "a 1"));
        assert!(m("[]]", "]"));
    }

    #[test]
    fn alternation_and_groups() {
        assert!(m("cat|dog", "hotdog"));
        assert!(m("^(cat|dog)s?$", "dogs"));
        assert!(m("(?:ab)+", "ababab"));
    }

    #[test]
    fn lazy_quantifiers_still_match() {
        assert!(m("<.+?>", "<a><b>"));
        assert!(m("a+?b", "aaab"));
    }

    #[test]
    fn invalid_patterns_are_errors() {
        assert!(Regex::new("(").is_err());
        assert!(Regex::new("*x").is_err());
        assert!(Regex::new("[a").is_err());
        assert!(Regex::new("a{2,1}").is_err());
    }

    #[test]
    fn pathological_patterns_fail_closed_instead_of_hanging() {
        // Classic exponential blowups: a failing tail forces
        // backtracking through every split; the budget ends it fast.
        assert!(!m("(a|a)*b", &"a".repeat(40)));
        assert!(!m("(a+)+$", &format!("{}!", "a".repeat(40))));
        // Ordinary patterns still match normally.
        assert!(m("a+b", &format!("{}b", "a".repeat(40))));
    }

    #[test]
    fn empty_pattern_matches_everything() {
        assert!(m("", "anything"));
        assert!(m("", ""));
    }
}
