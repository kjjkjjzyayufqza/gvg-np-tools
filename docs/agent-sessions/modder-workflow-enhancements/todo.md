# Modder Workflow Enhancements — TODO

Spec: `docs/superpowers/specs/2026-05-06-modder-workflow-enhancements.md`

## Phase 1: Tree View & Export — DONE

- [x] 1.1 AFS root node in tree view (filename + entry count)
- [x] 1.2 Remove `#xxxx` from tree, add index to Inspector + editable name field
- [x] 1.3 Updated context menus (PZZ: Open/Export Raw/Decrypt/Streams/Delete/Hex)
- [x] 1.4 Export Decrypted PZZ (XOR-only)
- [x] 1.5 Export PZZ Streams (full unpack to folder)
- [x] 1.6 AFS Dump to Folder (with collision resolution)

## Phase 2: AFS Management — DONE

- [x] 2.1 AFS Full Rebuild engine (`rebuild_afs()`) with test
- [x] 2.2 Add Entry stub (TreeAction wired, dialog not yet connected)
- [x] 2.3 Delete Entry stub (TreeAction wired, dialog not yet connected)

## Phase 3: 3D Preview — DONE

- [x] 3.1 Right-click pan camera control (wired to input)
- [x] 3.2 "Focus Model" replaces "Frame" button
- [x] 3.3 "Reset View" button added
- [x] 3.4 Dynamic grid (adapts to model bounds)

## Phase 4: Hex Viewer Overhaul — DONE

- [x] 4.1 Three-column layout (offset | hex 4-byte groups | ascii)
- [x] 4.2 Byte-level click/drag selection with bidirectional highlight
- [x] 4.3 Multi-window support (Vec<HexViewerState>)

## Remaining (Lower Priority / Future)

- [ ] Logo billboard (needs placeholder PNG asset)
- [ ] Always-visible 3D viewport when no model loaded (empty scene)
- [ ] Add Entry dialog UI (file picker + name input)
- [ ] Delete Entry confirmation dialog
- [ ] AFS entry hex view (load entry bytes into hex viewer)
- [ ] Connect `rebuild_afs()` to Save workflow
