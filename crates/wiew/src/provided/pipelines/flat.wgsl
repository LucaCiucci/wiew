// ================================================================
//                       Camera Uniform
// ================================================================

struct CameraUniform {
    view: mat4x4<f32>,
    proj: mat4x4<f32>,
    view_position: vec3<f32>,
};

@group(0) @binding(0)
var<uniform> camera: CameraUniform;

// ================================================================
//                           Vertex
// ================================================================

struct VertexInput {
    @location(7) position: vec3<f32>,
    @location(8) color: vec4<f32>,
};

struct InstanceInput {
    @location(0) model_0: vec4<f32>,
    @location(1) model_1: vec4<f32>,
    @location(2) model_2: vec4<f32>,
    @location(3) model_3: vec4<f32>,
};

@vertex
fn vs_main(
    model: VertexInput,
    instance: InstanceInput,
) -> VertexOutput {
    let model_matrix = mat4x4<f32>(
        instance.model_0,
        instance.model_1,
        instance.model_2,
        instance.model_3,
    );

    var out: VertexOutput;
    out.clip_position = camera.proj * camera.view * model_matrix * vec4<f32>(model.position, 1.0);
    out.color = model.color;
    return out;
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

// ================================================================
//                          Fragment
// ================================================================

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}
