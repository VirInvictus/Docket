# Docket spec

The contract. Behavior described here is normative: the roadmap
sequences it, the code implements it, and the tests pin it. Where a
rule is transcribed from the public TaskPaper User's Guide, the
source page is cited. Rules learned from behavioral study of
reference implementations are stated as facts here and pinned by
tests here; the evidence lives outside this repository.

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
- **The file is canonical.** Parsing and re-serializing is lossless
  and idempotent. Indentation, blank lines, and spelling are the
  user's, not ours.
- **GtkSourceView is the editing surface**, as it is for Quire.
  Custom gutter renderers (fold arrows) are the Quire-proven way to
  get outline behavior without abandoning a real text view.
- **Plain GTK4, no libadwaita**, styled by vir-gtk under Kanagawa
  Dragon.

### Fidelity and legal posture

Docket is an independent implementation of a publicly documented
format, built so that no one else's expression enters this tree.

- The normative reference is the public TaskPaper User's Guide
  (guide.taskpaper.com). The live site blocks datacenter fetches;
  Wayback Machine snapshots are the working source, cited by page
  and snapshot date.
- Behavior is cross-checked against the format author's own
  MIT-licensed reference libraries (birch-outline, BirchOutline,
  aequery), which are freely readable and reusable with attribution.
- Other reference implementations (cataloged in the workspace's
  reference table) were studied for behavioral facts only: rules,
  defaults, edge cases, feature scope. No code from them is copied,
  translated, or structurally mirrored, and no file paths or
  excerpts from them appear in this repository.
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
Every line with content is an item; the kind is decided lexically.
Rules marked canonical are transcribed from
guide.taskpaper.com/getting-started and cross-checked against the
author's MIT-licensed serialization:

- **Project**: a line whose content ends with `:` as its final
  character, where content means the line text with its trailing
  tag run removed. The project name is the content before the
  colon. Consequences: `- foo:` is a task, not a project, and
  `Name: @done` is a project whose `@done` is ordinary text.
- **Task**: a line whose first non-whitespace characters are one of
  the task markers `- `, `+ `, or `* ` (dash, plus, or asterisk
  followed by a space). Docket recognizes all three on read and
  writes only `- `.
- **Note**: any other line with content.
- **Blank lines** are group separators; they are not items. On
  load, a blank line adopts the indentation of the next non-empty
  line.

Leading whitespace nests an item under the nearest preceding
shallower item. The model depth unit is the tab. Space-indented
documents (as written by some third-party tools) normalize to tabs
on load: the smallest common leading-space run becomes one tab.
Over-indented lines keep their extra indentation through a
round-trip. Nesting defines children, and children matter to
folding, to filter scope, and to item paths below.

**Tags** appear anywhere in an item's text as `@name` or
`@name(value)`. A tag must be preceded by the start of the line or
whitespace, and followed by whitespace or end of line, so email
addresses and URLs never produce tags. Tag names may contain
letters (Unicode ranges included), digits, `-`, and `.`, and may
begin with a digit. Values are parenthesized; literal parentheses
inside a value are escaped as `\(` and `\)`. A value may be a
comma-separated list, which queries treat element-wise under the
`[l]` modifier. The first occurrence of a duplicated tag name is
the item's value for it; later occurrences are inert text.
`@id`, `@text`, and `@type` are reserved names that never become
tag attributes. A tag with empty parentheses carries an empty
string value, which is distinct from the tag being absent.

**Round-trip invariant.** Parsing followed by serializing is
lossless and idempotent: the first pass normalizes line endings
(CRLF and CR become LF), space indentation, and blank-line
indentation; every subsequent pass reproduces the bytes exactly.
Files carry no trailing newline. No feature may write derived state
(fold state, computed dates, counts) into the file.

### View state

Fold, focus, hoist, filter, and sidebar state are view state. It is
never written into the document. Docket persists it as a sidecar
under `~/.local/state/docket/`, keyed by document content hash:
state survives restarts and the rename of unchanged content, never
travels with the file when it is copied or synced, and resets when
the content changes.

### Query language

Transcribed from guide.taskpaper.com/reference/searches/ (snapshot
2019-12-16), with the precision of the author's MIT-licensed
implementation.

**Precedence.** Loosest to tightest: `union`, `except`,
`intersect`, then paths and predicates. The set operators chain
right-associatively and are whitespace-separated keywords.

**Predicates.** The full pattern is `@<attribute> <relation>
<search term>`. Parts may be omitted independently; the attribute
defaults to `@text`, the relation to `contains`, and the modifier
to `[i]`. So `Inbox`, `@text Inbox`, `contains Inbox`, and
`@text contains Inbox` are the same search. A bare `@tag` is an
existence test.

**Relations.** `=`, `!=`, `<`, `>`, `<=`, `>=`, `contains`,
`beginswith`, `endswith`, and `matches` (the term is converted to a
regular expression; an invalid regex matches nothing rather than
erroring). `like` parses and always matches nothing, the
documented quirk, kept for fidelity. All relations are
case-insensitive by default, `=` included.

**Modifiers** appear in brackets after a relation, combinable, as
in `contains[sl]`:

- `i`: case-insensitive compare (the default)
- `s`: case-sensitive compare
- `n`: numeric compare (both sides converted to numbers, so `01`
  equals `1`)
- `d`: date compare (both sides converted to dates; see Dates)
- `l`: list compare (both sides split on commas, so `@job
  contains[l] John` matches `@job(Jane,John)` but not `@job(Johny)`)

**Built-in attributes.** `@type` (project, task, or note), `@text`
(the item's full text including its task marker and tags), and
`@id` (the item's runtime id; ephemeral, never in the file). Any
tag exposes an attribute of the same name.

**Values.** Double quotes enclose values containing syntax
keywords, as in `contains "and"`, with backslash escapes.
Unquoted multi-word values concatenate into one value, so
`find this string` is a single search term. `count(path)`
evaluates a path relative to the item under test and yields its
result count.

**Booleans.** `and` binds tighter than `or`; `not` negates;
parentheses group. `(one or two) and not three` matches items
containing one or two but not three.

**Type shortcuts.** `project` expands to `@type = project and`;
`task` and `note` likewise. So `project Inbox` finds the Inbox
project.

**Item paths.** Paths control which part of the outline a search
runs over, with XPath-like steps of axis, type, predicate, and
optional slice. `/` is the child axis, `//` the descendant axis,
`///` the descendant-or-self axis, `..` the parent axis, `.` the
child axis, `*` the wildcard. Full axis names (`ancestor::`,
`following-sibling::`, and the rest) are supported. A path with a
leading `/` is absolute (anchored at the document root); a
relative path's first step defaults to the descendant axis.
Results are in document order and deduplicated. Set operations
combine path results: `(project Inbox//* union //@today) except
//@done`. Slices are zero-based with exclusive ends, negative
indices counting from the end, and out-of-range values clamping:
`project *//not @done[0]` yields each project's first open item.

**Failure semantics.** Missing or unparseable values make a
comparison false, never an error. A query that fails to parse
yields an empty result plus the parse error, which the UI surfaces.

**Scope.** A search evaluates within the focused or hoisted
subtree; the unscoped document ("Home") is the whole outline.
Results show matching items plus their ancestor context; everything
else hides. Filtering never edits the document.

### Dates

Transcribed from guide.taskpaper.com/reference/dates/ (snapshot
2019-07-21), with the precision of the author's MIT-licensed date
parser.

Date values resolve to midnight, local time, of the given day:
`2016`, `2016-01`, `2016-01-10`, `today`, `tomorrow`, `yesterday`,
`next week`, `next month`, `next Monday`, `last June`, `June`,
`next June 3`. `now` resolves to the current instant.

`this`, `next`, and `last` before a month or weekday name move by
one year or one week respectively (`next January` is next year's
January). `this|next|last` before a unit word uses start-of-unit
semantics: `this week` is the start of the current week, `next
month` the start of the next. Weeks are ISO weeks starting Monday,
per the guide (a deliberate choice over the author's library
default of a Sunday-start locale).

Times are relative to the current date: `6 am`, `3:15 pm`, `16:15`,
`at 2pm`.

Durations offset the result: `2 days`, `+1 week`, `-6 hours`,
`2 days 6 hours`. Duration units carry short aliases, with `o` for
months and `q` for quarters. A bare duration resolves from now,
not midnight: `+1 week` means now plus one week.

Combinations combine the forms, applied in order: `tomorrow 9am`,
`Nov 26 3:15 +1day`.

Input the grammar does not recognize falls back to ISO 8601
parsing, including ISO week dates (`2016-W51-4`). Unparseable
strings are invalid, never guessed.

The `[d]` relation modifier compares both sides of a predicate as
dates, so `@due < tomorrow` behaves as a date comparison.

### Editor behavior

- One window per document. Open and save through standard file
  dialogs; recent documents in the usual place; a welcome document
  on first run.
- **Continuous autosave.** There is no save button and no dirty
  state; the file on disk is always the truth. External changes are
  watched: auto-revert when the document is clean, prompt otherwise.
- Live formatting is presentation only. Highlighting, dimming, and
  folding change how text looks, never what it says. The default
  theme's rules: projects bold, notes italic, tags dimmed, `@done`
  content struck through (content only, never tags).
- Folding: any item with children folds. Fold commands have
  Completely and By Level variants; a filtered item's handle dims
  while a search is active.
- The leading dash of a task toggles `@done`; an optional
  preference stamps the completion date (`YYYY-MM-DD`).
- Tag completion proposes tag names drawn from the document census,
  in the editor and in the searchbar. Clicking a tag or tag value
  in text or sidebar applies it as the filter.
- Saved searches are `@search(query)` items, either embedded under
  a `Searches:` project in the document or global in
  `~/.config/docket/searches.taskpaper`. Seeded defaults: Today
  (`@today union @due <[d] tomorrow`) and Not Done (`not @done
  except @done//*`).
- Sidebar: Home, the document's projects (nested), searches, and
  tags with their distinct values as children; an
  include/exclude configuration controls which tags list.
- Enter continues the list: after a task line it opens a new task;
  after a project line, a child task; an empty item exits the
  list. Tab and Shift-Tab indent and outdent (never a literal tab);
  backspace at column zero outdents. Items and branches move with
  keyboard commands; pasted text is fully re-parsed,
  auto-classified, and depth-shifted to the insertion point.
- Keyboard bindings mirror TaskPaper's cluster, with Ctrl where
  TaskPaper uses Cmd.
- The 1.0 gate is the faithful editor: everything in the sections
  above. Command palette, user stylesheets, multiple windows and
  tabs, and scripting are post-1.0 (see roadmap).

### Out of scope (v1)

Sync, cloud services, accounts, calendar or reminder integration,
recurring tasks, a scripting or plugin API, color themes beyond
Kanagawa Dragon, multiple windows and tabs per document, and any
platform other than Linux. Docket is an editor; see "Relationship
to Atrium".

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

Resolved this pass (2026-10-12): tag name charset, value
termination, quoting and escaping, the trailing-tag-run content
rule, `+`/`*` task markers, blank-line indentation, over-indent
preservation, line-ending and space-indent normalization, the
no-trailing-newline rule, query precedence and defaults, list
values, `count()`, slice clamping, failure semantics, the full
date grammar, and the saved-search and sidebar models.

Still deferred, each to be pinned before the related phase ships:

- Stylesheet selector and property semantics (only if user
  stylesheets ever leave the post-1.0 list).
- The app's locale handling of week start (the guide says ISO
  Monday; the author's library defaults to Sunday): Docket commits
  to the guide and revisits only with user evidence.
