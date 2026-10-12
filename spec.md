# Docket spec

The contract. Behavior described here is normative: the roadmap
sequences it, the code implements it, and the tests pin it. Where a
rule is transcribed from the public TaskPaper User's Guide, the
source page is cited; where the guide has not yet been fully
captured, the gap is listed under "Fidelity capture", never silent.

## Purpose

Docket is a native Linux plain-text task editor, faithful to the
TaskPaper 3 file format and query language. The file is the
database: a Docket document is a `.taskpaper` text file, openable in
any editor, owned by nobody but the user. Docket's job is to make
that plain text look, fold, filter, and complete like an outliner
without ever taking the text away from you.

Docket is the TaskPaper counterpart to Quire (the Soulver
counterpart), and Quire's lessons are load-bearing here:

- **The engine is its own crate.** `docket-core` holds the document
  parser, the query language, and the date system, and depends on no
  GUI crate; `docket` (the app) is the only GTK code.
- **The file is canonical.** Parsing and re-serializing must
  round-trip a document byte-for-byte. Indentation, blank lines, and
  spelling are the user's, not ours.
- **GtkSourceView is the editing surface**, as it is for Quire.
  Custom gutter renderers (fold arrows) are the Quire-proven way to
  get outline behavior without abandoning a real text view.
- **Plain GTK4, no libadwaita**, styled by vir-gtk under Kanagawa
  Dragon.

### Fidelity and legal posture

Docket is a clean-room implementation built from public
documentation only.

- The reference is the public TaskPaper User's Guide
  (guide.taskpaper.com). The live site blocks datacenter fetches;
  Wayback Machine snapshots of the guide pages are the working
  source, cited by page and snapshot date.
- TaskPaper application source is never read. This includes the
  open-sourced 2014 iOS codebase (github.com/jessegrosjean/TaskPaper)
  and anything shipped inside the Mac app. No artwork, strings, or
  UI copy are copied from any TaskPaper application.
- "TaskPaper" is Hog Bay Software's trademark. Docket never brands
  itself with the name; the README carries the non-affiliation
  statement, and this file is the only place the name appears as
  anything other than the format's common description.

### Relationship to Atrium

Docket is an editor, not a task system. Atrium remains the
system-of-record task manager in this portfolio. Docket never grows
a recurrence engine, reminders, or a database; features that need a
system belong in Atrium.

## Semantics

### Document model

A document is UTF-8 text, conventionally suffixed `.taskpaper`.
Every line with content is an item; the kind is decided lexically,
transcribed from guide.taskpaper.com/getting-started:

- **Project**: a line whose text (after leading whitespace) ends
  with `:`. The project name is the text before the colon.
- **Task**: a line whose first non-whitespace characters are `- `
  (dash followed by a space).
- **Note**: any other line with content.
- **Blank lines** are group separators; they are not items.

Leading whitespace (tabs in TaskPaper's default) nests an item under
the nearest preceding shallower item. Nesting defines children, and
children matter to folding, to filter scope, and to item paths
below.

**Tags** appear anywhere in an item's text as `@name` or
`@name(value)`. A tag exposes a queryable attribute named for it,
with the tag's value (or no value) attached.

The fine tokenizer rules (which characters may appear in a tag name,
where a value terminates, how quotes behave inside a value) are
pinned in Phase 1 from the guide and then restated here as normative
rules with tests. See "Fidelity capture".

**Round-trip invariant**: for any input document, parsing followed
by serializing must produce the original bytes. No feature may write
derived state (fold state, computed dates, counts) into the file.

### Query language

Transcribed from guide.taskpaper.com/reference/searches/ (snapshot
2019-12-16).

**Predicates.** The full pattern is `@<attribute> <relation>
<search term>`. Defaults apply when parts are omitted: the attribute
defaults to `@text` and the relation defaults to `contains`, so
`Inbox`, `@text Inbox`, `contains Inbox`, and `@text contains Inbox`
are the same search.

**Relations.** `=`, `!=`, `<`, `>`, `<=`, `>=`, `contains`,
`beginswith`, `endswith`, and `matches` (the term is converted to a
regular expression).

**Modifiers** appear in brackets after a relation, combinable, as in
`contains[sl]`:

- `i`: case-insensitive compare (the default)
- `s`: case-sensitive compare
- `n`: numeric compare (both sides converted to numbers, so `01`
  equals `1`)
- `d`: date compare (both sides converted to dates; see Dates)
- `l`: list compare (both sides split on commas, so `@job
  contains[l] John` matches `@job(Jane,John)` but not `@job(Johny)`)

**Built-in attributes.** `@type` (project, task, or note), `@text`
(the item's full text), and `@id` (the item's unique id). Any tag
exposes an attribute of the same name.

**Values.** Enclose a value in double quotes when it collides with
syntax keywords, as in `contains "and"`.

**Booleans.** `and`, `or`, and `not` combine predicates; parentheses
group. `(one or two) and not three` matches items containing one or
two but not three.

**Type shortcuts.** `project` expands to `@type = project and`;
`task` and `note` likewise. So `project Inbox` finds the Inbox
project.

**Item paths.** Paths control which part of the outline a search
runs over, with XPath-like steps. Each step is an axis plus a
predicate; `/` is the child axis, `//` the descendant axis, `///`
the descendant-or-self axis. `*` is the wildcard predicate; `..` is
the parent axis; full XPath axis names (`ancestor::`,
`following-sibling::`, and the rest) are supported. Set operations
`union`, `intersect`, and `except` combine path results, as in
`(project Inbox//* union //@today) except //@done`. Slicing selects
ranges: `[0]` takes the first match, `[start:end]` a range, applied
to a single step or to a parenthesized whole path, so
`project *//not @done[0]` yields each project's first open item.

**Scope.** A search runs over the document selected in the sidebar;
"Home" means the whole outline. Results show matching items plus
their ancestor context; everything else hides. Filtering never edits
the document.

### Dates

Transcribed from guide.taskpaper.com/reference/dates/ (snapshot
2019-07-21).

Date values resolve to midnight, local time, of the given day:
`2016`, `2016-01`, `2016-01-10`, `today`, `tomorrow`, `next week`,
`next month`, `next Monday`, `last June`, `June`, `next June 3`.
Weeks start on Monday (ISO weeks), not Sunday.

Times are relative to the current date: `6 am`, `3:15 pm`, `16:15`.

Durations offset from the current time: `2 days`, `+1 week`,
`-6 hours`, `2 days 6 hours`.

Combinations combine the forms: `tomorrow 9am`, `Nov 26 3:15 +1day`.

Input the parser does not recognize falls back to ISO 8601
parsing (the guide's fallback is the momentjs library; Docket
implements the ISO 8601 subset directly, and documents any accepted
deviation in patchnotes).

The `[d]` relation modifier compares both sides of a predicate as
dates, so `@due < tomorrow` behaves as a date comparison.

### Editor behavior

- One window per document. Open and save through standard file
  dialogs; recent documents in the usual place.
- **Continuous autosave.** There is no save button and no dirty
  state; the file on disk is always the truth.
- Live formatting is presentation only. Highlighting, dimming, and
  folding change how text looks, never what it says.
- Folding: a project folds its children. Fold state is view state,
  per window; it is never written into the file.
- `@done` items render dimmed with a strikethrough.
- Tag completion proposes tag names and values already present in
  the document.
- Enter continues the list: after a task line it opens a new task;
  after a project line, a child task; an empty item exits the list.
- Items move with keyboard move-line bindings, keeping their
  indentation and children.

### Out of scope (v1)

Sync, cloud services, accounts, reminders, notifications, recurring
tasks, a scripting or plugin API, color themes beyond Kanagawa
Dragon, and any platform other than Linux. Docket is an editor; see
"Relationship to Atrium".

## Stack and packaging

- Rust 2024, two-crate workspace: `crates/docket-core` (engine, no
  GUI dependencies) and `crates/docket` (the GTK4 app).
- gtk4 0.11 (no libadwaita), sourceview5 0.11, and vir-gtk (git
  dependency) for the shared styling layer. `docket-core` is
  dependency-free.
- App id `io.github.virinvictus.Docket`. A desktop file and
  AppStream metainfo land in Phase 5; the Flatpak manifest, on the
  GNOME 50 runtime, lands with packaging in Phase 8.
- `VERSION` at the repo root is the single version source; the
  workspace `Cargo.toml` matches it, pinned by a test.

## Fidelity capture

Rules this spec defers to Phase 1, each to be transcribed from the
named guide page and then restated as normative text with tests:

- Tag name character set, value termination, and quoting rules
  (Getting Started, Using Tags).
- Autocomplete and date-tag behavior (@due, @start, @created) as the
  guide documents it (Using Dates & Reminders, Autocomplete).
- Stylesheet semantics, only if custom user stylesheets ever leave
  the out-of-scope list (TaskPaper Reference: StyleSheets).
