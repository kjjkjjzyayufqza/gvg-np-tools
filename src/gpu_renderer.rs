use crate::pmf2::BoneMeshData;
use crate::render::{PreviewBounds, PreviewCamera};
use eframe::egui;
use eframe::egui_wgpu::wgpu;
use eframe::egui_wgpu::wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuVertex {
    position: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct LineVertex {
    position: [f32; 3],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    mvp: [[f32; 4]; 4],
    model: [[f32; 4]; 4],
    light_dir: [f32; 3],
    ambient: f32,
    camera_pos: [f32; 3],
    use_texture: f32,
}

pub struct GpuMesh {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
    pub bounds: PreviewBounds,
}

pub struct GpuLineMesh {
    vertex_buffer: wgpu::Buffer,
    vertex_count: u32,
}

pub struct GpuRenderer {
    solid_pipeline: wgpu::RenderPipeline,
    wireframe_pipeline: wgpu::RenderPipeline,
    line_pipeline: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
    texture_bind_group_layout: wgpu::BindGroupLayout,
    default_texture_bind_group: wgpu::BindGroup,
    color_texture: Option<wgpu::Texture>,
    color_view: Option<wgpu::TextureView>,
    depth_view: Option<wgpu::TextureView>,
    viewport_size: [u32; 2],
    pub egui_texture_id: Option<egui::TextureId>,
    axis_lines: GpuLineMesh,
    grid_lines: GpuLineMesh,
}

impl GpuRenderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mesh_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/mesh.wgsl").into()),
        });

        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("uniforms"),
            contents: bytemuck::cast_slice(&[Uniforms::identity()]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let uniform_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("uniform_bgl"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("uniform_bg"),
            layout: &uniform_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("texture_bgl"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        let default_texture_bind_group =
            create_1x1_white_texture_bind_group(device, queue, &texture_bind_group_layout);

        let mesh_vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<GpuVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: 12,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: 24,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x2,
                },
            ],
        };

        let line_vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<LineVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: 12,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x4,
                },
            ],
        };

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mesh_pipeline_layout"),
            bind_group_layouts: &[&uniform_bind_group_layout, &texture_bind_group_layout],
            push_constant_ranges: &[],
        });

        let line_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("line_pipeline_layout"),
            bind_group_layouts: &[&uniform_bind_group_layout],
            push_constant_ranges: &[],
        });

        let depth_stencil = Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth24Plus,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Less,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        });

        let solid_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("solid_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[mesh_vertex_layout.clone()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_solid"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: depth_stencil.clone(),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let wireframe_depth = Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth24Plus,
            depth_write_enabled: false,
            depth_compare: wgpu::CompareFunction::LessEqual,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState {
                constant: -2,
                slope_scale: -1.0,
                clamp: 0.0,
            },
        });

        let wireframe_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("wireframe_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[mesh_vertex_layout],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_wireframe"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                ..Default::default()
            },
            depth_stencil: wireframe_depth,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let line_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("line_pipeline"),
            layout: Some(&line_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_line"),
                buffers: &[line_vertex_layout],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_line"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                ..Default::default()
            },
            depth_stencil: depth_stencil.clone(),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let axis_lines = create_axis_lines(device);
        let grid_lines = create_ground_grid(device);

        Self {
            solid_pipeline,
            wireframe_pipeline,
            line_pipeline,
            uniform_buffer,
            uniform_bind_group,
            texture_bind_group_layout,
            default_texture_bind_group,
            color_texture: None,
            color_view: None,
            depth_view: None,
            viewport_size: [0, 0],
            egui_texture_id: None,
            axis_lines,
            grid_lines,
        }
    }

    pub fn upload_mesh(&self, device: &wgpu::Device, meshes: &[BoneMeshData]) -> Option<GpuMesh> {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let mut bounds = PreviewBounds {
            min: [f32::INFINITY; 3],
            max: [f32::NEG_INFINITY; 3],
        };

        for mesh in meshes {
            let base = vertices.len() as u32;
            for v in &mesh.vertices {
                let pos = [v.x, v.y, v.z];
                for (axis, val) in pos.iter().enumerate() {
                    bounds.min[axis] = bounds.min[axis].min(*val);
                    bounds.max[axis] = bounds.max[axis].max(*val);
                }
                vertices.push(GpuVertex {
                    position: pos,
                    normal: [v.nx, v.ny, v.nz],
                    uv: [v.u, v.v],
                });
            }
            for &(a, b, c) in &mesh.faces {
                indices.push(base + a as u32);
                indices.push(base + b as u32);
                indices.push(base + c as u32);
            }
        }

        if vertices.is_empty() {
            return None;
        }

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh_vb"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh_ib"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        Some(GpuMesh {
            vertex_buffer,
            index_buffer,
            index_count: indices.len() as u32,
            bounds,
        })
    }

    pub fn upload_texture(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        rgba: &[[u8; 4]],
        width: u32,
        height: u32,
    ) -> wgpu::BindGroup {
        let flat: Vec<u8> = rgba.iter().flat_map(|p| *p).collect();
        let texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("gim_texture"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &flat,
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("texture_bg"),
            layout: &self.texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        })
    }

    pub fn ensure_viewport(
        &mut self,
        device: &wgpu::Device,
        egui_renderer: &mut eframe::egui_wgpu::Renderer,
        width: u32,
        height: u32,
    ) {
        let w = width.max(1);
        let h = height.max(1);
        if self.viewport_size == [w, h] {
            return;
        }
        eprintln!(
            "[gpu] ensure_viewport: {}x{} (was {:?})",
            w, h, self.viewport_size
        );
        self.viewport_size = [w, h];

        let color_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen_color"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let color_view = color_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen_depth"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth24Plus,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth_view = depth_texture.create_view(&wgpu::TextureViewDescriptor::default());

        if let Some(old_id) = self.egui_texture_id.take() {
            egui_renderer.free_texture(&old_id);
        }

        let texture_id =
            egui_renderer.register_native_texture(device, &color_view, wgpu::FilterMode::Linear);
        eprintln!("[gpu] registered egui texture: {:?}", texture_id);

        self.color_texture = Some(color_texture);
        self.color_view = Some(color_view);
        self.depth_view = Some(depth_view);
        self.egui_texture_id = Some(texture_id);
    }

    pub fn render(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        camera: &PreviewCamera,
        mesh: &GpuMesh,
        texture_bind_group: Option<&wgpu::BindGroup>,
        show_wireframe: bool,
        show_axes: bool,
        show_grid: bool,
    ) {
        let color_view = match &self.color_view {
            Some(v) => v,
            None => return,
        };
        let depth_view = match &self.depth_view {
            Some(v) => v,
            None => return,
        };

        let [vw, vh] = self.viewport_size;
        let uniforms = build_uniforms(camera, vw, vh, texture_bind_group.is_some());
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&[uniforms]));

        let wire_buf = if show_wireframe {
            let wire_ib = build_wireframe_indices(mesh.index_count);
            Some((
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("wire_ib"),
                    contents: bytemuck::cast_slice(&wire_ib),
                    usage: wgpu::BufferUsages::INDEX,
                }),
                wire_ib.len() as u32,
            ))
        } else {
            None
        };

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("mesh_render"),
        });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("mesh_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.176,
                            g: 0.176,
                            b: 0.176,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });

            let tex_bg = texture_bind_group.unwrap_or(&self.default_texture_bind_group);

            pass.set_pipeline(&self.solid_pipeline);
            pass.set_bind_group(0, &self.uniform_bind_group, &[]);
            pass.set_bind_group(1, tex_bg, &[]);
            pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
            pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.index_count, 0, 0..1);

            if let Some((ref buf, count)) = wire_buf {
                pass.set_pipeline(&self.wireframe_pipeline);
                pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                pass.set_bind_group(1, tex_bg, &[]);
                pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                pass.set_index_buffer(buf.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..count, 0, 0..1);
            }

            if show_grid {
                pass.set_pipeline(&self.line_pipeline);
                pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                pass.set_vertex_buffer(0, self.grid_lines.vertex_buffer.slice(..));
                pass.draw(0..self.grid_lines.vertex_count, 0..1);
            }

            if show_axes {
                pass.set_pipeline(&self.line_pipeline);
                pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                pass.set_vertex_buffer(0, self.axis_lines.vertex_buffer.slice(..));
                pass.draw(0..self.axis_lines.vertex_count, 0..1);
            }
        }

        queue.submit(std::iter::once(encoder.finish()));
    }
}

impl Uniforms {
    fn identity() -> Self {
        let id = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        Self {
            mvp: id,
            model: id,
            light_dir: [0.5, 0.8, 0.6],
            ambient: 0.3,
            camera_pos: [0.0, 0.0, 5.0],
            use_texture: 0.0,
        }
    }
}

fn build_uniforms(camera: &PreviewCamera, vw: u32, vh: u32, has_texture: bool) -> Uniforms {
    let aspect = vw as f32 / vh.max(1) as f32;
    let view = look_at(camera);
    let proj = perspective(camera.fov_y_radians, aspect, camera.near, camera.far);
    let mut model = mat4_identity();
    model[0][0] = -1.0;
    let mvp = mat4_mul(model, mat4_mul(view, proj));

    let cos_p = camera.pitch.cos();
    let eye = [
        camera.target[0] - camera.yaw.sin() * cos_p * camera.distance,
        camera.target[1] - camera.pitch.sin() * camera.distance,
        camera.target[2] - camera.yaw.cos() * cos_p * camera.distance,
    ];

    Uniforms {
        mvp,
        model,
        light_dir: [0.5, 0.8, 0.6],
        ambient: 0.3,
        camera_pos: eye,
        use_texture: if has_texture { 1.0 } else { 0.0 },
    }
}

fn build_wireframe_indices(tri_index_count: u32) -> Vec<u32> {
    let mut lines = Vec::with_capacity(tri_index_count as usize * 2);
    let tri_count = tri_index_count / 3;
    for t in 0..tri_count {
        let base = t * 3;
        lines.extend_from_slice(&[base, base + 1, base + 1, base + 2, base + 2, base]);
    }
    lines
}

fn create_axis_lines(device: &wgpu::Device) -> GpuLineMesh {
    let len = 1.0_f32;
    let verts = [
        LineVertex {
            position: [0.0, 0.0, 0.0],
            color: [1.0, 0.2, 0.2, 1.0],
        },
        LineVertex {
            position: [len, 0.0, 0.0],
            color: [1.0, 0.2, 0.2, 1.0],
        },
        LineVertex {
            position: [0.0, 0.0, 0.0],
            color: [0.2, 1.0, 0.2, 1.0],
        },
        LineVertex {
            position: [0.0, len, 0.0],
            color: [0.2, 1.0, 0.2, 1.0],
        },
        LineVertex {
            position: [0.0, 0.0, 0.0],
            color: [0.2, 0.2, 1.0, 1.0],
        },
        LineVertex {
            position: [0.0, 0.0, len],
            color: [0.2, 0.2, 1.0, 1.0],
        },
    ];
    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("axis_vb"),
        contents: bytemuck::cast_slice(&verts),
        usage: wgpu::BufferUsages::VERTEX,
    });
    GpuLineMesh {
        vertex_buffer,
        vertex_count: verts.len() as u32,
    }
}

fn create_ground_grid(device: &wgpu::Device) -> GpuLineMesh {
    let mut verts = Vec::new();
    let extent = 20.0_f32;
    let step = 1.0_f32;
    let color_major = [0.25, 0.25, 0.25, 1.0];
    let color_minor = [0.18, 0.18, 0.18, 1.0];
    let n = (extent / step) as i32;
    for i in -n..=n {
        let pos = i as f32 * step;
        let color = if i % 5 == 0 { color_major } else { color_minor };
        verts.push(LineVertex {
            position: [pos, 0.0, -extent],
            color,
        });
        verts.push(LineVertex {
            position: [pos, 0.0, extent],
            color,
        });
        verts.push(LineVertex {
            position: [-extent, 0.0, pos],
            color,
        });
        verts.push(LineVertex {
            position: [extent, 0.0, pos],
            color,
        });
    }
    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("grid_vb"),
        contents: bytemuck::cast_slice(&verts),
        usage: wgpu::BufferUsages::VERTEX,
    });
    GpuLineMesh {
        vertex_buffer,
        vertex_count: verts.len() as u32,
    }
}

fn create_1x1_white_texture_bind_group(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
) -> wgpu::BindGroup {
    let texture = device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some("white_1x1"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &[255, 255, 255, 255],
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("white_texture_bg"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    })
}

fn mat4_identity() -> [[f32; 4]; 4] {
    [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

fn mat4_mul(a: [[f32; 4]; 4], b: [[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut r = [[0.0_f32; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            r[i][j] = a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j] + a[i][3] * b[3][j];
        }
    }
    r
}

fn look_at(camera: &PreviewCamera) -> [[f32; 4]; 4] {
    let cos_p = camera.pitch.cos();
    let fwd = [
        camera.yaw.sin() * cos_p,
        camera.pitch.sin(),
        camera.yaw.cos() * cos_p,
    ];
    let eye = [
        camera.target[0] - fwd[0] * camera.distance,
        camera.target[1] - fwd[1] * camera.distance,
        camera.target[2] - fwd[2] * camera.distance,
    ];
    let f = normalize3(fwd);
    let up = [0.0_f32, 1.0, 0.0];
    let r = normalize3(cross3(up, f));
    let u = cross3(f, r);

    // Standard convention: camera looks along -Z in view space.
    // Negate f so objects in front have negative z_view,
    // matching the perspective matrix's w_clip = -z expectation.
    [
        [r[0], u[0], -f[0], 0.0],
        [r[1], u[1], -f[1], 0.0],
        [r[2], u[2], -f[2], 0.0],
        [-dot3(r, eye), -dot3(u, eye), dot3(f, eye), 1.0],
    ]
}

fn perspective(fov_y: f32, aspect: f32, near: f32, far: f32) -> [[f32; 4]; 4] {
    let f = 1.0 / (fov_y * 0.5).tan();
    let range_inv = 1.0 / (near - far);
    [
        [f / aspect, 0.0, 0.0, 0.0],
        [0.0, f, 0.0, 0.0],
        [0.0, 0.0, far * range_inv, -1.0],
        [0.0, 0.0, near * far * range_inv, 0.0],
    ]
}

fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize3(v: [f32; 3]) -> [f32; 3] {
    let len = dot3(v, v).sqrt();
    if len <= f32::EPSILON {
        [0.0; 3]
    } else {
        [v[0] / len, v[1] / len, v[2] / len]
    }
}
