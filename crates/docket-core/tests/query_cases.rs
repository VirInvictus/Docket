//! File-driven query cases: the fixture document, the queries, and
//! the expected item ids all live as data. Item ids are indices in
//! document order (the runtime `@id`).

use docket_core::{Document, Query};

const DOC: &str = include_str!("fixtures/query-doc.taskpaper");

/// The fixture document's items, for reference when reading ids:
///
/// ```text
///  0  Work:
///  1    - Ship Docket 1.0 @priority(1) @due(2026-11-01)
///  2      - Write the query engine @done
///  3      - Write the date parser
///  4    - Review specs @today
///  5  Home:
///  6    - Pay rent @due(2026-10-31)
///  7      - Call the bank @done(2026-10-10)
///  8    - Buy @2fast tickets @today
///  9  Read: @archive
/// 10  A plain note
/// ```
const CASES: &[(&str, &[usize])] = &[
    // Bare text: the default @text contains, case-insensitive.
    ("query", &[2]),
    ("ENGINE", &[2]),
    // Existence.
    ("@done", &[2, 7]),
    ("@2fast", &[8]),
    ("@missing", &[]),
    // Boolean logic and grouping.
    ("not @done", &[0, 1, 3, 4, 5, 6, 8, 9, 10]),
    ("engine or parser", &[2, 3]),
    ("(engine or parser) and not @done", &[3]),
    // Type shortcuts.
    ("project", &[0, 5, 9]),
    ("task @today", &[4, 8]),
    ("note", &[10]),
    // Relations and modifiers.
    ("@priority = 1", &[1]),
    ("@priority >[n] 0", &[1]),
    ("@text contains[s] Docket", &[1]),
    ("@text beginswith \"- Write\"", &[2, 3]),
    ("@text endswith note", &[10]),
    ("@text matches \"^Work\"", &[0]),
    // The documented quirk: like parses and never matches.
    ("@text like engine", &[]),
    // Dates: absolute ISO forms compare; keyword dates compare as
    // dates against the table's frozen reference instant (the
    // fixture's dues both sit after it, so `tomorrow` matches none).
    ("@due <[d] 2026-11-01", &[6]),
    ("@due <[d] tomorrow", &[]),
    // Set operations.
    ("@today union @due", &[1, 4, 6, 8]),
    ("* except task", &[0, 5, 9, 10]),
    ("@today intersect @done", &[]),
    // The seeded default searches.
    ("@today union @due <[d] tomorrow", &[4, 8]),
    ("not @done except @done//*", &[0, 1, 3, 4, 5, 6, 8, 9, 10]),
    // Multi-step structural paths. // is descendant-or-self, so
    // Work//* includes the Work project itself; /. is a descendant
    // step then a child step (the grammar's own table).
    ("Work//*", &[0, 1, 2, 3, 4]),
    ("Work/*", &[1, 2, 3, 4]),
    ("Home/.", &[7]),
    ("project task", &[]),
    // Slicing.
    ("task[0]", &[1]),
    ("task[-1]", &[8]),
    ("task[300]", &[8]),
    ("task[1:]", &[2, 3, 4, 6, 7, 8]),
    ("(project *)[0]", &[0]),
    // count() relative to the tested item.
    ("count(.*) = 1", &[6]),
    ("count(*) = 0", &[2, 3, 4, 7, 8, 9, 10]),
];

#[test]
fn the_case_table_holds() {
    let doc = Document::load(DOC);
    // The table runs against a frozen reference instant so relative
    // keywords (`tomorrow`) are deterministic; the fixture's dates
    // sit after it, which is what the date cases expect.
    let now = docket_core::dates::parse("2026-10-15", 0).unwrap().0;
    for (query, expected) in CASES {
        let results = Query::parse(query)
            .unwrap_or_else(|e| panic!("query {query:?} failed to parse: {}", e.message))
            .run_at(&doc, now);
        assert_eq!(&results, expected, "query {query:?}");
    }
}

#[test]
fn parse_failures_carry_a_reportable_message() {
    // The guide's incomplete searches are parse errors with
    // positions; the UI reports the message and shows no results.
    for bad in ["contains and", "socks or", "@due <"] {
        let err = Query::parse(bad).unwrap_err();
        assert!(!err.message.is_empty(), "query {bad:?}");
    }
}
