# Roadmap

Phase 0 is done; the rest sequences the spec. Re-plan freely: the
spec outranks this file.

## Phase 0: skeleton

- [x] Two-crate workspace (docket-core, docket), VERSION 0.1.0
- [x] Spec, roadmap, patchnotes, README, AGENTS.md, LICENSE, logo
- [x] CI: rustfmt, clippy with warnings denied, cargo test
- [x] Public repo at Phase 0, the Quire precedent

## Phase 1: format engine

- [ ] Fidelity capture: transcribe the guide's tag and item rules
      into spec.md with citations (see spec "Fidelity capture")
- [ ] Line tokenizer and item tree: projects, tasks, notes, blank
      separators, indentation nesting
- [ ] Tag parsing: bare tags, values, quoted values
- [ ] Lossless writer; byte-for-byte round-trip tests over the
      fixture corpus
- [ ] Fixture corpus of real-world-shaped `.taskpaper` files

## Phase 2: query engine

- [ ] Predicate parser: attributes, relations, modifiers, quoted
      values
- [ ] Boolean logic (and, or, not, parentheses) with the guide's
      keyword semantics
- [ ] Type shortcuts (project, task, note)
- [ ] File-driven engine tests: query, fixture, and expected item
      ids as data

## Phase 3: dates

- [ ] Keyword dates, times, durations, combinations, ISO 8601
      fallback
- [ ] Midnight resolution and Monday-start weeks
- [ ] Date comparisons wired into the query engine (the [d]
      modifier)

## Phase 4: item paths

- [ ] Child, descendant, and descendant-or-self steps; wildcard;
      parent
- [ ] XPath axes
- [ ] union, intersect, except
- [ ] Slicing on steps and parenthesized paths

## Phase 5: app shell

- [ ] Single-window GtkSourceView editor: open, save, continuous
      autosave, recents
- [ ] TaskPaper syntax highlighting, Kanagawa Dragon via vir-gtk
- [ ] Desktop file and AppStream metainfo
      (io.github.virinvictus.Docket), validated in CI

## Phase 6: outline behavior

- [ ] Folding with a custom gutter renderer
- [ ] @done styling; tag completion from document tokens
- [ ] Item move (keyboard) and smart enter

## Phase 7: filter UI

- [ ] Searchbar over the full query language
- [ ] Sidebar: projects, tag counts, saved searches
- [ ] Scope semantics: the sidebar selection limits the search

## Phase 8: packaging and release

- [ ] Real app icon replacing the placeholder logo
- [ ] Flatpak manifest on the GNOME 50 runtime
- [ ] Release job gated on tags (v0.1.0 is the tagged Phase 0
      skeleton, Quire precedent; this phase cuts the first release
      with the app packaged)
