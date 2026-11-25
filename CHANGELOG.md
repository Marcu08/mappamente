# Changelog

All notable changes to Mappamente are documented here.

## [0.1.0] - 2026-11-25

First public version.

### Added

- Native editing canvas built with `egui`/`eframe`:
  - inline node text editing with `F2` or double-click;
  - `Tab` adds a child, `Enter` inserts a sibling, `Delete` removes a branch;
  - drag to move a node or reparent it onto another node;
  - pan, zoom and fit-to-view;
  - layout (`tree`, `radial`, `logical`) and theme (`dark`, `light`, `paper`,
    `ocean`) switching, persisted in the document;
  - undo/redo with transactions and coalesced drags;
  - open/save/save-as, drag & drop, unsaved-change guard and window icon.
- Headless CLI: `new`, `convert`, `render`, `gui`.
- Markdown outline import/export.
- Standalone SVG export with shared layout, palettes, shadows and wrapped text.
- Core model with short stable IDs, atomic JSON persistence, schema validation
  and forward-compatible loading.
- Deterministic layouts with overlap guarantees, tested on 1,111 nodes.
- Desktop/Start Menu shortcut installer with a custom icon.
- CI for Linux, Windows and macOS; 88 tests; fmt, clippy and rustdoc checks.

### Known limitations

- No autosave, notes/colors editing UI or collapse toggle in the canvas yet.
- OPML, FreeMind, PNG and PDF are not implemented.
- MCP server and AI panel are planned for later milestones.
- Windows binaries are the only pre-built release artifacts; other platforms
  build from source.

[0.1.0]: https://github.com/Marcu08/mappamente/releases/tag/v0.1.0
