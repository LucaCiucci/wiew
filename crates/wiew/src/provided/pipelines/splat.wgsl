struct CameraUniform {
    view: mat4x4<f32>,
    proj: mat4x4<f32>,
    view_position: vec4<f32>,
    view_inverse: mat4x4<f32>,
    viewport_size: vec4<f32>,
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

fn lit_face_from_colors(
    front_color: vec4<f32>,
    back_color: vec4<f32>,
    source_normal: vec3<f32>,
    view_dir: vec3<f32>,
) -> LitFace {
    let normal = normalize(source_normal);
    let faces_viewer = dot(view_dir, normal) >= 0.0;

    var out: LitFace;
    if faces_viewer {
        out.normal = normal;
        out.color = front_color.rgb;
        out.alpha = front_color.a;
    } else {
        out.normal = -normal;
        out.color = back_color.rgb;
        out.alpha = back_color.a;
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
    point_color: vec4<f32>,
) -> vec4<f32> {
    let directions = lit_directions(
        camera_position,
        camera_view_inverse,
        material.light_offset.xyz,
        surface.world_position,
    );
    let face = lit_face_from_colors(
        vec4<f32>(point_color.rgb * material.front_color.rgb, point_color.a * material.front_color.a),
        vec4<f32>(point_color.rgb * material.back_color.rgb, point_color.a * material.back_color.a),
        surface.world_normal,
        directions.view_dir,
    );
    return shade_blinn_phong(material, face, directions);
}

struct PointInput {
    @location(7) position: vec3<f32>,
    @location(9) normal: vec3<f32>,
};

struct ColoredPointInput {
    @location(7) position: vec3<f32>,
    @location(9) normal: vec3<f32>,
    @location(8) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) local: vec2<f32>,
    @location(3) point_color: vec4<f32>,
};

fn quad_corner(vertex_index: u32) -> vec2<f32> {
    let corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>( 1.0,  1.0),
    );
    return corners[vertex_index];
}

fn splat_vertex(
    position: vec3<f32>,
    normal: vec3<f32>,
    point_color: vec4<f32>,
    vertex_index: u32,
) -> VertexOutput {
    let local = quad_corner(vertex_index);
    let half_world_size = max(material.params.w, 0.0001) * 0.5;
    let camera_right = normalize((camera.view_inverse * vec4<f32>(1.0, 0.0, 0.0, 0.0)).xyz);
    let center_clip = camera.proj * camera.view * vec4<f32>(position, 1.0);
    let right_clip = camera.proj * camera.view * vec4<f32>(position + camera_right * half_world_size, 1.0);
    let center_ndc = center_clip.xy / center_clip.w;
    let right_ndc = right_clip.xy / right_clip.w;
    let projected_radius_px = distance(center_ndc, right_ndc) * camera.viewport_size.y * 0.5;
    let radius_px = clamp(projected_radius_px, 1.25, 12.0);
    let offset_ndc = local * radius_px * 2.0 / camera.viewport_size.xy;

    var out: VertexOutput;
    out.clip_position = vec4<f32>(
        center_clip.xy + offset_ndc * center_clip.w,
        center_clip.z,
        center_clip.w,
    );
    out.world_position = position;
    out.world_normal = normalize(normal);
    out.local = local;
    out.point_color = point_color;
    return out;
}

@vertex
fn vs_main(
    point: PointInput,
    @builtin(vertex_index) vertex_index: u32,
) -> VertexOutput {
    return splat_vertex(point.position, point.normal, vec4<f32>(1.0), vertex_index);
}

@vertex
fn vs_colored_main(
    point: ColoredPointInput,
    @builtin(vertex_index) vertex_index: u32,
) -> VertexOutput {
    return splat_vertex(point.position, point.normal, point.color, vertex_index);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let radius2 = dot(in.local, in.local);
    if radius2 > 1.0 {
        discard;
    }

    //let edge_alpha = 1.0 - smoothstep(0.82, 1.0, radius2);
    let edge_alpha = 1.0;

    var surface: LitSurface;
    surface.world_position = in.world_position;
    surface.world_normal = in.world_normal;
    let color = shade_lit_surface(
        material,
        camera.view_position.xyz,
        camera.view_inverse,
        surface,
        in.point_color,
    );
    return vec4<f32>(color.rgb, color.a * edge_alpha);
}
