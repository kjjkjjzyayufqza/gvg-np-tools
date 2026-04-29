# GVG Modding Tool v2 — Design Specification

## Overview

A complete rewrite of the GVG Modding Tool GUI, addressing five critical issues:
performance when loading large AFS files, non-virtualized asset list, broken PZZ
expansion, missing search/context-menu UX, and cluttered tab-based layout.

The tool targets mod developers working with Gundam vs Gundam Next Plus game
assets (AFS containers → PZZ archives → PMF2 models / GIM textures).

## Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| AFS loading strategy | Header-only + on-demand read | No 595 MB memory spike; per-entry validation; AFS format is random-access by design |
| Editor presentation | Hybrid: right inspector + popup windows | Quick browsing in inspector; deep editing in dedicated windows |
| Central panel | Pure 3D preview (ssbh_editor style) | Modders need constant visual feedback while editing |
| Dock tabs | Removed (egui_dock removed) | Context menus + inspector replace all tab functions |
| UI reference | ssbh_editor panel layout pattern | Proven egui architecture for game asset editors |

## Architecture

### Module Layout

```
src/
├── lib.rs              # public module declarations
├── main.rs             # CLI entry point (unchanged)
├── bin/
│   └── gvg_modding_tool.rs  # GUI entry point
├── afs.rs              # AFS parser (add seek-based read_entry_from_file)
├── pzz.rs              # PZZ decrypt/compress (unchanged)
├── pmf2.rs             # PMF2 parser/rebuilder (unchanged)
├── dae.rs              # DAE import/export (unchanged)
├── texture.rs          # GIM decode/encode (unchanged)
├── save.rs             # PZZ/AFS save planner (unchanged)
├── workspace.rs        # Workspace state model (rewrite)
├── gui.rs              # App shell + panel layout (rewrite)
├── gui/
│   ├── asset_tree.rs   # Left panel: search + virtual list + context menus
│   ├── inspector.rs    # Right panel: context-sensitive metadata display
│   ├── preview.rs      # Central panel: 3D preview (extracted from gui.rs)
│   ├── editors.rs      # Popup windows: PMF2/GIM/Hex/SavePlanner editors
│   └── status.rs       # Bottom panel: status + validation
└── render.rs           # 3D projection (unchanged)
```

### Data Flow

```
User opens AFS file
  → afs::scan_inventory_from_file(path)     // reads only header (~4 KB)
  → workspace stores path + Vec<AfsEntryNode>  // no file content in memory
  → left panel shows virtualized entry list

User clicks PZZ entry
  → afs::read_entry_from_file(path, offset, size)  // seek + read_exact
  → pzz::extract_pzz_streams_strict(bytes)
  → workspace stores PzzWorkspace with expanded streams
  → left panel shows stream children under PZZ node

User right-clicks PMF2 stream → "View Metadata"
  → opens PMF2 Metadata egui::Window
  → parses PMF2 on demand from stream bytes already in PzzWorkspace

User right-clicks GIM stream → "Export PNG"
  → decode GIM → file dialog → save PNG
```

## Component Specifications

### 1. AFS Loading — Header-only + On-demand Read

**New function in `afs.rs`:**

```rust
pub fn scan_inventory_from_file(path: &Path) -> Result<AfsInventory>
```

- Opens file, reads first 8 bytes (magic + file_count)
- Reads entry table: `file_count * 8` bytes from offset 8
- Reads name table descriptor: 8 bytes at `8 + file_count * 8`
- Reads name table (typically a few KB)
- Closes file handle; stores only parsed metadata

```rust
pub fn read_entry_from_file(path: &Path, offset: usize, size: usize) -> Result<Vec<u8>>
```

- Opens file, seeks to offset, reads exactly `size` bytes
- Used when user clicks an entry to open it

**Entry validation at scan time (from header data only):**

- `offset + size > file_length` → mark entry as `Invalid("exceeds file bounds")`
- `offset == 0 && size == 0` → mark as `Empty`
- Overlapping entries → mark as `Warning("overlaps with entry N")`
- Name table entry missing → mark as `Unnamed` (assign synthetic name)

**`ModWorkspace` changes:**

- Remove `afs_data: Option<Vec<u8>>`
- Add `afs_path: Option<PathBuf>` (already exists, reuse)
- `open_pzz_entry` reads from file on demand instead of slicing in-memory data

### 2. Left Panel — Asset Tree

**Layout (top to bottom):**

1. Search box: `ui.text_edit_singleline(&mut self.search_filter)` with hint text
2. Virtualized entry list via `ScrollArea::show_rows()`
3. PZZ stream children rendered inline with indentation

**Virtual list implementation:**

```rust
let filtered: Vec<&AfsEntryNode> = entries.iter()
    .filter(|e| search_matches(&self.search_filter, &e.name))
    .collect();
let row_height = 20.0;
ScrollArea::vertical().show_rows(ui, row_height, filtered.len(), |ui, range| {
    for &entry in &filtered[range] {
        // render single row
    }
});
```

When a PZZ entry is expanded, its stream children are inserted into the flat
list (each stream is a separate row with extra indentation). The total row count
includes both AFS entries and expanded stream children.

**Context menus — `response.context_menu()`:**

AFS entry (PZZ kind):
- Open PZZ → triggers on-demand read + PZZ extraction
- Export Raw → file dialog, write raw entry bytes
- Properties → select entry, show details in inspector

PZZ stream (PMF2):
- Preview 3D → update central preview
- Export DAE → PMF2→DAE pipeline + file dialog
- Replace from DAE → file dialog + DAE→PMF2 pipeline, marks stream dirty
- View Metadata → open PMF2 Metadata popup window
- View Data → open PMF2 Data popup window
- Export Raw → file dialog

PZZ stream (GIM):
- Preview Texture → open GIM Preview popup window
- Export PNG → GIM→PNG + file dialog
- Replace from PNG → file dialog + PNG→GIM, marks stream dirty
- Export Raw → file dialog

PZZ stream (other):
- View Hex → open Hex Viewer popup window
- Export Raw → file dialog

**Visual indicators on list rows:**

- PZZ entries: folder icon or `▶`/`▼` expand indicator
- Stream type: `[PMF2]` `[GIM]` `[SAD]` `[BIN]` type tag
- Dirty state: `*` suffix or colored dot
- Validation error: red/yellow icon prefix

### 3. Central Panel — 3D Preview

Extracted from current `GvgTabViewer::preview_3d` into `gui/preview.rs`.

- Always visible in `CentralPanel::default()`
- Auto-updates when selected stream is PMF2
- Shows "Select a PMF2 stream to preview" when nothing applicable is selected
- Camera controls overlay at top: Frame / Wireframe / Axes / Bounds checkboxes
- Bone visibility collapsing section at bottom-left corner

No changes to rendering logic (`render.rs`), only extraction from the dock tab
system into a standalone panel function.

### 4. Right Panel — Inspector

Context-sensitive display based on current selection:

**AFS Entry selected:**
```
Entry: pl00.pzz
Index: 42
Offset: 0x00A0_0000
Size: 631,168 bytes
Kind: PZZ
Validation: OK ✓
```

**PMF2 Stream selected:**
```
Stream: stream000.pmf2
Size: 45,312 bytes
Dirty: No

── PMF2 Summary ──
BBox Scale: 1.234, 5.678, 9.012
Sections: 24
Bones with mesh: 12
Total vertices: 3,456
Total faces: 1,234
```

**GIM Stream selected:**
```
Stream: stream001.gim
Size: 32,768 bytes
Dirty: No

── GIM Summary ──
Dimensions: 128×128
Format: Indexed8
Swizzled: Yes
[thumbnail preview]
```

**Other Stream selected:**
```
Stream: stream002.bin
Size: 1,024 bytes
Magic: 0x53414420 ("SAD ")

── Hex Preview (first 256 bytes) ──
00000000: 53 41 44 20 ...
```

### 5. Popup Editor Windows

Each is an `egui::Window::new(title).open(&mut is_open).show()`:

**PMF2 Metadata Editor:**
- Section tree with collapsible bone nodes
- Per-section: index, parent, has_mesh, offset, size, category
- BBox display

**PMF2 Data Viewer:**
- Per-bone-mesh collapsible: vertex count, face count, UV, normals, vtypes
- Scrollable vertex/face tables (virtualized)

**GIM Preview Window:**
- Full-size texture rendered as `egui::Image`
- Export PNG / Replace from PNG buttons
- Metadata sidebar: format, dimensions, palette info

**Hex Viewer:**
- 16 bytes per row, up to full stream size
- Virtualized rows via `show_rows()`
- Offset column + hex + ASCII

**Save Planner Window:**
- PZZ rebuild preview: original size, rebuilt size, changed streams, tail status
- AFS patch impact: which entries shift, alignment changes
- Validation messages list
- "Save PZZ As" / "Patch AFS Entry" / "Save AFS As" action buttons

### 6. Bottom Status Bar

Single `TopBottomPanel::bottom`:

```
Status: Loaded Z_DATA.BIN (1234 entries) | Warnings: 3 ⚠
```

- Left: current operation status text
- Right: warning/error count badge; click opens validation detail popup

### 7. Menu Bar

```
File:
  Open AFS/BIN...        Ctrl+O
  Open PZZ...
  ─────────────
  Save PZZ As...         Ctrl+S
  Patch AFS Entry...
  Save AFS As...
  ─────────────
  Exit

Edit:
  (reserved for future: Undo/Redo)

View:
  Show Left Panel        ✓
  Show Right Panel       ✓
  Dark Mode              ✓
```

### 8. Dependencies Change

**Remove:**
- `egui_dock` — no longer needed

**Keep unchanged:**
- `eframe`, `egui`, `rfd`, `image`, `flate2`, `serde`, `serde_json`, `clap`,
  `anyhow`, `fbxcel`, `roxmltree`

No new dependencies added.

## File Impact Summary

| File | Change |
|------|--------|
| `Cargo.toml` | Remove `egui_dock` |
| `afs.rs` | Add `scan_inventory_from_file`, `read_entry_from_file` |
| `workspace.rs` | Rewrite: remove `afs_data`, add file-path-based loading |
| `gui.rs` | Rewrite: new panel layout, remove dock system |
| `gui/asset_tree.rs` | New: search + virtual list + context menus |
| `gui/inspector.rs` | New: context-sensitive right panel |
| `gui/preview.rs` | New: extracted 3D preview panel |
| `gui/editors.rs` | New: popup windows for PMF2/GIM/Hex/SavePlanner |
| `gui/status.rs` | New: bottom status bar |
| `render.rs` | Unchanged |
| `pmf2.rs` | Unchanged |
| `pzz.rs` | Unchanged |
| `dae.rs` | Unchanged |
| `texture.rs` | Unchanged |
| `save.rs` | Unchanged |
| `main.rs` | Unchanged |

## Risks

- **PZZ key detection failure**: Some AFS entries labeled `.pzz` may use unknown
  encryption. Error must be clearly shown per-entry, not silently swallowed.
- **GIM indexed format replacement**: Currently unsupported (correctly errors).
  The popup should explain why and suggest workarounds.
- **Large PZZ entries**: Some PZZ entries may be tens of MB. Stream extraction
  should not block the UI — consider showing a brief "Extracting..." status.
- **File locking on Windows**: On-demand file reads must handle the case where
  the AFS file is locked by another process or has been moved/deleted since
  opening.
