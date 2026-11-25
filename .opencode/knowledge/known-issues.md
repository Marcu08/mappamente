# Known issues and limitations

## Current version (0.1.0)

- No autosave or crash-recovery file yet.
- Node notes, colors and the collapse flag are supported by the format and
  core, but the canvas has no UI to edit them. Existing values are preserved.
- No in-canvas collapse/expand toggle; collapsed branches imported from files
  render as collapsed.
- MCP server and AI panel are not implemented.
- OPML, FreeMind and PNG/PDF export are not implemented.
- Cross-links (`links`) are reserved and validated but never rendered.
- Single-window only; opening a second file starts a second process.

## Packaging and shortcut

- The Desktop shortcut targets `target\release\mappamente-desktop.exe`. Running
  `cargo clean` or moving the repository breaks it; re-run
  `scripts/install-shortcut.ps1`.
- The custom icon is used by the window and the shortcut (`assets\mappamente.ico`),
  but it is not embedded in the `.exe` PE resources. Explorer shows the custom
  icon through the shortcut; the bare executable may show a generic icon.
- Installers, file associations, signing and notarization are future work.
- Pre-built v0.1.0 artifacts are Windows x64 only; other platforms build from
  source.

## Layout and rendering

- Radial layout guarantees non-overlapping boxes by increasing ring radii. Many
  direct children can produce a very large map; there is no compaction yet.
- Text metrics are estimated with `char_width` and `unicode-width`; the GUI and
  SVG may render glyphs slightly wider/narrower than the boxes. SVG uses the
  fixed 15 px font size / 20 px line height from `Metrics`.
- Manual positions are absolute; window resizing does not reflow them.
- Themes beyond the four built-ins fall back to dark in the UI.
- The inline editor is single-line; imported multi-line labels are flattened
  when edited.

## Interaction edge cases

- Drag-to-reparent uses the topmost non-dragged node under the pointer; dropping
  onto a branch that is behind another node may pick the visible one.
- A drag is a coalesced undo transaction; reparenting after the drag adds a
  second transaction, so undo may need two steps to restore the original parent.
- The inline edit overlay is constrained by egui when near the window edges and
  may shift slightly; the text still applies to the correct node.
- Keyboard shortcuts require the canvas/window to have focus (click once after
  launching from automation).

## Testing

- 88 tests: 75 core unit, 2 core integration, 4 GUI unit, 7 CLI integration.
  Doc tests: 0.
- The 1,111-node layout test has a generous 5-second guard in debug builds; it
  is not a micro-benchmark. Release layout measured at ~3 ms on this machine.
- GUI flows were smoke-tested manually with DPI-aware mouse/keyboard automation
  (create, edit, delete, undo, drag-reparent, save). There are no automated UI
  tests yet.
- No property-based testing yet; overlap and determinism tests use fixed
  generated trees.

## Environment

- The local shell starts with an empty `CARGO_TARGET_DIR`, which makes cargo
  fail. Set `CARGO_TARGET_DIR` explicitly when running commands.
- Windows UI automation must set thread DPI awareness before `SetCursorPos`;
  otherwise clicks are scaled and miss node rectangles.
- First release build downloads and compiles eframe/egui/wgpu and takes several
  minutes; incremental builds are much faster.
