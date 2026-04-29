# GVG Modding Tool — 3D Preview & Pipeline Design

## Problems

### P1: PMF2 Preview is Software-Rendered Wireframe Only
Current `render.rs` performs all rendering on CPU:
- Projects every vertex through manual matrix math per frame
- Sorts triangles by depth (painter's algorithm) on CPU
- Draws semi-transparent polygons + wireframe lines via egui's `Painter`
- No depth buffer, no lighting, no textures
- Result: looks like wireframe, not a 3D model

### P2: Performance is Unacceptable
With 3000+ triangle meshes, the CPU-based projection loop runs every frame:
- `project_point()` called 3× per triangle per frame
- Triangle depth-sort is O(n log n) per frame
- All drawing through egui shape primitives (no GPU batching)
- Result: UI freezes on interaction

### P3: PMF2 ↔ DAE Pipeline Incomplete
- Export DAE works but needs validation
- Import DAE → PMF2 replacement needs full roundtrip testing
- No auto-loading of GIM textures alongside PMF2
- No texture replacement workflow
- Custom operations (bone editing, section manipulation) not exposed

## Architecture Decision: eframe wgpu Backend + Off-screen Rendering

### Why NOT wgsl_to_wgpu
`wgsl_to_wgpu` is a build-time codegen tool that generates Rust bindings from
WGSL shaders. It adds build complexity and is overkill for our simple mesh+texture
shader. Direct wgpu usage with hand-written WGSL is simpler and sufficient.

### Why wgpu via eframe (same as ssbh_editor)
- eframe already supports wgpu as a rendering backend (feature flag)
- `egui_wgpu::CallbackTrait` + `PaintCallback` enables custom GPU rendering
  inside egui panels — the exact pattern ssbh_editor uses
- No additional rendering framework needed
- Full control over shaders, depth buffer, textures

### Off-screen Rendering (Required for Depth Testing)
egui's main render pass has no depth-stencil attachment. For proper 3D:
1. Create off-screen color texture + depth texture at viewport size
2. Render mesh in a custom render pass with depth testing enabled
3. Register color texture with egui as `TextureId`
4. Display as `egui::Image` in CentralPanel
5. Recreate textures when viewport resizes

## Component Specifications

### 1. Cargo.toml Changes

```toml
eframe = { version = "0.33.3", default-features = false, features = ["wgpu", "default_fonts", "x11", "wayland"] }
egui-wgpu = "0.33.3"
wgpu = "0.25"
bytemuck = { version = "1", features = ["derive"] }
```

Remove: implicit glow dependency (switching renderer)

### 2. WGSL Shader — `mesh.wgsl`

```wgsl
struct Uniforms {
    mvp: mat4x4<f32>,
    model: mat4x4<f32>,
    light_dir: vec3<f32>,
    ambient: f32,
    camera_pos: vec3<f32>,
    _pad: f32,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(1) @binding(0) var diffuse_texture: texture_2d<f32>;
@group(1) @binding(1) var diffuse_sampler: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) bone_index: u32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) world_pos: vec3<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = u.mvp * vec4(in.position, 1.0);
    out.world_normal = normalize((u.model * vec4(in.normal, 0.0)).xyz);
    out.uv = in.uv;
    out.world_pos = (u.model * vec4(in.position, 1.0)).xyz;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let tex_color = textureSample(diffuse_texture, diffuse_sampler, in.uv);
    let n = normalize(in.world_normal);
    let diffuse = max(dot(n, normalize(u.light_dir)), 0.0);
    let lighting = u.ambient + (1.0 - u.ambient) * diffuse;
    return vec4(tex_color.rgb * lighting, tex_color.a);
}
```

Two render pipelines:
- **Solid**: fill with lighting + optional texture
- **Wireframe**: overlay with line topology (or use polygon offset + line draw)

### 3. GPU Vertex Format

```rust
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuVertex {
    position: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
    bone_index: u32,
    _pad: u32,
}
```

### 4. Render Resources — `gpu_renderer.rs` (new file)

```rust
pub struct GpuRenderer {
    solid_pipeline: wgpu::RenderPipeline,
    wireframe_pipeline: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
    default_texture_bind_group: wgpu::BindGroup,
    // Off-screen targets (recreated on resize)
    color_texture: Option<wgpu::TextureView>,
    depth_texture: Option<wgpu::TextureView>,
    viewport_size: [u32; 2],
    egui_texture_id: Option<egui::TextureId>,
}

pub struct GpuMesh {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
    texture_bind_group: Option<wgpu::BindGroup>,
}
```

Lifecycle:
- `GpuRenderer::new(device, queue, target_format)` — create pipelines, buffers
- `GpuRenderer::upload_mesh(device, vertices, indices)` → `GpuMesh`
- `GpuRenderer::upload_texture(device, queue, rgba, w, h)` → bind group
- `GpuRenderer::render(encoder, camera, meshes, viewport)` — off-screen pass
- `GpuRenderer::ensure_viewport(device, renderer, w, h)` — resize textures

### 5. Camera System (reuse existing PreviewCamera)

```rust
pub struct Uniforms {
    mvp: [[f32; 4]; 4],
    model: [[f32; 4]; 4],
    light_dir: [f32; 3],
    ambient: f32,
    camera_pos: [f32; 3],
    _pad: f32,
}
```

Camera → view matrix → projection matrix → MVP computed on CPU,
uploaded to uniform buffer. Only updated on camera interaction (not every frame).

### 6. Integration with egui — Preview Panel

Two approaches (prefer A):

**A) Off-screen + egui::Image (recommended)**
```
1. In update(), check if viewport size changed → resize off-screen textures
2. If mesh is dirty or camera changed → re-render off-screen
3. Display color texture as egui::Image in CentralPanel
4. Handle mouse input for orbit/zoom
```

**B) PaintCallback (simpler but no depth buffer)**
Only works for very simple scenes. Not recommended for mesh rendering.

### 7. Auto-Loading GIM Textures

When a PMF2 stream is selected for preview:
1. Check sibling streams in the same PZZ for GIM textures
2. PZZ convention: stream pairs are (PMF2@0, GIM@1), (PMF2@2, GIM@3), etc.
3. Decode GIM → RGBA pixels → upload as wgpu texture
4. Bind to the mesh's texture slot

Stream pairing logic:
```rust
fn find_texture_stream(pmf2_index: usize, streams: &[StreamNode]) -> Option<usize> {
    let gim_index = pmf2_index + 1;
    streams.get(gim_index)
        .filter(|s| s.kind == AssetKind::Gim)
        .map(|_| gim_index)
}
```

### 8. Rendering Style (Blender / ssbh_editor reference)

Visual targets:
- **Background**: dark gray gradient (#2D2D2D → #1A1A1A)
- **Mesh fill**: diffuse lit with directional light from upper-right-front
- **Wireframe overlay**: toggle-able, thin dark lines over solid mesh
- **Grid**: XZ plane grid (optional, phase 2)
- **Axes**: RGB colored axis lines at origin
- **Bounds box**: gray wireframe box around model
- **Selection highlight**: selected bone meshes get brighter outline

### 9. PMF2 ↔ DAE Pipeline Improvements

**Current state** (from codebase analysis):
- `pmf2::extract_per_bone_meshes()` → `Vec<BoneMeshData>` (vertices, faces, UVs, normals)
- `dae::write_dae()` → writes COLLADA XML with per-bone geometry
- `dae::read_dae_to_meta()` → reads COLLADA back to `Pmf2Meta`
- `pmf2::patch_pmf2_with_mesh_updates()` → patches original PMF2 binary with new mesh data

**Required improvements:**
1. Roundtrip validation: export → import → compare vertex/face counts
2. UV preservation during import (verify UV mapping survives roundtrip)
3. Normal recalculation option during import
4. Texture reference preservation (GIM stream index stored in meta)
5. Batch export: export all PMF2+GIM pairs from a PZZ at once

### 10. Custom Operations (Context Menu Extensions)

**PMF2 operations:**
- Recalculate normals (smooth/flat)
- Scale model (uniform/per-axis)
- Mirror model (X/Y/Z axis)
- Merge duplicate vertices
- View/edit bone hierarchy

**GIM operations:**
- Resize texture
- Convert format (indexed ↔ direct color)
- Batch export all textures

**PZZ operations:**
- Batch export all streams
- Reorder streams
- Add/remove streams

## Implementation Phases

### Phase 1: wgpu Foundation (this session)
1. Switch eframe to wgpu backend
2. Create WGSL shader
3. Implement GpuRenderer with off-screen rendering
4. Basic mesh rendering with flat shading (no textures yet)
5. Camera orbit/zoom working

### Phase 2: Textures & Polish
1. Auto-load GIM textures
2. Blender-style visual polish (background, grid, lighting)
3. Wireframe overlay toggle
4. Bone visibility filtering on GPU (via bone_index discard)

### Phase 3: Pipeline & Operations
1. PMF2 ↔ DAE roundtrip validation
2. Batch operations
3. Custom operations in context menus
4. Error handling improvements

## File Impact

| File | Change |
|------|--------|
| `Cargo.toml` | Switch to wgpu, add bytemuck |
| `gui.rs` | App initialization with wgpu_render_state |
| `gui/preview.rs` | Rewrite: off-screen wgpu rendering |
| `render.rs` | Keep camera math, remove CPU projection |
| `gpu_renderer.rs` | New: wgpu pipeline, mesh upload, off-screen render |
| `shaders/mesh.wgsl` | New: vertex/fragment shaders |

## Risks

- **wgpu compatibility**: Windows GPU driver issues (mitigated: eframe already
  validates adapter selection)
- **Off-screen texture management**: Must handle resize correctly to avoid
  GPU memory leaks
- **GIM texture formats**: Some indexed GIM textures may not decode cleanly;
  need fallback to white texture
- **Performance with very large meshes**: 755 KB SAD streams won't be previewed;
  only PMF2 streams go to GPU
