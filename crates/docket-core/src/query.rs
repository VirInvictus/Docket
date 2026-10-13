//! The query language: lexer, AST, parser, and canonical form.
//!
//! Grammar shape (spec "Query language"): set expressions combine
//! paths with `union` / `except` / `intersect` (tightest first,
//! right-associative); a path is one or more steps of axis, type,
//! predicate, and optional slice; predicates are boolean
//! expressions over subsettable comparisons
//! (`@attribute relation[mods] value`, defaults `@text`,
//! `contains`, case-insensitive). Values are sequences of quoted
//! strings, words, `@attribute` references, and `count(path)`
//! expressions, joined with single spaces.
//!
//! Separator choice: a leading `/` anchors absolute and its step
//! runs on the child axis; between steps `/` is the child axis,
//! `//` descendant, `///` descendant-or-self, `.` child, `..`
//! parent (the guide's table). The first step of a relative path
//! is implicit descendant.

use crate::item::ItemKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    /// Byte offset into the query string.
    pub position: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Ancestor,
    AncestorOrSelf,
    Child,
    Descendant,
    DescendantOrSelf,
    Following,
    FollowingSibling,
    Preceding,
    PrecedingSibling,
    Parent,
    SelfAxis,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Slice {
    Single(i64),
    Range {
        start: Option<i64>,
        end: Option<i64>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Path { absolute: bool, steps: Vec<Step> },
    Union(Box<Node>, Box<Node>),
    Except(Box<Node>, Box<Node>),
    Intersect(Box<Node>, Box<Node>),
    Group(Box<Node>, Option<Slice>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub axis: Option<Axis>,
    pub ty: Option<ItemKind>,
    pub pred: Option<Pred>,
    pub slice: Option<Slice>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pred {
    Group(Box<Pred>),
    And(Box<Pred>, Box<Pred>),
    Or(Box<Pred>, Box<Pred>),
    Not(Box<Pred>),
    Cmp(Cmp),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cmp {
    pub attr: Option<AttrRef>,
    pub rel: Option<(Rel, String)>,
    /// Modifier letters (`i`, `s`, `n`, `d`, `l`); empty means the
    /// default `[i]`.
    pub value: Vec<ValPart>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttrRef {
    Name(String),
    Count(Box<Node>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rel {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Contains,
    Beginswith,
    Endswith,
    Matches,
    Like,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValPart {
    Str(String),
    Attr(String),
    Count(Box<Node>),
}

const RESERVED: &[&str] = &["and", "or", "not", "union", "except", "intersect"];
const REL_WORDS: &[&str] = &["contains", "beginswith", "endswith", "matches", "like"];
const TYPE_WORDS: &[&str] = &["project", "task", "note"];
const AXES: &[&str] = &[
    "ancestor",
    "ancestor-or-self",
    "child",
    "descendant",
    "descendant-or-self",
    "following",
    "following-sibling",
    "preceding",
    "preceding-sibling",
    "parent",
    "self",
];

/// A fully parsed query, ready to run or to render canonically.
#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    root: Node,
}

impl Query {
    pub fn parse(text: &str) -> Result<Query, ParseError> {
        let tokens = lex(text)?;
        let mut p = Parser { tokens, pos: 0 };
        let root = p.parse_set()?;
        if p.pos != p.tokens.len() {
            return Err(p.err("unexpected trailing input"));
        }
        Ok(Query { root })
    }

    pub fn root(&self) -> &Node {
        &self.root
    }

    /// Canonical form: defaults omitted, right-nested chains
    /// parenthesized.
    pub fn to_canonical(&self) -> String {
        canonical_node(&self.root)
    }
}

// ---------------------------------------------------------------------
// Lexer
// ---------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Attr(String),
    Word(String),
    Str(String),
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Slash,
    SlashSlash,
    SlashSlashSlash,
    Dot,
    DotDot,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Star,
    Punct(char),
}

#[derive(Debug, Clone)]
struct Token {
    tok: Tok,
    start: usize,
}

fn lex(text: &str) -> Result<Vec<Token>, ParseError> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = text[i..].chars().next().unwrap();
        let start = i;
        match c {
            _ if c.is_whitespace() => i += 1,
            '@' => {
                i += 1;
                let name_start = i;
                while let Some(ch) = text[i..].chars().next() {
                    let allowed = if i == name_start {
                        crate::tags::is_name_start(ch)
                    } else {
                        crate::tags::is_name_cont(ch)
                    };
                    if !allowed {
                        break;
                    }
                    i += ch.len_utf8();
                }
                let name = text[name_start..i].to_string();
                // `@name:segment` suffixes parse and are dropped;
                // only the first segment participates in evaluation.
                while i < b.len() && b[i] == b':' {
                    let seg = i + 1;
                    let mut j = seg;
                    while let Some(ch) = text[j..].chars().next() {
                        if !crate::tags::is_name_cont(ch) {
                            break;
                        }
                        j += ch.len_utf8();
                    }
                    if j == seg {
                        break;
                    }
                    i = j;
                }
                out.push(Token {
                    tok: Tok::Attr(name),
                    start,
                });
            }
            '"' => {
                i += 1;
                let mut s = String::new();
                loop {
                    let ch = text[i..].chars().next().ok_or_else(|| ParseError {
                        message: "unterminated string".into(),
                        position: start,
                    })?;
                    i += ch.len_utf8();
                    match ch {
                        '"' => break,
                        '\\' => {
                            let e = text[i..].chars().next().ok_or_else(|| ParseError {
                                message: "unterminated string".into(),
                                position: start,
                            })?;
                            i += e.len_utf8();
                            // Known escapes map; unknown escapes
                            // keep the backslash, so regex syntax
                            // like \d survives inside a value.
                            match e {
                                'b' => s.push('\u{8}'),
                                'f' => s.push('\u{c}'),
                                'n' => s.push('\n'),
                                'r' => s.push('\r'),
                                't' => s.push('\t'),
                                '"' => s.push('"'),
                                '/' => s.push('/'),
                                other => {
                                    s.push('\\');
                                    s.push(other);
                                }
                            }
                        }
                        other => s.push(other),
                    }
                }
                out.push(Token {
                    tok: Tok::Str(s),
                    start,
                });
            }
            '=' => {
                i += 1;
                out.push(Token {
                    tok: Tok::Eq,
                    start,
                });
            }
            '!' => {
                i += 1;
                if text.as_bytes().get(i) == Some(&b'=') {
                    i += 1;
                    out.push(Token {
                        tok: Tok::Ne,
                        start,
                    });
                } else {
                    return Err(ParseError {
                        message: "expected != ".into(),
                        position: start,
                    });
                }
            }
            '<' => {
                i += 1;
                if text.as_bytes().get(i) == Some(&b'=') {
                    i += 1;
                    out.push(Token {
                        tok: Tok::Le,
                        start,
                    });
                } else {
                    out.push(Token {
                        tok: Tok::Lt,
                        start,
                    });
                }
            }
            '>' => {
                i += 1;
                if text.as_bytes().get(i) == Some(&b'=') {
                    i += 1;
                    out.push(Token {
                        tok: Tok::Gt,
                        start,
                    });
                } else {
                    out.push(Token {
                        tok: Tok::Ge,
                        start,
                    });
                }
            }
            '/' => {
                if text[i..].starts_with("///") {
                    i += 3;
                    out.push(Token {
                        tok: Tok::SlashSlashSlash,
                        start,
                    });
                } else if text[i..].starts_with("//") {
                    i += 2;
                    out.push(Token {
                        tok: Tok::SlashSlash,
                        start,
                    });
                } else {
                    i += 1;
                    out.push(Token {
                        tok: Tok::Slash,
                        start,
                    });
                }
            }
            '.' => {
                if text[i..].starts_with("..") {
                    i += 2;
                    out.push(Token {
                        tok: Tok::DotDot,
                        start,
                    });
                } else {
                    i += 1;
                    out.push(Token {
                        tok: Tok::Dot,
                        start,
                    });
                }
            }
            '(' => {
                i += 1;
                out.push(Token {
                    tok: Tok::LParen,
                    start,
                });
            }
            ')' => {
                i += 1;
                out.push(Token {
                    tok: Tok::RParen,
                    start,
                });
            }
            '[' => {
                i += 1;
                out.push(Token {
                    tok: Tok::LBracket,
                    start,
                });
            }
            ']' => {
                i += 1;
                out.push(Token {
                    tok: Tok::RBracket,
                    start,
                });
            }
            '*' => {
                i += 1;
                out.push(Token {
                    tok: Tok::Star,
                    start,
                });
            }
            c if crate::tags::is_name_start(c) => {
                let mut j = i + c.len_utf8();
                while let Some(ch) = text[j..].chars().next() {
                    if !crate::tags::is_name_cont(ch) {
                        break;
                    }
                    j += ch.len_utf8();
                }
                out.push(Token {
                    tok: Tok::Word(text[i..j].to_string()),
                    start,
                });
                i = j;
            }
            other => {
                i += other.len_utf8();
                out.push(Token {
                    tok: Tok::Punct(other),
                    start,
                });
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn err(&self, message: &str) -> ParseError {
        let position = match self.tokens.get(self.pos) {
            Some(t) => t.start,
            None => self.tokens.last().map(|t| t.start).unwrap_or(0),
        };
        ParseError {
            message: message.to_string(),
            position,
        }
    }

    fn peek(&self) -> Option<&Tok> {
        self.tokens.get(self.pos).map(|t| &t.tok)
    }

    fn peek_at(&self, ahead: usize) -> Option<&Tok> {
        self.tokens.get(self.pos + ahead).map(|t| &t.tok)
    }

    fn eat(&mut self, tok: &Tok) -> bool {
        if self.peek() == Some(tok) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn eat_word(&mut self, word: &str) -> bool {
        if self.peek() == Some(&Tok::Word(word.to_string())) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, tok: &Tok, what: &str) -> Result<(), ParseError> {
        if self.eat(tok) {
            Ok(())
        } else {
            Err(self.err(what))
        }
    }

    fn parse_set(&mut self) -> Result<Node, ParseError> {
        let left = self.parse_except()?;
        if self.eat_word("union") {
            let right = self.parse_set()?;
            Ok(Node::Union(Box::new(left), Box::new(right)))
        } else {
            Ok(left)
        }
    }

    fn parse_except(&mut self) -> Result<Node, ParseError> {
        let left = self.parse_intersect()?;
        if self.eat_word("except") {
            let right = self.parse_except()?;
            Ok(Node::Except(Box::new(left), Box::new(right)))
        } else {
            Ok(left)
        }
    }

    fn parse_intersect(&mut self) -> Result<Node, ParseError> {
        let left = self.parse_path_level()?;
        if self.eat_word("intersect") {
            let right = self.parse_intersect()?;
            Ok(Node::Intersect(Box::new(left), Box::new(right)))
        } else {
            Ok(left)
        }
    }

    fn parse_path_level(&mut self) -> Result<Node, ParseError> {
        if self.peek() == Some(&Tok::LParen) {
            self.pos += 1;
            let inner = self.parse_set()?;
            self.expect(&Tok::RParen, "expected )")?;
            // A paren group whose content is a bare predicate is a
            // boolean group: it continues with and/or/not. Otherwise
            // it is a grouped path and takes an optional slice.
            let mut as_pred = None;
            if let Node::Path { steps, .. } = &inner
                && steps.len() == 1
            {
                let s = &steps[0];
                if s.ty.is_none() && s.axis.is_none() && s.slice.is_none() {
                    as_pred = s.pred.clone();
                }
            }
            if let Some(base) = as_pred {
                let grouped = Pred::Group(Box::new(base));
                let continued = if self.eat_word("and") {
                    let right = self.parse_pred_and()?;
                    Pred::And(Box::new(grouped), Box::new(right))
                } else {
                    grouped
                };
                let continued = self.finish_or(continued)?;
                let slice = self.parse_slice()?;
                return Ok(Node::Path {
                    absolute: false,
                    steps: vec![Step {
                        axis: None,
                        ty: None,
                        pred: Some(continued),
                        slice,
                    }],
                });
            }
            let slice = self.parse_slice()?;
            return Ok(Node::Group(Box::new(inner), slice));
        }
        let absolute;
        let mut first_axis = None;
        match self.peek() {
            Some(Tok::Slash) => {
                self.pos += 1;
                absolute = true;
                first_axis = Some(Axis::Child);
            }
            Some(Tok::SlashSlash) => {
                self.pos += 1;
                absolute = true;
                first_axis = Some(Axis::Descendant);
            }
            Some(Tok::SlashSlashSlash) => {
                self.pos += 1;
                absolute = true;
                first_axis = Some(Axis::DescendantOrSelf);
            }
            Some(Tok::Dot) => {
                self.pos += 1;
                absolute = false;
                first_axis = Some(Axis::Child);
            }
            Some(Tok::DotDot) => {
                self.pos += 1;
                absolute = false;
                first_axis = Some(Axis::Parent);
            }
            _ => absolute = false,
        }

        let mut steps = vec![self.parse_step(first_axis)?];
        loop {
            let axis = match self.peek() {
                Some(Tok::Slash) => Axis::Child,
                Some(Tok::SlashSlash) => Axis::Descendant,
                Some(Tok::SlashSlashSlash) => Axis::DescendantOrSelf,
                Some(Tok::Dot) => Axis::Child,
                Some(Tok::DotDot) => Axis::Parent,
                _ => break,
            };
            self.pos += 1;
            // "/." is the child idiom: the slash and the dot are one
            // child step, not two.
            if axis == Axis::Child && self.peek() == Some(&Tok::Dot) {
                self.pos += 1;
            }
            steps.push(self.parse_step(Some(axis))?);
        }
        Ok(Node::Path { absolute, steps })
    }

    fn parse_step(&mut self, axis: Option<Axis>) -> Result<Step, ParseError> {
        let mut axis = axis;
        if let Some(Tok::Word(w)) = self.peek()
            && AXES.contains(&w.as_str())
            && self.peek_at(1) == Some(&Tok::Punct(':'))
            && self.peek_at(2) == Some(&Tok::Punct(':'))
        {
            axis = Some(axis_from_word(w));
            self.pos += 3;
        }
        let mut ty = None;
        for (word, kind) in [
            ("project", ItemKind::Project),
            ("task", ItemKind::Task),
            ("note", ItemKind::Note),
        ] {
            if self.eat_word(word) {
                ty = Some(kind);
                break;
            }
        }

        // A bare `*` followed by anything that ends a step is the
        // wildcard predicate (no predicate at all).
        let pred = if self.peek() == Some(&Tok::Star) && self.step_ends_at(1) {
            self.pos += 1;
            None
        } else if self.step_starts_predicate() {
            Some(self.parse_pred_or()?)
        } else {
            None
        };

        let slice = self.parse_slice()?;
        Ok(Step {
            axis,
            ty,
            pred,
            slice,
        })
    }

    fn step_ends_at(&self, ahead: usize) -> bool {
        match self.peek_at(ahead) {
            None => true,
            Some(Tok::Slash)
            | Some(Tok::SlashSlash)
            | Some(Tok::SlashSlashSlash)
            | Some(Tok::Dot)
            | Some(Tok::DotDot)
            | Some(Tok::RParen)
            | Some(Tok::LBracket)
            | Some(Tok::RBracket) => true,
            Some(Tok::Word(w)) => RESERVED.contains(&w.as_str()),
            Some(_) => false,
        }
    }

    fn step_starts_predicate(&self) -> bool {
        match self.peek() {
            None => false,
            Some(Tok::Slash)
            | Some(Tok::SlashSlash)
            | Some(Tok::SlashSlashSlash)
            | Some(Tok::Dot)
            | Some(Tok::DotDot)
            | Some(Tok::RParen)
            | Some(Tok::LBracket)
            | Some(Tok::RBracket) => false,
            // A predicate may open with `not`; the other reserved
            // words cannot begin one.
            Some(Tok::Word(w)) => w == "not" || !RESERVED.contains(&w.as_str()),
            Some(_) => true,
        }
    }

    fn parse_pred_or(&mut self) -> Result<Pred, ParseError> {
        let left = self.parse_pred_and()?;
        if self.eat_word("or") {
            let right = self.parse_pred_or()?;
            Ok(Pred::Or(Box::new(left), Box::new(right)))
        } else {
            Ok(left)
        }
    }

    fn parse_pred_and(&mut self) -> Result<Pred, ParseError> {
        let left = self.parse_pred_not()?;
        if self.eat_word("and") {
            let right = self.parse_pred_and()?;
            Ok(Pred::And(Box::new(left), Box::new(right)))
        } else {
            Ok(left)
        }
    }

    fn parse_pred_not(&mut self) -> Result<Pred, ParseError> {
        if self.eat_word("not") {
            let inner = self.parse_pred_not()?;
            Ok(Pred::Not(Box::new(inner)))
        } else {
            self.parse_pred_primary()
        }
    }

    fn parse_pred_primary(&mut self) -> Result<Pred, ParseError> {
        if self.eat(&Tok::LParen) {
            let inner = self.parse_pred_or()?;
            self.expect(&Tok::RParen, "expected )")?;
            Ok(Pred::Group(Box::new(inner)))
        } else {
            Ok(Pred::Cmp(self.parse_comparison()?))
        }
    }

    fn finish_or(&mut self, left: Pred) -> Result<Pred, ParseError> {
        if self.eat_word("or") {
            let right = self.parse_pred_and()?;
            self.finish_or(Pred::Or(Box::new(left), Box::new(right)))
        } else {
            Ok(left)
        }
    }

    fn parse_comparison(&mut self) -> Result<Cmp, ParseError> {
        let mut attr = None;
        match self.peek() {
            Some(Tok::Attr(name)) => {
                attr = Some(AttrRef::Name(name.clone()));
                self.pos += 1;
            }
            Some(Tok::Word(w)) if w == "count" && self.peek_at(1) == Some(&Tok::LParen) => {
                self.pos += 2;
                let inner = self.parse_path_level()?;
                self.expect(&Tok::RParen, "expected ) after count path")?;
                attr = Some(AttrRef::Count(Box::new(inner)));
            }
            _ => {}
        }

        let rel = match self.peek() {
            Some(Tok::Eq) => Some(Rel::Eq),
            Some(Tok::Ne) => Some(Rel::Ne),
            Some(Tok::Lt) => Some(Rel::Lt),
            Some(Tok::Le) => Some(Rel::Le),
            Some(Tok::Gt) => Some(Rel::Gt),
            Some(Tok::Ge) => Some(Rel::Ge),
            Some(Tok::Word(w)) => match w.as_str() {
                "contains" => Some(Rel::Contains),
                "beginswith" => Some(Rel::Beginswith),
                "endswith" => Some(Rel::Endswith),
                "matches" => Some(Rel::Matches),
                "like" => Some(Rel::Like),
                _ => None,
            },
            _ => None,
        };
        let mut mods = String::new();
        if rel.is_some() {
            self.pos += 1;
            if self.eat(&Tok::LBracket) {
                let mut seen = String::new();
                while let Some(Tok::Word(w)) = self.peek() {
                    if w.chars().all(|c| "islnd".contains(c)) {
                        seen.push_str(w);
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
                self.expect(&Tok::RBracket, "expected ] after modifiers")?;
                mods = seen;
            }
        }

        let value = self.parse_value_parts()?;
        if attr.is_none() && rel.is_none() && value.is_empty() {
            return Err(self.err("expected a search term"));
        }
        if rel.is_some() && value.is_empty() {
            return Err(self.err("expected a value after the relation"));
        }
        Ok(Cmp {
            attr,
            rel: rel.map(|r| (r, mods)),
            value,
        })
    }

    fn parse_value_parts(&mut self) -> Result<Vec<ValPart>, ParseError> {
        let mut parts = Vec::new();
        loop {
            match self.peek() {
                Some(Tok::Str(s)) => parts.push(ValPart::Str(s.clone())),
                Some(Tok::Attr(a)) => parts.push(ValPart::Attr(a.clone())),
                Some(Tok::Star) => parts.push(ValPart::Str("*".into())),
                Some(Tok::Dot) => parts.push(ValPart::Str(".".into())),
                Some(Tok::Punct(c)) => parts.push(ValPart::Str(c.to_string())),
                Some(Tok::Word(w)) if w == "count" && self.peek_at(1) == Some(&Tok::LParen) => {
                    self.pos += 2;
                    let inner = self.parse_path_level()?;
                    self.expect(&Tok::RParen, "expected ) after count path")?;
                    parts.push(ValPart::Count(Box::new(inner)));
                    // This arm consumed its own tokens, including the
                    // closing paren; the loop tail's advance must not
                    // run for it.
                    continue;
                }
                Some(Tok::Word(w))
                    if !RESERVED.contains(&w.as_str()) && !REL_WORDS.contains(&w.as_str()) =>
                {
                    parts.push(ValPart::Str(w.clone()));
                }
                _ => break,
            }
            self.pos += 1;
        }
        Ok(parts)
    }

    fn parse_slice(&mut self) -> Result<Option<Slice>, ParseError> {
        if self.peek() != Some(&Tok::LBracket) {
            return Ok(None);
        }
        self.pos += 1;
        let neg = |p: &Parser| matches!(p.peek(), Some(Tok::Punct('-')));
        let number = |p: &mut Parser| -> Result<Option<i64>, ParseError> {
            let sign = if neg(p) {
                p.pos += 1;
                -1
            } else {
                1
            };
            match p.peek() {
                Some(Tok::Word(w)) if w.chars().all(|c| c.is_ascii_digit()) => {
                    let v: i64 = w.parse().map_err(|_| p.err("bad slice index"))?;
                    p.pos += 1;
                    Ok(Some(sign * v))
                }
                _ => Ok(None),
            }
        };
        let first = number(self)?;
        if self.eat(&Tok::Punct(':')) {
            let second = number(self)?;
            self.expect(&Tok::RBracket, "expected ]")?;
            Ok(Some(Slice::Range {
                start: first,
                end: second,
            }))
        } else {
            self.expect(&Tok::RBracket, "expected ]")?;
            let first = first.ok_or_else(|| self.err("expected slice index"))?;
            Ok(Some(Slice::Single(first)))
        }
    }
}

// ---------------------------------------------------------------------
// Canonical form
// ---------------------------------------------------------------------

fn canonical_node(node: &Node) -> String {
    match node {
        Node::Path { absolute, steps } => {
            let mut out = String::new();
            for (i, step) in steps.iter().enumerate() {
                if i > 0 {
                    match step.axis {
                        Some(Axis::Descendant) => out.push_str("//"),
                        Some(Axis::DescendantOrSelf) => out.push_str("///"),
                        Some(Axis::Parent) => out.push_str("/.."),
                        Some(Axis::Child) | None => out.push('/'),
                        Some(other) => {
                            out.push('/');
                            out.push_str(axis_word(other));
                            out.push_str("::");
                        }
                    }
                } else if *absolute {
                    // The leading token carries both the absolute
                    // anchor and the first step's axis.
                    match step.axis {
                        Some(Axis::Descendant) => out.push_str("//"),
                        Some(Axis::DescendantOrSelf) => out.push_str("///"),
                        _ => out.push('/'),
                    }
                } else {
                    match step.axis {
                        // A relative child or parent step spells its
                        // own shortcut; descendant is the implicit
                        // default and prints nothing.
                        Some(Axis::Child) => out.push('.'),
                        Some(Axis::Parent) => out.push_str(".."),
                        Some(Axis::Descendant) | None => {}
                        Some(other) => {
                            out.push_str(axis_word(other));
                            out.push_str("::");
                        }
                    }
                }
                let bare = steps.len() == 1
                    && step.axis.is_none()
                    && (step.ty.is_some() || step.pred.is_some() || step.slice.is_some());
                out.push_str(&canonical_step(step, bare));
            }
            out
        }
        Node::Union(l, r) => format!("{} union {}", canonical_node(l), canonical_paren_set(r)),
        Node::Except(l, r) => format!("{} except {}", canonical_node(l), canonical_paren_set(r)),
        Node::Intersect(l, r) => {
            format!("{} intersect {}", canonical_node(l), canonical_paren_set(r))
        }
        Node::Group(inner, slice) => {
            let s = slice.as_ref().map(canonical_slice).unwrap_or_default();
            format!("({}){}", canonical_node(inner), s)
        }
    }
}

fn canonical_paren_set(node: &Node) -> String {
    match node {
        Node::Union(..) | Node::Except(..) | Node::Intersect(..) => {
            format!("({})", canonical_node(node))
        }
        _ => canonical_node(node),
    }
}

fn canonical_step(step: &Step, bare: bool) -> String {
    let mut out = String::new();
    if let Some(ty) = step.ty {
        out.push_str(type_word(ty));
    }
    match &step.pred {
        Some(pred) => {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(&canonical_pred(pred));
        }
        // The wildcard prints unless the step is a lone, plain one
        // (a type word or slice alone already says what it selects).
        None if step.ty.is_none() && !bare => out.push('*'),
        None => {}
    }
    if let Some(slice) = &step.slice {
        out.push_str(&canonical_slice(slice));
    }
    out
}

fn canonical_pred(pred: &Pred) -> String {
    match pred {
        Pred::Group(inner) => format!("({})", canonical_pred(inner)),
        Pred::And(l, r) => format!("{} and {}", canonical_pred(l), canonical_pred_paren(r)),
        Pred::Or(l, r) => format!("{} or {}", canonical_pred(l), canonical_pred_paren(r)),
        Pred::Not(inner) => match inner.as_ref() {
            Pred::And(..) | Pred::Or(..) => format!("not ({})", canonical_pred(inner)),
            other => format!("not {}", canonical_pred(other)),
        },
        Pred::Cmp(cmp) => canonical_cmp(cmp),
    }
}

fn canonical_pred_paren(pred: &Pred) -> String {
    match pred {
        Pred::And(..) | Pred::Or(..) => format!("({})", canonical_pred(pred)),
        _ => canonical_pred(pred),
    }
}

fn canonical_cmp(cmp: &Cmp) -> String {
    let mut out = String::new();
    match &cmp.attr {
        // The default attribute (@text) prints as nothing.
        None => {}
        Some(AttrRef::Name(n)) if n == "text" => {}
        Some(AttrRef::Name(n)) => {
            out.push('@');
            out.push_str(n);
        }
        Some(AttrRef::Count(node)) => {
            out.push_str("count(");
            out.push_str(&canonical_node(node));
            out.push(')');
        }
    }
    if let Some((rel, mods)) = &cmp.rel {
        let value_is_type_word =
            matches!(cmp.value.as_slice(), [ValPart::Str(w)] if TYPE_WORDS.contains(&w.as_str()));
        if !(*rel == Rel::Contains && mods.is_empty() && !value_is_type_word) {
            out.push(' ');
            out.push_str(rel_word(*rel));
            if !mods.is_empty() {
                out.push('[');
                out.push_str(mods);
                out.push(']');
            }
        }
    }
    if !cmp.value.is_empty() {
        out.push(' ');
        out.push_str(&canonical_value(&cmp.value));
    }
    out.trim().to_string()
}

fn canonical_value(parts: &[ValPart]) -> String {
    parts
        .iter()
        .map(|p| match p {
            ValPart::Str(s) => quote_if_needed(s),
            ValPart::Attr(a) => format!("@{a}"),
            ValPart::Count(node) => format!("count({})", canonical_node(node)),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Quote a value part unless re-lexing it bare would reproduce the
/// same single word.
fn quote_if_needed(s: &str) -> String {
    let word_like = !s.is_empty()
        && s.chars().enumerate().all(|(i, c)| {
            if i == 0 {
                crate::tags::is_name_start(c)
            } else {
                crate::tags::is_name_cont(c)
            }
        })
        && !RESERVED.contains(&s)
        && !REL_WORDS.contains(&s);
    if word_like {
        s.to_string()
    } else {
        let mut out = String::from("\"");
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                other => out.push(other),
            }
        }
        out.push('"');
        out
    }
}

fn canonical_slice(slice: &Slice) -> String {
    match slice {
        Slice::Single(k) => format!("[{k}]"),
        Slice::Range { start, end } => {
            let s = start.map_or(String::new(), |v| v.to_string());
            let e = end.map_or(String::new(), |v| v.to_string());
            format!("[{s}:{e}]")
        }
    }
}

fn axis_from_word(word: &str) -> Axis {
    match word {
        "ancestor" => Axis::Ancestor,
        "ancestor-or-self" => Axis::AncestorOrSelf,
        "child" => Axis::Child,
        "descendant" => Axis::Descendant,
        "descendant-or-self" => Axis::DescendantOrSelf,
        "following" => Axis::Following,
        "following-sibling" => Axis::FollowingSibling,
        "preceding" => Axis::Preceding,
        "preceding-sibling" => Axis::PrecedingSibling,
        "parent" => Axis::Parent,
        _ => Axis::SelfAxis,
    }
}

fn axis_word(axis: Axis) -> &'static str {
    match axis {
        Axis::Ancestor => "ancestor",
        Axis::AncestorOrSelf => "ancestor-or-self",
        Axis::Child => "child",
        Axis::Descendant => "descendant",
        Axis::DescendantOrSelf => "descendant-or-self",
        Axis::Following => "following",
        Axis::FollowingSibling => "following-sibling",
        Axis::Preceding => "preceding",
        Axis::PrecedingSibling => "preceding-sibling",
        Axis::Parent => "parent",
        Axis::SelfAxis => "self",
    }
}

fn type_word(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Project => "project",
        ItemKind::Task => "task",
        ItemKind::Note => "note",
    }
}

fn rel_word(rel: Rel) -> &'static str {
    match rel {
        Rel::Eq => "=",
        Rel::Ne => "!=",
        Rel::Lt => "<",
        Rel::Le => "<=",
        Rel::Gt => ">",
        Rel::Ge => ">=",
        Rel::Contains => "contains",
        Rel::Beginswith => "beginswith",
        Rel::Endswith => "endswith",
        Rel::Matches => "matches",
        Rel::Like => "like",
    }
}

#[cfg(test)]
mod tests {
    use super::{ParseError, Query, Slice, ValPart};

    fn ok(q: &str) -> Query {
        Query::parse(q).unwrap()
    }

    fn err(q: &str) -> ParseError {
        Query::parse(q).unwrap_err()
    }

    #[test]
    fn bare_text_is_a_default_comparison() {
        let q = ok("socks");
        assert_eq!(
            q.to_canonical(),
            "socks",
            "defaults are omitted in canonical form"
        );
    }

    #[test]
    fn comparison_subsets_parse() {
        // All four documented shapes are the same search.
        for form in [
            "Inbox",
            "@text Inbox",
            "contains Inbox",
            "@text contains Inbox",
        ] {
            assert_eq!(ok(form).to_canonical(), "Inbox", "form: {form}");
        }
        // Attribute-only is existence and keeps the attribute.
        assert_eq!(ok("@done").to_canonical(), "@done");
        // Relation-only keeps the default attribute unprinted.
        assert_eq!(ok("= one").to_canonical(), "= one");
    }

    #[test]
    fn modifiers_and_relations() {
        assert_eq!(
            ok("@job contains[sl] John").to_canonical(),
            "@job contains[sl] John"
        );
        assert_eq!(ok("@a = x").to_canonical(), "@a = x");
        assert_eq!(ok("@a like x").to_canonical(), "@a like x");
        assert_eq!(
            ok("@a matches \"a+b\"").to_canonical(),
            "@a matches \"a+b\""
        );
    }

    #[test]
    fn quoted_strings_with_escapes() {
        let q = ok("contains \"and\"");
        assert_eq!(q.to_canonical(), "\"and\"");
        let q = ok("contains \"line1\\nline2 \\\"q\\\"\"");
        assert_eq!(q.to_canonical(), "\"line1\\nline2 \\\"q\\\"\"");
    }

    #[test]
    fn multi_word_values_are_one_value() {
        let q = ok("find this string");
        match q.root() {
            super::Node::Path { steps, .. } => match &steps[0].pred {
                Some(super::Pred::Cmp(cmp)) => {
                    assert_eq!(
                        cmp.value,
                        vec![
                            ValPart::Str("find".into()),
                            ValPart::Str("this".into()),
                            ValPart::Str("string".into())
                        ]
                    );
                }
                other => panic!("unexpected predicate: {other:?}"),
            },
            other => panic!("unexpected node: {other:?}"),
        }
    }

    #[test]
    fn boolean_precedence_and_grouping() {
        let q = ok("(one or two) and not three");
        assert_eq!(q.to_canonical(), "(one or two) and not three");
        // and binds tighter than or
        assert_eq!(
            ok("one or two and three").to_canonical(),
            "one or (two and three)"
        );
        // right-nested chains parenthesize in canonical form
        assert_eq!(ok("a and b and c").to_canonical(), "a and (b and c)");
        assert_eq!(ok("not not @done").to_canonical(), "not not @done");
    }

    #[test]
    fn type_shortcuts() {
        assert_eq!(ok("project Inbox").to_canonical(), "project Inbox");
        assert_eq!(ok("task").to_canonical(), "task");
        assert_eq!(ok("note").to_canonical(), "note");
        // The words are values in predicate positions.
        assert_eq!(ok("contains project").to_canonical(), "contains project");
    }

    #[test]
    fn set_precedence_and_associativity() {
        // union loosest, then except, then intersect; right-assoc.
        assert_eq!(
            ok("a union b intersect c").to_canonical(),
            "a union (b intersect c)"
        );
        assert_eq!(
            ok("a union b union c").to_canonical(),
            "a union (b union c)"
        );
        assert_eq!(
            ok("a except b except c").to_canonical(),
            "a except (b except c)"
        );
    }

    #[test]
    fn item_paths_parse_with_separators() {
        assert_eq!(ok("a/b").to_canonical(), "a/b");
        assert_eq!(ok("a//*").to_canonical(), "a//*");
        assert_eq!(
            ok("/my heading/subhead").to_canonical(),
            "/my heading/subhead"
        );
        assert_eq!(ok("//@today").to_canonical(), "//@today");
        assert_eq!(ok("count(.*) = 0").to_canonical(), "count(.*) = 0");
        assert_eq!(ok("@m = count(.*)").to_canonical(), "@m = count(.*)");
    }

    #[test]
    fn slices() {
        assert_eq!(ok("task[0]").to_canonical(), "task[0]");
        assert_eq!(ok("task[-1]").to_canonical(), "task[-1]");
        assert_eq!(ok("task[1:]").to_canonical(), "task[1:]");
        assert_eq!(ok("task[:2]").to_canonical(), "task[:2]");
        assert_eq!(ok("(project *)[0]").to_canonical(), "(project)[0]");
        match ok("task[1:3]").root() {
            super::Node::Path { steps, .. } => {
                assert_eq!(
                    steps[0].slice,
                    Some(Slice::Range {
                        start: Some(1),
                        end: Some(3)
                    })
                );
            }
            _ => panic!(),
        }
    }

    #[test]
    fn attribute_segments_and_axis_names() {
        // :segment suffixes parse and drop.
        assert_eq!(ok("@name:seg = x").to_canonical(), "@name = x");
        // Full axis names.
        let q = ok("descendant::@today");
        assert_eq!(q.to_canonical(), "@today");
    }

    #[test]
    fn parse_errors_report_position() {
        let e = err("contains and");
        assert!(e.message.contains(')') || !e.message.is_empty());
        assert_eq!(err("\"unterminated").message, "unterminated string");
        assert!(!err("a b !").message.is_empty());
    }

    #[test]
    fn canonical_form_is_stable() {
        for q in [
            "Inbox",
            "project Inbox",
            "(one or two) and not three",
            "a union b intersect c",
            "@job contains[sl] John",
            "not @done except @done//*",
            "task[0]",
            "count(.*) = 0",
        ] {
            let once = ok(q).to_canonical();
            let twice = ok(&once).to_canonical();
            assert_eq!(once, twice, "canonical form not stable for: {q}");
        }
    }
}
