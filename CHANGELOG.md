# Changelog

All notable changes to Mappamente are documented here.

## [0.2.0] - 2026-11-25

### Added

- **Reliable persistence:**
  - Autosave in the GUI after a short idle delay; saves are skipped when the
    document is clean or has no path.
  - Crash-recovery snapshots written while a document is dirty; the next open
    offers to restore a newer snapshot instead of silently replacing the saved
    file.
  - Recovery metadata tracks the original path and the saved `modified_at`, so
    outdated or invalid recovery files are ignored automatically.
  - Recovery is also offered when the original saved file is missing.
- **GUI/MCP synchronization:**
  - The GUI watches the open `.mappa` file and auto-reloads external changes
    when there are no local unsaved changes.
  - When external changes conflict with local unsaved edits, a dialog lets the
    user reload the file, keep the local version, or save local changes under a
    new name.
  - "Keep Local" records the ignored disk revision so autosave and later saves
    cannot silently overwrite the external version.
  - Save As uses atomic compare-and-save with an empty expected revision, so an
    existing destination cannot be silently overwritten and a destination created
    by another process between the check and the write is still detected.
  - Deleted or renamed files are surfaced in the status line.
  - File watcher stops the previous watch when switching paths and cleans up on
    drop.
- **Concurrency and conflict protection:**
  - Advisory file locking (`fs2`) on every map read and write in
    `mappamente-core`.
  - The MCP server holds the lock across the entire read-check-write mutation,
    eliminating the check-then-write race.
  - `expected_modified_at` still returns a clear `modified_at_mismatch` error for
    stale MCP clients.
  - New core helpers: `io::json::modified_at`, `io::lock::FileLock`,
    `io::recovery::write_recovery` / `read_recovery` / `cleanup_recovery`.
  - New GUI module: `file_watch::FileWatcher` backed by `notify`.
- Core: transactional `MindMap::edit`, `outline_with_ids`, Markdown subtree
  insertion with fresh ids, JSON migration hook and a 5 MiB map-file limit.
- `mappamente-mcp`: stdio MCP server with sandboxed `--dir`, ten map tools,
  optimistic concurrency via `modified_at`/`expected_modified_at`, atomic
  writes, advisory locking and tool-level errors.
- AI assistant side panel with OpenAI, Anthropic, OpenCode Console/Go/local,
  OpenRouter, Groq, DeepSeek, Mistral, Gemini, xAI, Ollama, LM Studio and custom
  OpenAI-compatible providers. Keys use the OS keyring with environment
  fallbacks; returned JSON actions are applied to the live canvas in one undo
  step.
- AI: new **OpenCode Go** provider preset for Go / Go Plus subscriptions
  (`https://opencode.ai/inference/go/openai/v1`) with the required stable
  `x-opencode-session` header. A Console 402 insufficient-balance error now
  explains that Go keys belong on the OpenCode Go provider instead.
- Visual redesign: semantic style tokens, a floating vector-icon toolbar with
  symmetric 24×24 glyphs, pill nodes with a curated accent and eight branch
  hues, branch-coloured Bézier connectors, an adaptive dot grid, animated
  selection/hover/fit, a minimal status line and a restyled AI panel with
  collapsed provider settings.
- User-local preferences for theme, grid and motion in the platform config
  directory; new/welcome maps follow the OS theme by default.
- Optional portable install: `install-shortcut.ps1 -Destination <folder>
  -Portable` copies `Mappamente.exe`, embeds the application icon into it and
  creates an icon shortcut next to it.
- SVG export now shares the branch palette, readable text colors and pill
  geometry with the canvas.
- README, design notes, file-format, architecture and decision documentation
  updates.

### Changed

- Release profile no longer aborts on panic, so the long-running MCP server and
  future tool loops can unwind.
- 176 tests, including persistence, locking, concurrent-write, file-watcher,
  recovery, Save As safety and local mock HTTP coverage for all three AI
  protocols.

### Visual changes

- Toolbar icons are drawn on a 24×24 logical grid so every glyph is symmetric,
  straight and balanced; the icon canvas is larger and the stroke is slightly
  heavier for better readability at toolbar size.  Save, Save As and the AI
  assistant icons were refined to be immediately readable.
- Node labels are vertically centered using real font metrics instead of a fixed
  line height, so single-line and wrapped text both sit cleanly inside pills.
- Trackpad pinch-to-zoom now zooms the canvas in addition to the scroll-wheel
  zoom; both zoom toward the pointer position.
- The window title dirty marker is unchanged; the bottom status line now shows
  only file state, zoom and node count.
- Layout and theme remain document properties; switching them still participates
  in undo and is saved with the map.
- SVG output changed consciously: node fills, branch-colored edges, text
  contrast and radii now match the redesigned canvas. The `.mappa` schema is
  unchanged.

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

[0.2.0]: https://github.com/Marcu08/mappamente/releases/tag/v0.2.0
[0.1.0]: https://github.com/Marcu08/mappamente/releases/tag/v0.1.0
