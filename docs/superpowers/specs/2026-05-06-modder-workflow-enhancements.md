# GVG Modding Tool — Modder Workflow Enhancements

## Overview

A feature set designed to streamline the modding workflow for Gundam vs Gundam
Next Plus game assets. Covers AFS archive management, export pipelines, 3D
preview improvements, hex viewer overhaul, and tree view UX refinements.

This spec extends the existing v2 design (`2026-04-29-gvg-modding-tool-v2-design.md`)
and 3D preview design (`2026-04-29-gvg-3d-preview-and-pipeline-design.md`).

## Decisions

| # | Decision | Choice | Rationale |
|---|----------|--------|-----------|
| 1 | AFS add/delete/update strategy | Full Rebuild | PSP games reference entries by index; full rebuild keeps indexes correct and avoids fragmentation |
| 2 | AFS add — entry naming | Dialog prompt for manual input | Modders need precise control over internal AFS names |
| 3 | AFS delete — confirmation | Immediate confirmation dialog | Destructive operation; low entry count makes dialog cost negligible |
| 4 | AFS dump — output structure | Flat directory, name table names | Simple and intuitive; no manifest overhead |
| 5a | AFS dump — unnamed entries | `entry_NNN` format (no extension) | Clear fallback without assuming file type |
| 5b | AFS dump — name collisions | Append `_2`, `_3` suffix | Preserves readability, prevents data loss |
| 6 | 3D preview — default display | Always visible: empty scene + logo billboard + guide text | Immediate tool capability awareness |
| 7a | Logo billboard — on model load | Disappears when model loaded | Avoids visual interference |
| 7b | Logo billboard — asset source | Compile-time embed (`include_bytes!`) | Zero external file dependency |
| 8 | Camera controls | Left-drag orbit, right-drag pan, scroll zoom | Standard 3D viewer convention; touchpad friendly |
| 9 | Camera focus target | Frame model on load; Reset returns to world origin (0,0,0) | Best of both: immediate visibility + world reference |
| 10 | Viewport buttons | "Reset View" (origin + default angle) + "Focus Model" (frame bbox) | Two complementary operations |
| 11 | Render/view distance | Skipped — `far: 100_000.0` already sufficient | No issue observed |
| 12 | Grid sizing | Dynamic: adapts to model bounds | Universal; no fixed-size mismatch |
| 13 | Export Raw | Already implemented for all entry/stream types | Confirmed sufficient |
| 14 | Export Decrypted PZZ | Two options: XOR-only decrypt + full stream unpack to folder | Covers both analysis and extraction workflows |
| 15 | PZZ stream unpack naming | `stream_000.pmf2`, `stream_001.gim`, etc. | Index + detected extension; clear and sortable |
| 16a | Tree view root | Display AFS filename at top | File browser mental model |
| 16b | Index display | Remove `#xxxx` from tree; show in Inspector only | Cleaner tree; technical detail where it belongs |
| 17 | Hex viewer layout | Three independent columns (offset/hex/ascii) + byte-level click highlight with bidirectional sync | HxD-style interaction; solves alignment and selection issues |
| 18 | Hex viewer row format | 16 bytes/row, 4-byte grouping | HxD classic layout |
| 19 | Hex viewer scope | Available for both AFS entries and PZZ streams; shows raw bytes | Consistent; no auto-decrypt |
| 20 | Hex viewer windows | Multiple simultaneous windows | Enables side-by-side comparison |
| 21 | Empty scene grid | Default extent=50, step=5 (100x100 units) | Reasonable middle ground before model loads |

## Feature Specifications

### 1. AFS Full Rebuild — Add / Delete / Update Entries

#### Architecture

Current `patch_entries_bytes()` in `afs.rs` supports replacing existing entries
with offset shifting. Full rebuild requires a new function that constructs an
entirely new AFS binary from scratch:

```rust
pub fn rebuild_afs(entries: &[AfsRebuildEntry]) -> Result<Vec<u8>>

pub struct AfsRebuildEntry {
    pub data: Vec<u8>,
    pub name: Option<String>,
}
```

The rebuild function:
1. Writes `AFS\0` magic + new `file_count`
2. Computes entry table (offset/size pairs) with 2048-byte alignment
3. Writes all entry data sequentially
4. Writes name table (0x30 bytes per entry: 0x20 name + metadata)
5. Writes name table pointer after the entry table

#### Add Entry

- **Trigger**: Context menu on AFS root node or toolbar button
- **Flow**:
  1. File picker dialog to select the file to import
  2. Name input dialog (pre-filled with the selected filename)
  3. New entry appended to the end of the entry list
  4. Workspace marked dirty

#### Delete Entry

- **Trigger**: Context menu on any AFS entry → "Delete Entry"
- **Flow**:
  1. Confirmation dialog: "Delete entry [name]? This will change all subsequent entry indexes after save."
  2. Entry removed from workspace entry list
  3. If the deleted entry was an expanded PZZ, close it
  4. Workspace marked dirty

#### Update Entry

- Uses existing replace/patch infrastructure
- Name table editing exposed in Inspector panel as an editable text field

### 2. AFS Dump to Folder

- **Trigger**: File menu → "Dump AFS to Folder..." or context menu on AFS root
- **Flow**:
  1. Folder picker dialog
  2. Iterate all entries, read each from file
  3. Write to folder using name table names
  4. Unnamed entries: `entry_NNN` (three-digit zero-padded index, no extension)
  5. Name collisions: append `_2`, `_3`, etc. before the extension
- **Error handling**: Per-entry errors reported in status bar; continues with remaining entries

#### Collision Resolution Example

```
pl00.pzz          ← first occurrence
pl00_2.pzz        ← second occurrence with same name
entry_005          ← unnamed entry at index 5
```

### 3. Export Decrypted PZZ

Two new context menu options on PZZ-type AFS entries:

#### 3a. Export Decrypted PZZ (structure preserved)

- XOR decrypt only; output is a valid PZZ file without encryption
- File save dialog with `.pzz` extension

#### 3b. Export PZZ Streams (full unpack)

- XOR decrypt → parse descriptors → zlib decompress each stream
- Folder picker dialog
- Output files named by index + detected type:
  - `stream_000.pmf2`
  - `stream_001.gim`
  - `stream_002.sad`
  - `stream_003.bin` (unknown magic)
- Per-stream errors reported; continues with remaining streams

### 4. 3D Preview — Always Visible

#### Empty Scene State

When no model is loaded:
- Render ground grid (extent=50, step=5) + coordinate axes
- Render app logo as a billboard quad (always faces camera)
- Display centered guide text: "Select a PMF2 stream to preview"

#### Logo Billboard

- **Source**: `include_bytes!("assets/logo.png")` compiled into binary
- **Behavior**: Quad always facing camera (billboard transform)
- **Lifecycle**: Visible only when no model is loaded; disappears on model load

#### Persistence

- 3D viewport never closes, regardless of bone visibility state
- Hiding all bone meshes shows empty scene (grid + axes), NOT removes the viewport

### 5. Camera Controls

#### Input Mapping

| Input | Action |
|-------|--------|
| Left mouse drag | Orbit (rotate around target) |
| Right mouse drag | Pan (translate target in screen plane) |
| Scroll wheel | Zoom (adjust distance to target) |

#### Pan Implementation

Add to `PreviewCamera`:

```rust
pub fn pan(&mut self, delta_x: f32, delta_y: f32) {
    let basis = self.basis();
    let scale = self.distance * 0.002;
    self.target[0] += basis.right[0] * delta_x * scale + basis.up[0] * delta_y * scale;
    self.target[1] += basis.right[1] * delta_x * scale + basis.up[1] * delta_y * scale;
    self.target[2] += basis.right[2] * delta_x * scale + basis.up[2] * delta_y * scale;
}
```

#### Buttons

| Button | Action |
|--------|--------|
| Reset View | Camera target → (0,0,0), yaw/pitch/distance → defaults |
| Focus Model | Camera target → model bbox center, distance → fit bounds |

#### Focus Behavior

- **On model load**: Automatically frame to model bbox center (same as Focus Model)
- **Reset View**: Target (0,0,0), yaw = PI + 0.35, pitch = -0.28, distance = default
- **Focus Model**: Target = bbox center, distance = `radius / tan(fov/2)`

### 6. Dynamic Grid

Grid adapts to loaded model bounds:

```rust
fn compute_grid_params(bounds: Option<&PreviewBounds>) -> (f32, f32) {
    let (extent, step) = match bounds {
        Some(b) => {
            let max_dim = [
                (b.max[0] - b.min[0]).abs(),
                (b.max[1] - b.min[1]).abs(),
                (b.max[2] - b.min[2]).abs(),
            ].into_iter().fold(0.0_f32, f32::max);
            let extent = (max_dim * 1.5).max(10.0);
            let step = (extent / 20.0).max(0.5);
            (extent, step)
        }
        None => (50.0, 5.0),  // empty scene default
    };
    (extent, step)
}
```

Grid must be regenerated (GPU buffer recreated) when model changes.

### 7. Tree View Changes

#### AFS Root Node

Non-selectable header row at the top of the tree:

```
Z_DATA.BIN (1234 entries)
├── pl00.pzz (3.2 MB)
│   ├── [PMF2] stream000 (256 KB)
│   └── [GIM]  stream001 (128 KB)
├── pl01.pzz (3.1 MB)
└── ...
```

#### Index Removal

- Remove `#{:04}` prefix from all tree rows
- AFS entry format: `{validation_icon}{expand_icon}{name} ({size}){dirty_mark}`
- Entry index displayed in Inspector panel under entry metadata

#### AFS Entry Context Menu (Updated)

For PZZ entries:
- Open PZZ
- Export Raw
- Export Decrypted PZZ
- Export PZZ Streams...
- Delete Entry
- ---
- View Hex

For non-PZZ entries:
- Export Raw
- Delete Entry
- ---
- View Hex

Root node context menu:
- Dump AFS to Folder...
- Add Entry...

### 8. Hex Viewer Overhaul

#### Layout — Three Independent Columns

```
┌──────────┬─────────────────────────────────────────┬──────────────────┐
│ Offset   │ Hex                                     │ ASCII            │
├──────────┼─────────────────────────────────────────┼──────────────────┤
│ 00000000 │ 504D4632 04000000 28000000 00000000     │ PMF2....(......  │
│ 00000010 │ 8C3DF03D 295C8D3D 00000000 00000000     │ .<.=)\..=......  │
└──────────┴─────────────────────────────────────────┴──────────────────┘
```

- **Row format**: 16 bytes per row, 4-byte groups (no space between bytes within a group)
- **Columns**: Offset (8 hex chars) | Hex (4 groups of 8 hex chars) | ASCII (16 chars)
- **Each column independently selectable** for clean copy behavior

#### Byte-Level Interaction

- Click a hex byte → highlight corresponding ASCII char (and vice versa)
- Drag-select a range → synchronized highlight in both hex and ASCII columns
- Highlight color distinguishes primary (clicked column) from secondary (linked column)

#### Implementation Approach

Use `egui_extras::TableBuilder` with three columns:
1. Offset column: fixed width, monospace `Label`
2. Hex column: per-byte clickable elements with group spacing
3. ASCII column: per-byte clickable elements

State tracked in a `HexViewerState` struct per window:

```rust
pub struct HexViewerState {
    pub target: HexViewTarget,       // AfsEntry(index) or Stream(index)
    pub selection_start: Option<usize>,  // byte offset
    pub selection_end: Option<usize>,    // byte offset (drag range)
}
```

#### Multi-Window Support

Change `EditorWindows`:

```rust
// Before:
pub hex_view: Option<usize>,

// After:
pub hex_views: Vec<HexViewerState>,
```

Each window has an independent title derived from its target:
- AFS entry: `"Hex View - {entry_name}"`
- PZZ stream: `"Hex View - stream{:03}"`

### 9. Inspector — Index Display

When an AFS entry is selected, Inspector shows:

```
── Entry Info ──
Name: pl00.pzz
Index: 42
Offset: 0x00A0_0000
Size: 631,168 bytes (616.4 KB)
Kind: PZZ
Validation: OK
```

Index is displayed here instead of in the tree view.

#### Name Editing

The entry name field is an editable `TextEdit::singleline`. On losing focus or
pressing Enter, the new name is written back to the workspace's `AfsEntryNode`.
Name changes mark the workspace as dirty. The name is limited to 0x20 bytes
(AFS name table constraint); longer names are truncated with a warning in the
status bar.

## AFS Entry Right-Click Menu Summary

| Entry Kind | Menu Items |
|------------|------------|
| PZZ | Open PZZ, Export Raw, Export Decrypted PZZ, Export PZZ Streams..., Delete Entry, ---, View Hex |
| Non-PZZ | Export Raw, Delete Entry, ---, View Hex |
| Root node | Dump AFS to Folder..., Add Entry... |

## File Impact

| File | Change |
|------|--------|
| `afs.rs` | Add `rebuild_afs()`, `AfsRebuildEntry`; add `xor_decrypt_pzz()` export helper |
| `pzz.rs` | Expose `xor_decrypt()` as public for decrypted PZZ export |
| `workspace.rs` | Add entry add/delete operations; track pending AFS modifications |
| `gui.rs` | Wire new actions; manage multiple hex viewer windows; AFS dump flow |
| `gui/asset_tree.rs` | AFS root node; remove `#xxxx`; new context menu items; add/delete actions |
| `gui/inspector.rs` | Show entry index; editable name field |
| `gui/preview.rs` | Always-visible viewport; logo billboard; pan controls; Reset View / Focus Model buttons |
| `gui/editors.rs` | Hex viewer rewrite: three-column layout, byte interaction, multi-window |
| `render.rs` | Add `pan()` to `PreviewCamera`; `reset_view()` method; `frame_bounds()` unchanged |
| `gpu_renderer.rs` | Dynamic grid regeneration; logo billboard rendering; grid param computation |
| `shaders/mesh.wgsl` | Billboard vertex shader variant for logo quad |

## Implementation Order

### Phase 1: Tree View & Export (low risk, high visibility)
1. AFS root node in tree view
2. Remove `#xxxx` from tree, add index to Inspector
3. Export Decrypted PZZ (XOR-only)
4. Export PZZ Streams (full unpack to folder)
5. AFS Dump to Folder

### Phase 2: AFS Management (high impact, needs careful testing)
6. AFS Full Rebuild engine (`rebuild_afs()`)
7. Add Entry workflow (dialog + rebuild)
8. Delete Entry workflow (confirmation + rebuild)
9. Hex View for AFS entries

### Phase 3: 3D Preview Enhancements
10. Always-visible 3D viewport with empty scene
11. Logo billboard (compile-time embed)
12. Right-click pan camera control
13. Reset View / Focus Model buttons
14. Dynamic grid sizing

### Phase 4: Hex Viewer Overhaul
15. Three-column layout with `TableBuilder`
16. 4-byte grouping display
17. Byte-level click/highlight with bidirectional sync
18. Multi-window support

## Risks

- **AFS Full Rebuild correctness**: Rebuilding changes every offset in the archive.
  Must validate with real game loading. Mitigation: always write to a new file,
  never overwrite the original; add a byte-level comparison test against known-good
  AFS files.
- **Hex viewer performance**: Per-byte clickable elements in egui may be slow for
  large files (>1 MB). Mitigation: virtual scrolling limits visible rows; only
  render interactive elements for visible bytes.
- **Logo billboard texture**: Requires loading a PNG at startup and uploading to
  GPU. Must handle the case where wgpu device is not yet ready during init.
- **Dynamic grid GPU buffer churn**: Recreating the grid vertex buffer on every
  model change. Mitigation: only regenerate when bounds actually change; grid
  vertex count is small (<1000 vertices).
- **Multi-window hex viewer memory**: Each open hex view holds a reference to
  potentially large byte data. Mitigation: hex viewers read from workspace data
  (shared reference), not copies.
