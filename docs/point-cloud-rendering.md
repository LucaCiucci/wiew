# Point-Cloud Rendering Notes

These notes record the intended direction for scanner-style point-cloud
rendering in `wiew`. They are based on the visual behavior seen in CAD/scanner
tools such as Leios, not on confirmed knowledge of their internal renderer.

## Observed Behavior

Some tools render sparse point clouds in a way that looks almost like a
triangulated surface when the view is still. During interaction, holes become
visible again, which suggests the data is still being rendered as points rather
than as a real triangle mesh.

That behavior is usually achieved by changing quality or technique between
interactive and settled rendering:

- While interacting, draw cheaper raw points or lower-quality splats.
- When interaction stops, draw a higher-quality point-cloud representation.

## Likely Techniques

The most relevant technique is point splatting.

Instead of drawing each point as a single pixel, each point is expanded into a
small screen-facing or normal-oriented primitive. These primitives are often
called splats or surfels. When they overlap, they visually fill gaps and can read
as a continuous surface without creating triangles.

Useful ingredients:

- Adaptive point size based on camera distance, projected density, or local
  spacing.
- Normal-oriented surfels for scanner data that already has point normals.
- Normal-based lighting so the point cloud responds like a surface.
- Alpha blending or weighted blending for smoother overlap.
- Depth testing so surfaces occlude correctly.
- Optional screen-space passes for tiny hole filling or depth enhancement.
- Progressive refinement: fast while moving, higher quality once the view is
  idle.

Eye-Dome Lighting is also worth considering later. It is a screen-space depth
enhancement commonly used for point clouds because it makes local shape and
depth discontinuities much easier to read.

## Direction For `wiew`

This should not be folded into the generic lit mesh pipeline. It should be a
separate provided pipeline family.

Possible split:

- `PointPipeline`: simple raw points, fast and predictable.
- `SplatPipeline` or `SurfelPipeline`: expands points into camera-facing quads
  or normal-oriented disks.
- Optional later post-processing pass: Eye-Dome Lighting or small-gap filling.

For the scanner app, the important source streams are:

- positions
- normals
- optional color
- optional scanner-specific attributes

The first useful experiment should use positions and normals, with a material
similar to the current lit material. The goal is to make sparse scan data look
solid at rest while keeping interaction responsive.

## Open Questions

- Should splat radius be a material parameter, derived from point spacing, or
  both?
- Should the high-quality still pass be triggered by the app, the view, or a
  small interaction-state helper?
- Should surfels be camera-facing for robustness, normal-oriented for surface
  fidelity, or selectable per pipeline?
- Is weighted blended transparency needed, or is simple alpha plus depth enough
  for scanner use cases?
