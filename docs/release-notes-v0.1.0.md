# Mappamente 0.1.0

**Mind maps, charted.** The first version of Mappamente is a native,
open-source mind map editor in Rust with a headless CLI.

## Highlights

- Native editor: inline labels, `Tab` child, `Enter` sibling, `Delete` branch,
  drag to move or reparent, undo/redo, pan/zoom/fit.
- Layout and theme switching saved in the document.
- Native `.mappa` JSON with atomic writes, validation and stable IDs.
- Markdown outline import/export and standalone SVG rendering with four themes.
- CLI: `new`, `convert`, `render`, `gui`.
- Desktop shortcut installer and custom icon.
- 88 tests, CI on Linux/Windows/macOS, MIT OR Apache-2.0, no telemetry.

## Install (Windows)

Download `mappamente-desktop.exe` for the double-click editor and
`mappamente.exe` for the console CLI. Both are standalone; no installer or
runtime is required.

```powershell
.\mappamente-desktop.exe
.\mappamente.exe --help
```

Or build from source:

```powershell
cargo build --release
powershell -ExecutionPolicy Bypass -File scripts/install-shortcut.ps1
```

## Known limitations

- No autosave, notes/colors UI, collapse toggle, OPML/FreeMind or PNG/PDF yet.
- MCP server and AI panel are on the roadmap.
- Binaries attached to this release are Windows x64; other platforms build
  from source with the same commands.

## License

MIT OR Apache-2.0.
