# Docket

Plain-text task lists for Linux. Docket is a native GTK4 editor for
the TaskPaper file format: projects end with a colon, tasks start
with a dash, notes are everything else, and `@tags` mark things up.
Your files stay plain text that any editor can open; Docket makes
them look, fold, and filter like an outliner without ever taking the
text away from you.

<!-- logo.svg is a placeholder; the real mark lands with packaging (roadmap Phase 8) -->

The design contract lives in [spec.md](spec.md); the work plan in
[roadmap.md](roadmap.md).

## Status

Phase 0 skeleton: workspace, spec, CI. Real editing arrives with the
Phase 1+ milestones in the roadmap.

## Building

Rust (edition 2024) with GTK 4 and GtkSourceView 5:

```sh
cargo build --release
```

System packages: `libgtk-4-dev` and `libgtksourceview-5-dev` (names
vary by distribution).

## Format compatibility

Docket reads and writes `.taskpaper` documents and implements the
format, search language, and date system documented in the public
[TaskPaper User's Guide](https://guide.taskpaper.com/), with
behavior cross-checked against the format author's MIT-licensed
reference libraries. Docket is an independent implementation: it is
not affiliated with or endorsed by Hog Bay Software, whose TaskPaper
application and name remain theirs.

## License

MIT. See [LICENSE](LICENSE).
