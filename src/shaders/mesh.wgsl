struct Uniforms {
    mvp: mat4x4<f32>,
    model: mat4x4<f32>,
    light_dir: vec3<f32>,
    ambient: f32,
    camera_pos: vec3<f32>,
    use_texture: f32,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(1) @binding(0) var diffuse_texture: texture_2d<f32>;
@group(1) @binding(1) var diffuse_sampler: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
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
fn fs_solid(in: VertexOutput) -> @location(0) vec4<f32> {
    let n = normalize(in.world_normal);
    let light = normalize(u.light_dir);
    let diffuse = max(dot(n, light), 0.0);
    let lighting = u.ambient + (1.0 - u.ambient) * diffuse;

    var base_color = vec3<f32>(0.7, 0.7, 0.75);
    if u.use_texture > 0.5 {
        base_color = textureSample(diffuse_texture, diffuse_sampler, in.uv).rgb;
    }

    let view_dir = normalize(u.camera_pos - in.world_pos);
    let half_dir = normalize(light + view_dir);
    let spec = pow(max(dot(n, half_dir), 0.0), 32.0) * 0.3;

    let color = base_color * lighting + vec3<f32>(spec);
    return vec4(color, 1.0);
}

@fragment
fn fs_wireframe(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4(0.0, 0.0, 0.0, 0.6);
}

// Grid shader - axis lines
struct GridVertex {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
};

struct GridOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_line(in: GridVertex) -> GridOutput {
    var out: GridOutput;
    out.clip_position = u.mvp * vec4(in.position, 1.0);
    out.color = in.color;
    return out;
}

@fragment
fn fs_line(in: GridOutput) -> @location(0) vec4<f32> {
    return in.color;
}
