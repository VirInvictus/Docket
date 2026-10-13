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

- [ ] Keyword dates: `now`, `yesterday`, `today`, `tomorrow`
      (midnight)
- [ ] Month and weekday names with `this` / `next` / `last` (next
      month +1 year; next weekday +1 week); month names with
      optional day
- [ ] `this|next|last <unit>` start-of-unit semantics (week,
      month, quarter, year, isoweek)
- [ ] Times: `at 2pm`, `2:15[:ss]`, am/pm
- [ ] Durations: `+N unit` / `-N unit` with the documented unit
      aliases, chainable, combinable after dates and times; a bare
      duration resolves from now, not midnight
- [ ] ISO 8601 fallback including ISO week dates; absolute dates at
      local midnight; Monday-start weeks per the guide
- [ ] Date comparisons wired through the query engine (the `[d]`
      modifier, both sides coerced)

## Phase 4: item paths (the remainder)

The structural axes, multi-step chaining, set operations, and
slicing landed with Phase 2; what remains is the axes and the
anchoring that Phase 2 defers.

- [ ] Exotic XPath axes: ancestor, ancestor-or-self, following,
      following-sibling, preceding, preceding-sibling (the child,
      descendant, descendant-or-self, parent, and self axes already
      evaluate)
- [ ] Absolute vs relative anchoring with focus/hoist scope:
      relative paths anchor at the focused subtree, absolute paths
      at the document root
- [ ] Oracle reconciliation of separator semantics (`/` and `//`
      between steps) against the author's MIT-licensed reference
      engine, resolving the guide-versus-implementation divergence
      the spec flags

## Phase 5: app shell

- [ ] Single-window GtkSourceView editor: open, save, continuous
      autosave, recents, welcome document on first run
- [ ] External-change watch: auto-revert when clean, prompt
      otherwise
- [ ] Syntax presentation per the default theme's rules: projects
      bold, notes italic, tags dimmed, `@done` content struck
      through (content only, never tags); Kanagawa Dragon via
      vir-gtk
- [ ] View-state sidecar (fold, focus, filter) under
      ~/.local/state/docket/, keyed by document content hash
- [ ] Keyboard map mirrors TaskPaper's cluster (Ctrl where TaskPaper
      uses Cmd), documented in-app
- [ ] Desktop file and AppStream metainfo
      (io.github.virinvictus.Docket), validated in CI

## Phase 6: outline behavior

- [ ] Folding: any item with children; fold gutter handle (click
      toggles, alt-click focuses); expand/collapse commands with
      Completely and By Level variants; guide lines beneath
      expanded ancestors
- [ ] Editing: smart enter (task continuation, project child,
      empty-item behavior, depth rule after collapsed parents);
      Tab / Shift-Tab indent (never a literal tab); backspace at
      column zero outdents; item and branch move commands;
      duplicate keeping fold state; group selection under a new
      parent
- [ ] Move to Project: pick a project, or type a new name to create
      it on the fly
- [ ] Drag: handle drag with on/above/below drop semantics and
      into-own-descendant rejection
- [ ] Paste: full re-parse and auto-classification of pasted text,
      depth-shifted to the insertion point
- [ ] Undo: grouped structural steps, typing coalescing, selection
      restore
- [ ] @done: dash click toggles, keyboard binding, optional date
      stamp on completion (preference)

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
