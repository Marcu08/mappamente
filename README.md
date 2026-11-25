<div align="center">

<img src="docs/logo.svg" width="96" alt="Mappamente logo">

# Mappamente

**Mind maps, charted.**

[![CI](https://github.com/Marcu08/mappamente/actions/workflows/ci.yml/badge.svg)](https://github.com/Marcu08/mappamente/actions/workflows/ci.yml)
[![Version](https://img.shields.io/badge/version-0.1.0-blue)](CHANGELOG.md)
[![Rust](https://img.shields.io/badge/Rust-2024-orange?logo=rust)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#license)

</div>

<p align="center">
  <img src="docs/preview.png" alt="Mappamente editor showing a mind map" width="900">
</p>

Mappamente is a lightweight, open-source mind map editor written in Rust. The
name combines *mappa* (map) and *mente* (mind). It is built to stay small: a
native window, no WebView, plain JSON files you can read and diff, and a CLI
you can script. Editing, layout, themes and undo are local and instant.

> **v0.1.0:** the core engine, headless CLI and native editor are available.
> MCP and AI generation are the next milestones.

## Highlights

| | |
| --- | --- |
| **Edit in place** | Double-click a node (or press `F2`), then type. `Tab` adds a child, `Enter` adds a sibling, `Delete` removes a branch. |
| **Drag to organise** | Drag a branch to move it, or drop it on another node to reparent it. Drag the background to pan. |
| **Your data** | `.mappa` is readable JSON with a schema version. Writes are atomic and validated. |
| **Interop, not lock-in** | Import Markdown outlines; export Markdown and standalone SVG with four themes. |
| **Small by design** | Native `egui` canvas, no WebView, no Electron, no background services. |
| **Open source** | MIT OR Apache-2.0, no telemetry, no account. |

## Quick start

### Windows shortcut (no terminal)

From the repository root:

```powershell
cargo build --release
powershell -ExecutionPolicy Bypass -File scripts/install-shortcut.ps1
```

This creates **Mappamente** on your Desktop with its own icon. Double-click it
to open the editor. The script accepts `-StartMenu` or `-Destination <folder>`
and can be re-run after `cargo clean`.

Portable launch:

```powershell
.\target\release\mappamente-desktop.exe
.\target\release\mappamente-desktop.exe docs\demo.mappa
```

The first release build takes a few minutes because eframe/egui are compiled
from source; incremental builds are much faster.

### Keyboard

| Action | Shortcut |
| --- | --- |
| Edit selected node / double-click | `F2` |
| Add child | `Tab` |
| Add sibling | `Enter` |
| Delete branch | `Delete` / `Backspace` |
| Undo / redo | `Ctrl+Z` / `Ctrl+Shift+Z` (or `Ctrl+Y`) |
| Save / save as | `Ctrl+S` / `Ctrl+Shift+S` |
| Open / new | `Ctrl+O` / `Ctrl+N` |
| Fit the map | `F` |
| Navigate | arrow keys |
| Pan / zoom | drag the background / mouse wheel |

### CLI

`mappamente.exe` is the console binary.

```powershell
# create a map
.\target\release\mappamente.exe new ideas.mappa --title "Project ideas" --root "Launch" --layout tree

# Markdown outline -> native JSON
.\target\release\mappamente.exe convert notes.md ideas.mappa

# native JSON -> Markdown outline
.\target\release\mappamente.exe convert ideas.mappa notes.md

# standalone SVG with a theme and layout override
.\target\release\mappamente.exe render ideas.mappa ideas.svg --theme light --layout tree

# open the editor
.\target\release\mappamente.exe gui ideas.mappa
```

Install it on your `PATH` with `cargo install --path crates/mappamente`, then
drop the `.\target\release\` prefix. Run `mappamente --help` for every option.
More in [docs/cli.md](docs/cli.md).

## What works today

- **Editor:** inline labels, add/delete nodes, sibling ordering, drag to move or
  reparent, pan/zoom/fit, layout and theme switching, undo/redo, save and save
  as, open dialog, drag & drop, unsaved-change guard.
- **Documents:** tree model, short stable IDs, notes, colors, collapsed
  branches, manual positions, validation, forward-compatible JSON.
- **Layouts:** bilateral tree, radial and logical; wrapped labels with Unicode
  widths; deterministic output; no overlaps.
- **CLI:** `new`, `convert`, `render`, `gui`.
- **Formats:** native `.mappa`/`.json`, Markdown outlines, SVG.
- **Engineering:** 88 tests, CI for Linux/Windows/macOS, `clippy -D warnings`,
  rustdoc clean, dual license.

Not implemented yet: autosave, node notes/colors UI, collapse toggle in the
canvas, OPML/FreeMind, PNG/PDF, MCP server, AI panel. See the
[roadmap](#roadmap).

## File format

`.mappa` is UTF-8 JSON with a version, stable field order and a trailing
newline:

```json
{
  "version": 1,
  "id": "m_pd289a2w",
  "title": "Project ideas",
  "theme": "light",
  "layout": "tree",
  "created_at": 1795594643981,
  "modified_at": 1795594643981,
  "root": {
    "id": "n_gq596zks",
    "text": "Launch",
    "children": [
      { "id": "n_2qb1123j", "text": "First branch", "children": [] }
    ]
  }
}
```

Writes go to a temporary file, are synced, then renamed over the target. Read
[docs/file-format.md](docs/file-format.md) for every field and invariant.

Why JSON and not XML? Internally XML would add verbosity and parsing overhead
without any structural benefit. XML remains an **interop** format at the edges
(OPML and FreeMind, planned); Markdown and SVG are already implemented.

## Architecture

```
crates/
  mappamente-core/     no GUI dependencies
    model.rs           document tree, IDs, validation, operations
    layout.rs          tree / radial / logical layout, text wrapping
    history.rs         snapshot undo/redo, transactions, coalescing
    io/                native JSON + Markdown, atomic writes
    export/svg.rs      deterministic SVG renderer
    theme.rs           four shared palettes
  mappamente/          lib + two binaries
    cli.rs             new | convert | render | gui
    gui.rs             editing canvas (eframe)
    bin/mappamente     console CLI
    bin/mappamente-desktop  GUI-subsystem launcher for double-click
scripts/                shortcut installer
assets/                 application icon and shortcut icon
docs/                   demo map, format and CLI docs, screenshots
```

The core owns layout and persistence, so the editor, the SVG exporter and future
tooling all agree on geometry and colors. The console and desktop binaries share
the same library; the desktop one has no console window on Windows release
builds.

The planned MCP server and AI panel will consume one shared tool registry
instead of two independent APIs. External agents will edit maps through atomic
file writes with live reload; the in-app panel will call the same handlers
in-process.

## Roadmap

| Milestone | Scope | Status |
| --- | --- | --- |
| 1 | Core model, JSON, layout, undo, CI, licenses | Done |
| 2 | CLI, Markdown, SVG, preview window, shortcut | Done |
| 3 | Editing canvas: inline edit, add/delete, drag/reparent, save, undo | Done |
| 4 | OPML / FreeMind, PNG export, autosave | Planned |
| 5 | MCP server, live reload, client setup | Planned |
| 6 | AI panel with keyring storage and tool calling | Planned |
| 7 | Installer, file associations, packaging | Planned |

## Development

```bash
cargo test --all
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Demo assets are reproducible:

```powershell
.\target\release\mappamente.exe convert docs\demo.md docs\demo.mappa --theme light --layout tree --force
.\target\release\mappamente.exe render docs\demo.mappa docs\screenshot.svg --force
```

Contributions are welcome. Keep changes focused, add tests for behaviour, and
run the three commands above before opening a pull request.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option. Unless you explicitly state otherwise, any contribution
intentionally submitted for inclusion in this project by you, as defined in the
Apache-2.0 license, shall be dual licensed as above, without any additional
terms or conditions.
