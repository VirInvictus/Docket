//! Fixture-corpus round-trip tests: the lossless writer's proof.
//! Canonical fixtures round-trip byte-for-byte on the first pass;
//! every fixture is byte-stable from the second pass on.

use docket_core::{Document, ItemKind};

const CANONICAL: &[(&str, &str)] = &[
    ("basic", include_str!("fixtures/basic.taskpaper")),
    ("quirks", include_str!("fixtures/quirks.taskpaper")),
    ("overindent", include_str!("fixtures/overindent.taskpaper")),
    (
        "single-space",
        include_str!("fixtures/single-space.taskpaper"),
    ),
];

const NORMALIZING: &[(&str, &str)] = &[
    (
        "space-indent",
        include_str!("fixtures/space-indent.taskpaper"),
    ),
    ("blanks", include_str!("fixtures/blanks.taskpaper")),
];

#[test]
fn canonical_fixtures_round_trip_byte_for_byte() {
    for (name, src) in CANONICAL {
        assert_eq!(
            Document::load(src).serialize(),
            *src,
            "fixture {name} is not byte-exact"
        );
    }
}

#[test]
fn every_fixture_is_stable_from_the_second_pass_on() {
    for (name, src) in CANONICAL.iter().chain(NORMALIZING) {
        let first = Document::load(src).serialize();
        let second = Document::load(&first).serialize();
        assert_eq!(first, second, "fixture {name} never stabilized");
    }
}

#[test]
fn basic_fixture_classifies_and_parents() {
    let doc = Document::load(include_str!("fixtures/basic.taskpaper"));
    assert_eq!(doc.items[0].kind(), ItemKind::Project);
    assert_eq!(doc.items[2].kind(), ItemKind::Task);
    assert_eq!(doc.items[4].kind(), ItemKind::Note);
    assert_eq!(doc.items[9].kind(), ItemKind::Note);
    assert_eq!(doc.items[2].depth, 2);
    assert_eq!(doc.parent_of(2), Some(1));
    assert_eq!(doc.parent_of(0), None);
    assert_eq!(doc.items[2].content(), "- Milk");
    assert_eq!(doc.items[2].tags()[0].name, "due");
}

#[test]
fn quirks_fixture_holds_the_ecosystem_lines() {
    let doc = Document::load(include_str!("fixtures/quirks.taskpaper"));

    // Emails and URLs never produce tags.
    assert!(doc.items[0].tags().is_empty());
    assert!(doc.items[1].tags().is_empty());

    // "Project: @done" is a project carrying a real @done tag.
    assert_eq!(doc.items[2].kind(), ItemKind::Project);
    assert_eq!(doc.items[2].tags().len(), 1);

    // A task marker wins over a project colon; a colon-final note
    // with trailing whitespace stays a note (unit-tested too).
    assert_eq!(doc.items[3].kind(), ItemKind::Task);
    assert_eq!(doc.items[4].kind(), ItemKind::Project);

    // The spaced-done line keeps its parenthetical as text: the tag
    // is bare and the trailing run is empty, so it is a note.
    assert_eq!(doc.items[6].kind(), ItemKind::Note);
    assert_eq!(doc.items[6].tags()[0].name, "done");
    assert_eq!(doc.items[6].tags()[0].value, None);

    // Escaped parens parse; a raw paren fails the whole tag.
    assert_eq!(doc.items[7].tags()[0].value.as_deref(), Some("b) c"));
    assert!(doc.items[8].tags().is_empty());

    // The lone @ is an empty-name tag; digit-leading names work.
    assert_eq!(doc.items[9].tags()[0].name, "");
    assert_eq!(doc.items[10].tags()[0].name, "2fast");

    // Duplicate names both scan; resolution is a query-level rule.
    assert_eq!(doc.items[11].tags().len(), 2);

    // Empty parens are an empty value, distinct from bare.
    assert_eq!(doc.items[12].tags()[0].value.as_deref(), Some(""));

    // Reserved names still scan as tags; attributes resolve later.
    assert_eq!(doc.items[13].tags().len(), 2);
}

#[test]
fn normalizing_fixtures_settle_on_their_canonical_form() {
    // space-indent: smallest common run (4) becomes the tab width.
    let space = Document::load(include_str!("fixtures/space-indent.taskpaper"));
    assert_eq!(space.items[1].depth, 1);
    assert_eq!(space.items[2].depth, 2);
    assert_eq!(
        space.serialize(),
        "Root:\n\t- one\n\t\t- nested\n\tNote child\nLoose note"
    );

    // blanks: each blank adopts the next non-empty line's depth.
    let blanks = Document::load(include_str!("fixtures/blanks.taskpaper"));
    assert_eq!(blanks.items[1].depth, 1);
    assert_eq!(blanks.items[2].depth, 1);
    assert_eq!(blanks.items[3].depth, 0);
    assert_eq!(
        blanks.serialize(),
        "A:\n\t\n\t- child after blank\n\n\nB:\n\ttail note"
    );
}
