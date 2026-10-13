//! Tag scanning per the spec's tag rules: `@name` or `@name(value)`,
//! preceded by start-of-body or whitespace and followed by whitespace
//! or end-of-body, so email addresses and URLs never produce tags.

use std::borrow::Cow;

/// One tag occurrence in an item body, with its byte range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag<'a> {
    pub name: &'a str,
    /// `None` for a bare `@name`, `Some("")` for empty parentheses.
    /// Both carry an empty attribute value; the distinction exists
    /// at parse level only.
    pub value: Option<Cow<'a, str>>,
    pub start: usize,
    /// One past the closing paren, or one past the name for a bare tag.
    pub end: usize,
}

pub(crate) fn is_name_start(c: char) -> bool {
    c.is_ascii_alphabetic()
        || c.is_ascii_digit()
        || c == '_'
        || ('\u{C0}'..='\u{FFFD}').contains(&c)
}

pub(crate) fn is_name_cont(c: char) -> bool {
    is_name_start(c)
        || c == '-'
        || c == '.'
        || c == '\u{B7}'
        || ('\u{300}'..='\u{36F}').contains(&c)
        || ('\u{203F}'..='\u{2040}').contains(&c)
}

/// Scan every tag occurrence in a body, left to right. Occurrences
/// never overlap; a failed candidate (unclosed or malformed value)
/// consumes only its `@` so a tag nested inside it can still parse.
/// Duplicates and reserved names (`@id`, `@text`, `@type`) are
/// reported here; attribute resolution resolves them, not the scan.
pub fn scan(body: &str) -> Vec<Tag<'_>> {
    let bytes = body.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'@' {
            i += 1;
            continue;
        }
        let boundary_before = i == 0
            || body[..i]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace);
        if !boundary_before {
            i += 1;
            continue;
        }
        match parse_at(body, i) {
            Some((tag, end)) => {
                out.push(tag);
                i = end;
            }
            None => i += 1,
        }
    }
    out
}

fn parse_at(body: &str, at: usize) -> Option<(Tag<'_>, usize)> {
    let bytes = body.as_bytes();
    let mut j = at + 1;
    while let Some(ch) = body[j..].chars().next() {
        let allowed = if j == at + 1 {
            is_name_start(ch)
        } else {
            is_name_cont(ch)
        };
        if !allowed {
            break;
        }
        j += ch.len_utf8();
    }
    let name = &body[at + 1..j];

    if bytes.get(j) == Some(&b'(') {
        let mut k = j + 1;
        let mut value = String::new();
        loop {
            let c = body[k..].chars().next()?;
            match c {
                // A raw `(` inside a value fails the whole tag.
                '(' => return None,
                ')' => {
                    let end = k + 1;
                    let ok_after = body[end..].chars().next().is_none_or(char::is_whitespace);
                    if !ok_after {
                        return None;
                    }
                    return Some((
                        Tag {
                            name,
                            value: Some(Cow::Owned(value)),
                            start: at,
                            end,
                        },
                        end,
                    ));
                }
                '\\' => match body[k + 1..].chars().next() {
                    Some('(') => {
                        value.push('(');
                        k += 2;
                    }
                    Some(')') => {
                        value.push(')');
                        k += 2;
                    }
                    // Only parentheses are escapes; a lone backslash
                    // is an ordinary character.
                    _ => {
                        value.push('\\');
                        k += 1;
                    }
                },
                _ => {
                    value.push(c);
                    k += c.len_utf8();
                }
            }
        }
    }

    // Bare tag: must be followed by whitespace or end of body.
    match body[j..].chars().next() {
        None => {}
        Some(c) if c.is_whitespace() => {}
        Some(_) => return None,
    }
    Some((
        Tag {
            name,
            value: None,
            start: at,
            end: j,
        },
        j,
    ))
}

/// Resolve an item attribute by name: the first tag with that name
/// wins, bare tags and empty parentheses both carry the empty
/// value, and the reserved names (`id`, `text`, `type`) never
/// resolve. Query-side attribute lookup builds on this.
pub fn attribute<'a>(body: &'a str, name: &str) -> Option<Cow<'a, str>> {
    if matches!(name, "id" | "text" | "type") {
        return None;
    }
    scan(body)
        .into_iter()
        .find(|t| t.name == name)
        .map(|t| t.value.unwrap_or(Cow::Borrowed("")))
}

#[cfg(test)]
mod tests {
    use super::scan;

    fn names(body: &str) -> Vec<&str> {
        scan(body).iter().map(|t| t.name).collect()
    }

    #[test]
    fn boundaries() {
        assert_eq!(names("@a"), vec!["a"]);
        assert_eq!(names("x @a"), vec!["a"]);
        assert!(names("x@a").is_empty(), "no whitespace before");
        assert!(names("@a:").is_empty(), "colon right after name");
        assert!(names("@a(x)y").is_empty(), "no whitespace after value");
        assert!(names("@a(x)(y)").is_empty(), "paren right after value");
    }

    #[test]
    fn values() {
        let tags = scan("@a(1,2)");
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].value.as_deref(), Some("1,2"));

        let tags = scan("@a(b\\) c)");
        assert_eq!(tags[0].value.as_deref(), Some("b) c"));

        let tags = scan("@a(b\\(c)");
        assert_eq!(tags[0].value.as_deref(), Some("b(c"));

        // Empty parens are an empty value; bare is None.
        assert_eq!(scan("@a()")[0].value.as_deref(), Some(""));
        assert_eq!(scan("@a")[0].value, None);

        // A raw `(` fails the tag; only the escaped form works.
        assert!(scan("@a(b(c)").is_empty());
    }

    #[test]
    fn attribute_resolution() {
        use super::attribute;

        // First duplicate wins.
        assert_eq!(
            attribute("@x(one) and @x(two)", "x").as_deref(),
            Some("one")
        );
        // Bare tags and empty parens both carry the empty value.
        assert_eq!(attribute("@done", "done").as_deref(), Some(""));
        assert_eq!(attribute("@empty()", "empty").as_deref(), Some(""));
        // Missing and reserved names never resolve.
        assert_eq!(attribute("@a(1)", "b"), None);
        for reserved in ["id", "text", "type"] {
            assert_eq!(attribute("@id @text @type", reserved), None);
        }
    }

    #[test]
    fn failure_lets_inner_tags_parse() {
        // @a's value never closes cleanly; @c still parses.
        let tags = scan("hello @a(b @c(d)");
        assert_eq!(names("hello @a(b @c(d)"), vec!["c"]);
        assert_eq!(tags[0].name, "c");
    }

    #[test]
    fn name_charset() {
        assert_eq!(names("@2fast"), vec!["2fast"]);
        assert_eq!(names("@a-b.c"), vec!["a-b.c"]);
        assert_eq!(names("@ünïcode"), vec!["ünïcode"]);
        assert_eq!(names("@"), vec![""], "lone @ is an empty-name tag");
    }

    #[test]
    fn ranges_are_reported() {
        let tags = scan("x @done(1) y");
        assert_eq!(body_of("x @done(1) y", &tags[0]), "@done(1)");
    }

    fn body_of<'a>(body: &'a str, tag: &super::Tag<'_>) -> &'a str {
        &body[tag.start..tag.end]
    }
}
