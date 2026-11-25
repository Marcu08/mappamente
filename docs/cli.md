# Command-line interface

`mappamente.exe` is the console binary. The MCP server is a separate binary,
`mappamente-mcp.exe`, so the CLI build does not include the MCP dependencies. The
GUI-subsystem launcher for double-click is `mappamente-desktop.exe`.

```
mappamente [COMMAND]

Commands:
  new      Create a new .mappa file
  convert  Convert between .mappa, .md and .svg
  render   Render a map to SVG
  gui      Open the editor
  help     Print help for a command
```

With no command, `mappamente` opens the editor (a console stays attached on
Windows; use `mappamente-desktop.exe` for a clean launch).

## `mappamente new`

```
mappamente new <PATH> [--title <TITLE>] [--root <TEXT>]
                 [--layout tree|radial|logical] [--force]
```

Creates a native map with one root node. `.mappa` is appended when the path has
no extension; missing parent directories are created. Existing files are only
replaced with `--force`.

```powershell
mappamente new "My ideas.mappa" --title "Ideas" --root "Central idea" --layout radial
```

## `mappamente convert`

```
mappamente convert <INPUT> <OUTPUT> [--layout ...] [--theme ...] [--force]
```

Reads `.mappa`, `.json`, `.md` or `.markdown`; writes `.mappa`, `.json`, `.md`,
`.markdown` or `.svg`. When the output has no extension, `.mappa` is used.
`--layout` and `--theme` override the values stored in the map.

Markdown conversion is intentionally lossy: labels and tree structure survive;
notes, colors, manual positions and collapsed state do not.

```powershell
mappamente convert outline.md map.mappa
mappamente convert map.mappa outline.md
mappamente convert map.mappa preview.svg --theme paper --layout radial
```

## `mappamente render`

```
mappamente render <INPUT> <OUTPUT.svg> [--layout ...] [--theme dark|light|paper|ocean]
             [--margin <PIXELS>] [--force]
```

Produces a standalone SVG with background, rounded nodes, soft shadows, curved
edges and wrapped labels. `.svg` is appended when missing. The renderer uses
the same layout and palettes as the editor.

```powershell
mappamente render docs/demo.mappa docs/screenshot.svg --theme light --layout tree
```

## `mappamente gui`

```
mappamente gui [FILE]
```

Opens the editor. It accepts `.mappa`, `.json`, `.md` and
`.markdown`; invalid files show an error banner while keeping the welcome map.
You can also drag a file onto the window or use **Open…**.

## Exit codes and errors

- `0` success;
- `1` runtime error (missing file, invalid syntax, existing output without
  `--force`, unsupported format);
- `2` argument error from the CLI parser.

Errors are printed to stderr with the failing path. Never overwrite
unintentionally: every command that writes a file refuses to replace an
existing one unless `--force` is passed.

## Desktop shortcut

From PowerShell, at the repository root:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/install-shortcut.ps1
```

Options:

- `-StartMenu` puts the shortcut in the Start Menu instead of the Desktop;
- `-Destination <folder>` chooses a folder;
- `-SkipBuild` refuses to build and fails if the executable is missing.

The shortcut targets `target\release\mappamente-desktop.exe` and uses
`assets\mappamente.ico`. Re-run the script after moving the repository or cleaning
`target`.
