struct CameraUniform {
    view: mat4x4<f32>,
    proj: mat4x4<f32>,
    view_position: vec4<f32>,
    view_inverse: mat4x4<f32>,
};

@group(0) @binding(0)
var<uniform> camera: CameraUniform;

struct LitMaterialUniform {
    front_color: vec4<f32>,
    back_color: vec4<f32>,
    light_color: vec4<f32>,
    light_offset: vec4<f32>,
    params: vec4<f32>,
};

@group(1) @binding(0)
var<uniform> material: LitMaterialUniform;

struct LitSurface {
    world_position: vec3<f32>,
    world_normal: vec3<f32>,
};

struct LitDirections {
    view_dir: vec3<f32>,
    light_dir: vec3<f32>,
};

struct LitFace {
    normal: vec3<f32>,
    color: vec3<f32>,
    alpha: f32,
};

fn lit_directions(
    camera_position: vec3<f32>,
    camera_view_inverse: mat4x4<f32>,
    camera_space_light_offset: vec3<f32>,
    world_position: vec3<f32>,
) -> LitDirections {
    let light_world_offset = (camera_view_inverse * vec4<f32>(camera_space_light_offset, 0.0)).xyz;
    let light_position = camera_position + light_world_offset;

    var out: LitDirections;
    out.view_dir = normalize(camera_position - world_position);
    out.light_dir = normalize(light_position - world_position);
    return out;
}

fn lit_face(material: LitMaterialUniform, source_normal: vec3<f32>, view_dir: vec3<f32>) -> LitFace {
    let normal = normalize(source_normal);
    let faces_viewer = dot(view_dir, normal) >= 0.0;

    var out: LitFace;
    if faces_viewer {
        out.normal = normal;
        out.color = material.front_color.rgb;
        out.alpha = material.front_color.a;
    } else {
        out.normal = -normal;
        out.color = material.back_color.rgb;
        out.alpha = material.back_color.a;
    }
    return out;
}

fn shade_blinn_phong(
    material: LitMaterialUniform,
    face: LitFace,
    directions: LitDirections,
) -> vec4<f32> {
    let light_color = material.light_color.rgb;
    let ambient_strength = material.params.x;
    let specular_strength = material.params.y;
    let shininess = material.params.z;

    let ambient = ambient_strength * light_color;
    let diffuse_amount = max(dot(face.normal, directions.light_dir), 0.0);
    let diffuse = diffuse_amount * light_color * (1.0 - ambient_strength);

    let halfway_dir = normalize(directions.light_dir + directions.view_dir);
    let specular_amount = pow(max(dot(face.normal, halfway_dir), 0.0), shininess);
    let specular = specular_strength * specular_amount * light_color;

    let color = (ambient + diffuse) * face.color + specular;
    return vec4<f32>(color, face.alpha);
}

fn shade_lit_surface(
    material: LitMaterialUniform,
    camera_position: vec3<f32>,
    camera_view_inverse: mat4x4<f32>,
    surface: LitSurface,
) -> vec4<f32> {
    let directions = lit_directions(
        camera_position,
        camera_view_inverse,
        material.light_offset.xyz,
        surface.world_position,
    );
    let face = lit_face(material, surface.world_normal, directions.view_dir);
    return shade_blinn_phong(material, face, directions);
}

struct VertexInput {
    @location(7) position: vec3<f32>,
    @location(9) normal: vec3<f32>,
};

struct InstanceInput {
    @location(0) model_0: vec4<f32>,
    @location(1) model_1: vec4<f32>,
    @location(2) model_2: vec4<f32>,
    @location(3) model_3: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
};

@vertex
fn vs_main(model: VertexInput, instance: InstanceInput) -> VertexOutput {
    let model_matrix = mat4x4<f32>(
        instance.model_0,
        instance.model_1,
        instance.model_2,
        instance.model_3,
    );

    let world_position = model_matrix * vec4<f32>(model.position, 1.0);
    var out: VertexOutput;
    out.clip_position = camera.proj * camera.view * world_position;
    out.world_position = world_position.xyz;
    out.world_normal = normalize((model_matrix * vec4<f32>(model.normal, 0.0)).xyz);
    return out;
}

@fragment
fn fs_main(
    in: VertexOutput,
) -> @location(0) vec4<f32> {
    var surface: LitSurface;
    surface.world_position = in.world_position;
    surface.world_normal = in.world_normal;
    return shade_lit_surface(material, camera.view_position.xyz, camera.view_inverse, surface);
}
