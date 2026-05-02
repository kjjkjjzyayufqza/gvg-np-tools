use crate::pmf2::{BoneMeshData, Pmf2Meta};
use anyhow::{Result, bail};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewVertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    pub normal: [f32; 3],
    pub bone_index: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewBounds {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewViewport {
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewCamera {
    pub target: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub fov_y_radians: f32,
    pub near: f32,
    pub far: f32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewVisibility {
    pub show_axes: bool,
    pub show_bounds: bool,
    pub show_grid: bool,
    hidden_bones: BTreeSet<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pmf2PreviewMesh {
    pub vertices: Vec<PreviewVertex>,
    pub indices: Vec<u32>,
    pub bounds: PreviewBounds,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProjectedTriangle {
    pub points: [[f32; 2]; 3],
    pub bone_index: usize,
    pub depth: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProjectedLine {
    pub start: [f32; 2],
    pub end: [f32; 2],
    pub color: PreviewLineColor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewLineColor {
    XAxis,
    YAxis,
    ZAxis,
    Bounds,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectedPreview {
    pub triangles: Vec<ProjectedTriangle>,
    pub axes: Vec<ProjectedLine>,
    pub bounds: Vec<ProjectedLine>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PreviewState {
    pub camera: Option<PreviewCamera>,
    pub visibility: PreviewVisibility,
    pub wireframe: bool,
}

impl Default for PreviewVisibility {
    fn default() -> Self {
        Self {
            show_axes: true,
            show_bounds: true,
            show_grid: true,
            hidden_bones: BTreeSet::new(),
        }
    }
}

impl PreviewVisibility {
    pub fn set_bone_visible(&mut self, bone_index: usize, visible: bool) {
        if visible {
            self.hidden_bones.remove(&bone_index);
        } else {
            self.hidden_bones.insert(bone_index);
        }
    }

    pub fn is_bone_visible(&self, bone_index: usize) -> bool {
        !self.hidden_bones.contains(&bone_index)
    }
}

impl Default for PreviewState {
    fn default() -> Self {
        Self {
            camera: None,
            visibility: PreviewVisibility::default(),
            wireframe: false,
        }
    }
}

impl PreviewCamera {
    pub fn frame_bounds(bounds: PreviewBounds) -> Self {
        let center = [
            (bounds.min[0] + bounds.max[0]) * 0.5,
            (bounds.min[1] + bounds.max[1]) * 0.5,
            (bounds.min[2] + bounds.max[2]) * 0.5,
        ];
        let extent = [
            (bounds.max[0] - bounds.min[0]).abs(),
            (bounds.max[1] - bounds.min[1]).abs(),
            (bounds.max[2] - bounds.min[2]).abs(),
        ];
        let radius = length(extent).max(1.0) * 0.5;
        let fov_y_radians = 45.0_f32.to_radians();
        Self {
            target: center,
            yaw: 0.35,
            pitch: 0.35,
            distance: (radius / (fov_y_radians * 0.5).tan()).max(2.0),
            fov_y_radians,
            near: 0.01,
            far: 100_000.0,
        }
    }

    pub fn orbit(&mut self, delta_yaw: f32, delta_pitch: f32) {
        self.yaw += delta_yaw;
        self.pitch = (self.pitch + delta_pitch).clamp(-1.45, 1.45);
    }

    pub fn zoom(&mut self, delta: f32) {
        self.distance = (self.distance * (1.0 + delta)).max(0.05);
    }

    fn basis(&self) -> CameraBasis {
        let cos_pitch = self.pitch.cos();
        let forward = normalize([
            self.yaw.sin() * cos_pitch,
            self.pitch.sin(),
            self.yaw.cos() * cos_pitch,
        ]);
        let right = normalize(cross([0.0, 1.0, 0.0], forward));
        let up = cross(forward, right);
        let eye = sub(self.target, scale(forward, self.distance));
        CameraBasis {
            eye,
            forward,
            right,
            up,
        }
    }
}

impl Pmf2PreviewMesh {
    pub fn from_meta(meta: &Pmf2Meta) -> Result<Self> {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let mut bounds = PreviewBounds {
            min: [f32::INFINITY; 3],
            max: [f32::NEG_INFINITY; 3],
        };
        for mesh in &meta.bone_meshes {
            let base = vertices.len() as u32;
            for vertex in &mesh.local_vertices {
                let position = [vertex[0], vertex[1], vertex[2]];
                for (axis, value) in position.iter().enumerate() {
                    bounds.min[axis] = bounds.min[axis].min(*value);
                    bounds.max[axis] = bounds.max[axis].max(*value);
                }
                vertices.push(PreviewVertex {
                    position,
                    uv: [vertex[3], vertex[4]],
                    normal: [vertex[5], vertex[6], vertex[7]],
                    bone_index: mesh.bone_index,
                });
            }
            for face in &mesh.faces {
                for index in face {
                    if *index >= mesh.local_vertices.len() {
                        bail!(
                            "PMF2 preview face references vertex {} but mesh has {} vertices",
                            index,
                            mesh.local_vertices.len()
                        );
                    }
                    indices.push(base + *index as u32);
                }
            }
        }
        if vertices.is_empty() {
            bail!("PMF2 metadata contains no previewable vertices");
        }
        Ok(Self {
            vertices,
            indices,
            bounds,
        })
    }

    pub fn from_bone_meshes(meshes: &[BoneMeshData]) -> Result<Self> {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let mut bounds = PreviewBounds {
            min: [f32::INFINITY; 3],
            max: [f32::NEG_INFINITY; 3],
        };
        for mesh in meshes {
            let base = vertices.len() as u32;
            for vertex in &mesh.vertices {
                let position = [vertex.x, vertex.y, vertex.z];
                for (axis, value) in position.iter().enumerate() {
                    bounds.min[axis] = bounds.min[axis].min(*value);
                    bounds.max[axis] = bounds.max[axis].max(*value);
                }
                vertices.push(PreviewVertex {
                    position,
                    uv: [vertex.u, vertex.v],
                    normal: [vertex.nx, vertex.ny, vertex.nz],
                    bone_index: mesh.bone_index,
                });
            }
            for &(a, b, c) in &mesh.faces {
                for index in [a, b, c] {
                    if index >= mesh.vertices.len() {
                        bail!(
                            "PMF2 preview face references vertex {} but mesh has {} vertices",
                            index,
                            mesh.vertices.len()
                        );
                    }
                    indices.push(base + index as u32);
                }
            }
        }
        if vertices.is_empty() {
            bail!("PMF2 mesh extraction contains no previewable vertices");
        }
        Ok(Self {
            vertices,
            indices,
            bounds,
        })
    }

    pub fn project(
        &self,
        camera: &PreviewCamera,
        viewport: PreviewViewport,
        visibility: &PreviewVisibility,
    ) -> Result<ProjectedPreview> {
        if viewport.width <= 0.0 || viewport.height <= 0.0 {
            bail!("preview viewport dimensions must be positive");
        }
        let mut triangles = Vec::new();
        for triangle in self.indices.chunks(3) {
            if triangle.len() != 3 {
                continue;
            }
            let a = self.vertices[triangle[0] as usize];
            let b = self.vertices[triangle[1] as usize];
            let c = self.vertices[triangle[2] as usize];
            if !visibility.is_bone_visible(a.bone_index)
                || !visibility.is_bone_visible(b.bone_index)
                || !visibility.is_bone_visible(c.bone_index)
            {
                continue;
            }
            let Some(pa) = project_point(a.position, camera, viewport) else {
                continue;
            };
            let Some(pb) = project_point(b.position, camera, viewport) else {
                continue;
            };
            let Some(pc) = project_point(c.position, camera, viewport) else {
                continue;
            };
            triangles.push(ProjectedTriangle {
                points: [pa.screen, pb.screen, pc.screen],
                bone_index: a.bone_index,
                depth: (pa.depth + pb.depth + pc.depth) / 3.0,
            });
        }
        triangles.sort_by(|a, b| b.depth.total_cmp(&a.depth));
        let axes = if visibility.show_axes {
            project_axes(camera, viewport)
        } else {
            Vec::new()
        };
        let bounds = if visibility.show_bounds {
            project_bounds(self.bounds, camera, viewport)
        } else {
            Vec::new()
        };
        Ok(ProjectedPreview {
            triangles,
            axes,
            bounds,
        })
    }

    pub fn bones(&self) -> Vec<usize> {
        self.vertices
            .iter()
            .map(|vertex| vertex.bone_index)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
}

#[derive(Clone, Copy)]
struct CameraBasis {
    eye: [f32; 3],
    forward: [f32; 3],
    right: [f32; 3],
    up: [f32; 3],
}

#[derive(Clone, Copy)]
struct ProjectedPoint {
    screen: [f32; 2],
    depth: f32,
}

fn project_axes(camera: &PreviewCamera, viewport: PreviewViewport) -> Vec<ProjectedLine> {
    let axes = [
        ([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], PreviewLineColor::XAxis),
        ([0.0, 0.0, 0.0], [0.0, 1.0, 0.0], PreviewLineColor::YAxis),
        ([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], PreviewLineColor::ZAxis),
    ];
    axes.iter()
        .filter_map(|(start, end, color)| project_line(*start, *end, *color, camera, viewport))
        .collect()
}

fn project_bounds(
    bounds: PreviewBounds,
    camera: &PreviewCamera,
    viewport: PreviewViewport,
) -> Vec<ProjectedLine> {
    let [min_x, min_y, min_z] = bounds.min;
    let [max_x, max_y, max_z] = bounds.max;
    let corners = [
        [min_x, min_y, min_z],
        [max_x, min_y, min_z],
        [max_x, max_y, min_z],
        [min_x, max_y, min_z],
        [min_x, min_y, max_z],
        [max_x, min_y, max_z],
        [max_x, max_y, max_z],
        [min_x, max_y, max_z],
    ];
    let edges = [
        (0, 1),
        (1, 2),
        (2, 3),
        (3, 0),
        (4, 5),
        (5, 6),
        (6, 7),
        (7, 4),
        (0, 4),
        (1, 5),
        (2, 6),
        (3, 7),
    ];
    edges
        .iter()
        .filter_map(|(a, b)| {
            project_line(
                corners[*a],
                corners[*b],
                PreviewLineColor::Bounds,
                camera,
                viewport,
            )
        })
        .collect()
}

fn project_line(
    start: [f32; 3],
    end: [f32; 3],
    color: PreviewLineColor,
    camera: &PreviewCamera,
    viewport: PreviewViewport,
) -> Option<ProjectedLine> {
    let start = project_point(start, camera, viewport)?;
    let end = project_point(end, camera, viewport)?;
    Some(ProjectedLine {
        start: start.screen,
        end: end.screen,
        color,
    })
}

fn project_point(
    point: [f32; 3],
    camera: &PreviewCamera,
    viewport: PreviewViewport,
) -> Option<ProjectedPoint> {
    let basis = camera.basis();
    let relative = sub(point, basis.eye);
    let x = dot(relative, basis.right);
    let y = dot(relative, basis.up);
    let z = dot(relative, basis.forward);
    if z <= camera.near || z >= camera.far {
        return None;
    }
    let aspect = viewport.width / viewport.height;
    let f = 1.0 / (camera.fov_y_radians * 0.5).tan();
    let ndc_x = (x * f / aspect) / z;
    let ndc_y = (y * f) / z;
    Some(ProjectedPoint {
        screen: [
            (ndc_x * 0.5 + 0.5) * viewport.width,
            (0.5 - ndc_y * 0.5) * viewport.height,
        ],
        depth: z,
    })
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn length(v: [f32; 3]) -> f32 {
    dot(v, v).sqrt()
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = length(v);
    if len <= f32::EPSILON {
        [0.0, 0.0, 0.0]
    } else {
        scale(v, 1.0 / len)
    }
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale(v: [f32; 3], s: f32) -> [f32; 3] {
    [v[0] * s, v[1] * s, v[2] * s]
}
