# AGENTS.md

Guidance for coding agents working in Docket. Overrides the global
~/.zcode/AGENTS.md where the two conflict.

## What this is

A native GTK4 plain-text task editor for Linux, faithful to the
TaskPaper 3 file format and search language. Two-crate Rust
workspace: `crates/docket-core` (document parser, query language,
dates; NO GUI dependencies) and `crates/docket` (the GTK4 app; the
only crate that touches gtk4, sourceview5, or vir-gtk).

## Hard rules

- **Independent implementation; facts-only references.** The
  normative reference is the public User's Guide (guide.taskpaper.com;
  the live site blocks datacenter fetches, so use Wayback Machine
  snapshots and cite page plus snapshot date). The workspace keeps
  reference clones, cataloged in ~/.gitrepos/CLAUDE.md: the format
  author's MIT-licensed libraries (birch-outline, BirchOutline,
  aequery) and other TaskPaper tools (restricted-source or
  unlicensed). These may be mined for behavioral facts: rules,
  defaults, edge cases, feature scope. Never copy, translate, or
  structurally mirror their code. Never copy artwork, strings, or
  UI copy. No file paths or excerpts from reference clones ever
  enter this repository. Docket is never branded with the TaskPaper
  name; the README non-affiliation note stays.
- **The file is canonical.** Parsing and re-serializing is lossless
  and idempotent. Any feature that writes derived state (fold
  state, computed dates, counts) into the document is a spec
  violation. View state lives in the ~/.local/state/docket/ sidecar,
  never the file.
- **docket-core stays GUI-free.** The engine tests headless; the app
  crate is the only GTK consumer.
- No third-party dependencies in docket-core without asking. The
  app crate's stack (gtk4, sourceview5, vir-gtk) is already
  approved.

## References while building

The workspace keeps four reference clones with phase-scoped
retention (cataloged in ~/.gitrepos/CLAUDE.md): birch-outline
(MIT; through Phase 4, the engine oracle and test-case source),
taskpaper-mode (through Phase 3, the community query/date
cross-check), the TaskPaper 3 app source (through Phase 7,
facts-only), and vscode-todo-plus (until the post-1.0
PlainTasks-compat lane is decided). A phase gate sweeps the clones
whose phase closed. The differential-testing oracle is the author's
MIT-licensed library bundle at
~/.local/share/docket-oracle/birchoutline.js; it is never vendored
into this repository.

## Commands

- Build: `cargo build`
- Test: `cargo test`
- Lint: `cargo clippy --all-targets -- -D warnings` (CI enforces)
- Format: `cargo fmt --check` (CI enforces)

System packages for a local build: libgtk-4-dev, libgtksourceview-5-dev.

## Tests

docket-core carries the test weight: unit tests per module plus
file-driven engine tests where the fixture `.taskpaper` input, the
query, and the expected results all live as data. The fixture
corpus includes files written by other editors (space-indented,
spaced `@done (date)`, emails and URLs in prose). The app crate
gets smoke tests only; UI verification is manual on the desktop.

## Docs

spec.md is the contract; roadmap.md sequences it; patchnotes.md logs
releases, newest first, `## vX.Y.Z` headings. VERSION at the repo
root is the single version source and the workspace Cargo.toml must
match it (a version-sync test pins the pair).

## Release flow

Commit and push verified work (the standing push grant in the global
file; force-pushes still need approval), wait for CI to go green, then
tag: annotated `vX.Y.Z` at the release commit, full patchnotes entry as
the message (`--cleanup=verbatim`; the global git-habit rules carry the
complete procedure). Version carriers: `VERSION` and the workspace
`Cargo.toml`, pinned by the version-sync test.
