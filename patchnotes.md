# Patchnotes

Newest first.

## v0.2.0 (2026-10-12)

- The format engine: the complete document model with the
  reference classification rules (project colon after the
  trailing tag run, three task markers, blank lines as empty note
  items), the tag engine with boundary and escaping rules, and a
  lossless idempotent writer proven over a fixture corpus of
  real-world quirks.
- The query engine: the full TaskPaper 3 search language
  (predicates with default-attribute and case-insensitivity
  fallbacks, all relations including the like-never-matches quirk,
  the [i]/[s]/[n]/[d]/[l] modifiers, booleans, type shortcuts,
  union/except/intersect, clamped slices, count()) with canonical
  stringification and the spec's failure semantics. The matches
  relation runs on a small stdlib-only regex engine with a step
  budget, so pathological patterns fail closed instead of hanging
  the live search.
- The full date grammar behind the [d] modifier: keywords, month
  and weekday names with this/next/last, start-of-unit forms,
  times, durations with the reference alias table, combinations,
  and a strict ISO 8601 fallback for timestamps and ISO week
  dates. Conformance vectors come from the author's MIT-licensed
  library specs.
- The full item-path grammar per the reference parser's separator
  table (/ is descendant, // descendant-or-self, . child, ..
  parent), all eleven axes, and focus/hoist scope anchoring via
  run_scoped_at.
- The application shell: a one-window-per-document editor with
  continuous autosave, external-change reload (the view never
  holds unsaved state), recents, the welcome first-run document,
  TaskPaper syntax presentation (projects bold, tags dimmed, done
  items struck through) in Kanagawa Dragon via vir-gtk, and the
  desktop file and AppStream metadata validated in CI.

## v0.1.0 (2026-10-12)

- Initial skeleton: two-crate workspace (docket-core engine, docket
  GTK4 app), spec and roadmap seeded from the public TaskPaper
  format and search documentation, CI (rustfmt, clippy, tests), and
  a placeholder logo. Phase 0; no end-user behavior yet.
