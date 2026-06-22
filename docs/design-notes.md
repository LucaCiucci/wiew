# wiew Design Notes

These notes are an initial design direction for the crate. They are not a
mandatory path, not a finalized architecture, and not a promise about the public
API. The point is to record the lessons from earlier experiments and give the
current implementation a concrete direction to test against.

## Background

The crate follows two earlier rendering experiments:

- `LC`: an older OpenGL renderer with retained scene objects, render resources,
  rendering contexts, drawables, cameras, shaders, and explicit prepare/draw
  phases.
- `wiew2`: a newer `wgpu` renderer that explored a more reactive/component
  approach based on `birb-react`.

Both versions worked in some form, but both became awkward to use. The current
crate should keep the useful ideas while avoiding the complexity that made the
old versions hard to extend.

## Lessons From LC

The useful idea in `LC` was the distinction between CPU-side render objects and
context-local GPU resources. A mesh, shader, or drawable can be a persistent
scene object, but the actual GPU buffer, pipeline, or texture belongs to a
specific rendering context.

That is still a good idea.

The costly part was the amount of machinery needed to support it: inheritance,
object IDs, dynamic casts, signal-driven updates, manual resource maps, shader
libraries, and `prepare`/`updateResources` calls spread through the code.

What to keep:

- Persistent scene/drawable objects.
- Explicit rendering phases.
- Context-local GPU resources.
- CPU-side source data invalidating cached GPU resources.

What to avoid:

- Inheritance-heavy resource interfaces.
- Global object ID lookup.
- Signal-driven GPU updates.
- Hidden shader/pipeline state living in scene globals.

## Lessons From wiew2

The useful idea in `wiew2` was composition. Components like grids, backgrounds,
trackball cameras, render targets, and sequences made scenes easy to assemble.
Passing contextual data such as camera state and render targets was also a good
direction.

The problem was using a React-like lifecycle for rendering. Rendering is
side-effectful, ordered, and tightly coupled to GPU context state. The reactive
approach introduced boxed render closures, wake/re-render hacks, unclear context
scope, and hidden dependency tracking.

What to keep:

- Composable renderable objects.
- Explicit camera/render-target context.
- Reusable provided objects.
- Value-based configs that rebuild GPU resources when changed.

What to avoid:

- A React/component lifecycle as the core renderer model.
- Hidden dependency/context propagation.
- `Box<dyn FnOnce(&mut RenderPass)>` as the main render abstraction.
- Render order emerging indirectly from component evaluation.

## Initial Direction

The current crate should use a small retained/immediate hybrid.

The retained part: user-facing render objects are normal Rust structs that own
their CPU-side configuration and source data.

The immediate part: rendering order is explicit. A scene or application calls
methods on those objects in the order it wants them drawn.

The resource cache should stay internal and boring. It is a support mechanism,
not the main architecture.

Sketch:

```rust
pub trait Drawable {
    fn prepare(&mut self, cx: &WCx);
    fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, cx: &'a WCx);
}
```

This exact trait may change, but the shape is intentional:

- `WCx` owns `device`, `queue`, and the internal resource cache.
- Render objects expose explicit mutation methods such as `resize`,
  `set_vertices`, `set_camera`, or `set_color`.
- Mutations update CPU-side source/config data.
- GPU resources are resolved lazily through the context.
- Resolved resources are returned through scoped handles such as `H<'cx, T>`,
  so callers cannot accidentally keep context-owned resources beyond the context
  borrow.

## Resource Model

The current resource direction is:

```rust
Res<Source, Value>
```

instead of a marker trait like:

```rust
trait Resource {
    type Source;
    type Value;
}
```

This keeps the common case lightweight. A render object can directly store the
source/value relationship:

```rust
struct RenderTarget {
    resources: Res<RenderTargetConfig, RenderTargetTextures>,
}
```

The resource manager should not be exposed as the user-facing API. Users should
normally interact with helpers such as `RenderTarget::get(&cx)`, `Grid::draw`,
or `Mesh::prepare`.

## First Vertical Slice

Before committing to a larger architecture, the crate should prove this design
with one small complete path:

1. `WCx`
2. `RenderTarget`
3. Camera
4. Flat pipeline
5. Vertex buffer
6. Grid
7. One example that renders to a texture or surface

If that slice feels direct and predictable, the design is probably on the right
track. If it starts requiring framework-like machinery, hidden lifecycle hooks,
or many marker types, the design should be revised.

## Guiding Principle

Prefer explicit, boring rendering code over clever framework behavior.

The crate should make common `wgpu` rendering easier, but it should not hide the
fundamental order of operations so much that debugging resource lifetime,
pipeline state, or render order becomes harder.

## Related Notes

- [Point-cloud rendering](point-cloud-rendering.md)
