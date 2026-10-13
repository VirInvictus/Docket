# Roadmap

Phase 0 is done. The 1.0 gate is the end of Phase 8: the faithful
editor. Phases after that are post-1.0 candidates; they may be
re-ordered or dropped. The spec outranks this file.

## Phase 0: skeleton (done)

- [x] Two-crate workspace (docket-core, docket), VERSION 0.1.0
- [x] Spec, roadmap, patchnotes, README, AGENTS.md, LICENSE, logo
- [x] CI: rustfmt, clippy with warnings denied, cargo test
- [x] Public repo at Phase 0, the Quire precedent
- [x] Roadmap fleshed to the full mapped surface (2026-10-12)

## Phase 1: format engine

The document contract, complete and pinned by tests.

- [x] Item tree: one item per line; classification exactly per spec
      (project colon as the final content character after the
      trailing tag run; task markers `- `, `+ `, `* ` recognized on
      read, `- ` written; notes; blank-line separators)
- [x] Indentation model: tab depth, nearest-shallower parenting,
      over-indent preservation, space-indent normalization
      (smallest common leading-space run), line-ending
      normalization, no trailing newline
- [x] Tag engine: boundary rules (line start or whitespace before,
      whitespace or end of line after), name charset, parenthesized
      values with `\(` / `\)` escapes, comma lists, empty-string
      vs absent values, duplicate names first-wins, reserved
      `@id` / `@text` / `@type`
- [x] Lossless writer: parse/serialize idempotence tests over the
      whole fixture corpus
- [x] Fixture corpus: real-world-shaped `.taskpaper` files covering
      the ecosystem's known quirks (space-indented files, spaced
      `@done (date)` variants, email addresses and URLs in prose,
      over-indents, mixed line endings, output of other editors)

## Phase 2: query engine

The full TaskPaper 3 search language. File-driven engine tests:
fixture, query, and expected item ids as data.

- [x] Predicate grammar: `@attribute relation term` with the
      documented defaults (`@text`, `contains`, case-insensitive);
      subsettable comparisons; quoted values with escapes;
      multi-word unquoted values as one value; reserved words
- [x] Relations: `= != < <= > >= contains beginswith endswith
      matches`, plus `like` accepted and matching nothing (the
      documented quirk); modifiers `[i] [s] [n] [d] [l]` with
      element-wise list semantics
- [x] Booleans: `and` binding tighter than `or`, grouped `not`,
      parentheses
- [x] Type shortcuts: `project`, `task`, `note`
- [x] Set operations: `union`, `except`, `intersect` with the
      documented precedence and right-associative chaining
- [x] Slicing: `[n]`, `[n:m]`, `[n:]`, `[:m]`, negative indices,
      clamping, exclusive ends
- [x] Failure semantics: missing or unparseable values compare
      false rather than erroring; an invalid regex matches nothing;
      a parse failure reports the error and yields empty results
- [x] `count(path)`; canonical stringification of a parsed query
      (defaults omitted) for display and round-tripping
- [x] Multi-step structural paths (child, descendant,
      descendant-or-self, parent, self) over the outline, with the
      seeded default searches passing as cases

## Phase 3: dates

- [x] Keyword dates: `now`, `yesterday`, `today`, `tomorrow`
      (midnight)
- [x] Month and weekday names with `this` / `next` / `last` (next
      month +1 year; next weekday +1 week); month names with
      optional day
- [x] `this|next|last <unit>` start-of-unit semantics (week,
      month, quarter, year, isoweek)
- [x] Times: `at 2pm`, `2:15[:ss]`, am/pm
- [x] Durations: `+N unit` / `-N unit` with the documented unit
      aliases, chainable, combinable after dates and times; a bare
      duration resolves from now, not midnight
- [x] ISO 8601 fallback including ISO week dates; absolute dates at
      local midnight; Monday-start weeks per the guide
- [x] Date comparisons wired through the query engine (the `[d]`
      modifier, both sides coerced); conformance vectors from the
      author's MIT-licensed library specs, run against an
      injectable reference instant

## Phase 4: item paths (done)

- [x] Exotic XPath axes: ancestor, ancestor-or-self, following,
      following-sibling, preceding, preceding-sibling (the child,
      descendant, descendant-or-self, parent, and self axes landed
      with Phase 2)
- [x] Absolute vs relative anchoring with focus/hoist scope:
      relative paths anchor at the focused subtree (run_scoped_at),
      absolute paths at the document root
- [x] Separator semantics reconciled against the reference
      grammar's own source: between steps / is the descendant axis,
      // descendant-or-self, . child, .. parent; there is no ///
      token (it parses as // then /). The user guide's "direct
      children" prose is looser than the shipped parser, and the
      parser wins; spec.md carries the same table

## Phase 5: app shell

- [x] Single-window GtkSourceView editor: open, save, continuous
      autosave, recents, welcome document on first run
- [x] External-change watch: auto-revert when clean (the view
      never holds unsaved state, so external changes always
      reload, cursor and selection clamped)
- [x] Syntax presentation per the default theme's rules: projects
      bold, notes italic, tags dimmed, `@done` items struck
      through (content only, never tags); Kanagawa Dragon via
      vir-gtk, shipped as a bundled .lang plus style scheme
- [x] Desktop file and AppStream metainfo
      (io.github.virinvictus.Docket), validated in CI
- [x] Standard file accelerators (open, save-as, quit); the
      task-cluster keymap moves to Phase 6 with the commands it
      binds
- [x] VERSION 0.2.0 with the release notes for the first packaged
      release (tag pending)

## Phase 6: outline behavior

- [x] View-state sidecar under ~/.local/state/docket/, keyed by
      document content hash (fold state round-trips through
      restarts); the keyboard map mirrors TaskPaper's cluster
      (Ctrl where TaskPaper uses Cmd): Ctrl+D done, Ctrl+Shift+D
      duplicate, Ctrl+Alt+G group, Ctrl+Alt+arrows move, Tab and
      Shift-Tab indent
- [x] Folding: any item with children folds via the gutter marker
      (click toggles); the caret is guarded out of hidden ranges;
      copy and cut never leak folded text (the invisible-tag
      pitfall the research flagged)
- [x] Editing: smart enter (task continuation, project child,
      empty-item behavior, depth rule via anchor depth); Tab /
      Shift-Tab indent (never a literal tab); backspace at column
      zero outdents; duplicate block; group selection under a new
      parent
- [x] Paste: full re-parse and auto-classification of pasted text,
      depth-shifted to the insertion point
- [x] @done: Ctrl+D toggles on the selection (add, remove, or
      re-stamp per the reference all-have-else-set rule); the
      dash-click hit zone is wired
- [ ] Move to Project, item drag, undo grouping beyond the
      buffer's own undo: deferred to the Phase 7 sidebar and the
      post-1.0 polish pass (they need the palette infrastructure)

## Phase 7: filter and sidebar UI

- [ ] Searchbar: hidden while empty, live filtering, syntax errors
      surfaced in place, Escape cancels, refresh re-evaluates
      relative dates against now
- [ ] Results: matching items with their ancestor chains;
      non-matching descendants hidden (the filtered fold state,
      dimmed handle)
- [ ] Saved searches as `@search(query)` items: embedded under a
      `Searches:` project, or global in
      ~/.config/docket/searches.taskpaper; defaults seeded (Today:
      `@today union @due <[d] tomorrow`; Not Done: `not @done
      except @done//*`)
- [ ] Sidebar: Home, nested Projects, Searches, Tags with distinct
      values as children; include/exclude tag configuration;
      clicking a tag or tag value filters, in the sidebar and in
      item text
- [ ] Tag completion from the document census, in the editor and in
      the searchbar
- [ ] Archive done: find-or-create `Archive:`, move topmost done
      items with descendants riding along, optional `@project`
      origin tag recording the source path (preference)

## Phase 8: packaging and release (1.0)

- [ ] Real app icon replacing the placeholder logo
- [ ] Flatpak manifest on the GNOME 50 runtime
- [ ] Release job gated on tags
- [ ] 1.0 pass: spec-compliance review over the whole diff,
      patchnotes, tagged release

## Phase 9 and later (post-1.0 candidates, unsequenced)

- [ ] Command palette: Go to Anything, command palette, fuzzy
      matching with highlighted matches
- [ ] User stylesheets: a CSS-subset theming system over the
      editor's element model
- [ ] Multiple windows and tabs per document
- [ ] A `docket` CLI speaking the query language against files
      (filter, archive, tag) as the first scripting consumer
- [ ] Print support and a print stylesheet
- [ ] PlainTasks-flavored read compatibility (box bullets, spaced
      `@done (date)`, `@started` / `@lasted` conventions)
