# Docket

Plain-text task lists for Linux. Docket is a native GTK4 editor for
the TaskPaper file format: projects end with a colon, tasks start
with a dash, notes are everything else, and `@tags` mark things up.
Your files stay plain text that any editor can open; Docket makes
them look, fold, and filter like an outliner without ever taking the
text away from you.

The design contract lives in [spec.md](spec.md); the work plan in
[roadmap.md](roadmap.md).

## Status

v1.0.0: the faithful editor. The full format, search, and date
engine; a folding editor with the complete fold-command cluster,
structural editing (moves, Delete Items, Select Branch, Format
As), and focus; live filtering with the full query language; a
sidebar of projects, saved searches, and tags; tag completion in
the editor and the searchbar; Archive Done; and the command
surface in a headerbar menu. The post-1.0 candidates (command
palette, stylesheets, a CLI) are sequenced in the roadmap.

## Installing

A Flatpak manifest targeting the GNOME 50 runtime ships in `data/`:

```sh
flatpak-builder --user --install --force-clean build-dir \
  data/io.github.virinvictus.Docket.yml
flatpak run io.github.virinvictus.Docket
```

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
