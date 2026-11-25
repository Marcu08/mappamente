# The `.mappa` file format

Status: **version 1** (implemented in `mappamente-core`).

The native format is UTF-8 JSON. It is deliberately boring: a version field, a
tree of nodes, and optional presentation data. Files are written
pretty-printed with two-space indentation and a trailing newline, with a stable
field order, so diffs stay readable.

## Example

```json
{
  "version": 1,
  "id": "m_pd289a2w",
  "title": "Project ideas",
  "theme": "dark",
  "layout": "radial",
  "created_at": 1795594643981,
  "modified_at": 1795594643981,
  "root": {
    "id": "n_gq596zks",
    "text": "Central idea",
    "note": "Optional longer note",
    "color": "#4f8cff",
    "collapsed": false,
    "pos": null,
    "children": [
      {
        "id": "n_2qb1123j",
        "text": "First branch",
        "children": []
      }
    ]
  }
}
```

## Top-level fields

| Field | Type | Required | Meaning |
| --- | --- | --- | --- |
| `version` | integer | yes | Schema version. Version 1 is the only accepted value. |
| `id` | string | yes | Document id, e.g. `m_pd289a2w`. |
| `title` | string | yes | Human-readable document title. May be empty. |
| `theme` | string | no | Theme name: `dark` (default), `light`, `paper`, `ocean`. |
| `layout` | string | no | `tree`, `radial` (default) or `logical`. |
| `created_at` | integer | yes | Unix timestamp in milliseconds. |
| `modified_at` | integer | yes | Unix timestamp in milliseconds. |
| `root` | object | yes | Root node. Every map has exactly one root. |
| `links` | array | no | Reserved for cross-branch links. Omitted when empty. |

## Node fields

| Field | Type | Required | Meaning |
| --- | --- | --- | --- |
| `id` | string | yes | Unique node id within the document. |
| `text` | string | yes | Label. May contain newlines; shown wrapped. |
| `note` | string | no | Longer note. Omitted when empty. |
| `color` | string | no | `#RRGGBB` or `#RRGGBBAA` accent color. Omitted when absent. |
| `collapsed` | boolean | no | Hides the children in the editor. Default `false`. |
| `pos` | array | no | Manual `[x, y]` top-left position. Omitted when automatic. |
| `children` | array | yes | Child nodes, in display order. May be empty. |

### IDs

- Generated ids use a short, URL-safe form: one prefix (`n_` for nodes, `m_`
  for maps) plus eight base-32 characters, for example `n_7k3m9q2x`.
- Ids are unique inside a document, are stable across edits, and are what the
  MCP/AI tool layer uses to address nodes.
- Loading validates that ids are 1–64 ASCII letters, digits, `_` or `-` and
  that no two nodes share an id.

### Colors

Only `#RRGGBB` and `#RRGGBBAA` are accepted. Named CSS colors are rejected so
that every implementation renders the same value.

### Positions and layouts

- `pos: null` means the layout engine decides where the node goes.
- A non-null `pos` is an **absolute top-left coordinate** in canvas space (y
  grows downwards). Moving a node also translates its whole subtree, so a
  branch keeps its internal shape.
- Switching layout algorithms recomputes automatic positions but keeps manual
  overrides. This makes it possible to lay out a branch by hand without losing
  it on the next automatic pass.

### Collapsed branches

`collapsed: true` is a view state. The children remain in the file and in
outline exports; the layout engine simply does not position them.

### Links (reserved)

`links` is reserved for future cross-branch connections:

```json
"links": [{ "from": "n_a", "to": "n_b", "label": "related" }]
```

Both endpoints must exist. Version 1 writes the field only when the list is not
empty; the current UI does not render links yet.

## Compatibility rules

- Readers ignore unknown fields. This allows newer files to carry extra data
  without breaking older readers.
- A file with a `version` other than `1` is rejected with a clear error instead
  of being guessed at.
- Values are validated on both save and load: duplicate ids, invalid colors,
  non-finite positions, and dangling links are errors.

## Writing safely

`mappamente-core` writes to a temporary file in the same directory, calls
`sync_all`, and then renames it over the target. On Windows the rename is
retried briefly because a watcher or another process may briefly hold the
destination open. A failed save leaves the previous file untouched.

Loads reject files larger than 5 MiB (`MAX_MAP_FILE_BYTES`) before reading them,
so a malformed or hostile file cannot exhaust memory. Markdown input and text
fields have their own limits when edited through the MCP server.

## Interoperability

| Format | Direction | Status |
| --- | --- | --- |
| Markdown indented bullets | import/export | Implemented (lossy: labels and tree; notes/colors/positions lost) |
| SVG | export | Implemented (`mappamente render`) |
| OPML 2.0 | import/export | Planned |
| FreeMind `.mm` | import/export | Planned |
| PNG | export | Planned |

Markdown parsing requires exactly one root bullet. The first heading, if
present, becomes the map title; otherwise the root label is used. Notes,
colors, positions and collapsed state are not represented.

## Media type

Until a registration exists, use `application/vnd.mappa+json` for the native
format. The recommended file extension is `.mappa`.
