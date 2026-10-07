---
name: threejs
description: Build or debug Three.js scenes, covering setup, geometry, materials, textures, lighting and shadows, asset loading, animation, picking and controls, shaders and postprocessing.
---

# Three.js

Check the installed Three.js version before copying addon, shader-chunk or
WebGPU examples. Their import paths and APIs change between releases.

## Core rules

- **Ownership.** Give the canvas, scene, camera, renderer, frame loop,
  listeners and each GPU resource one clear owner. At teardown, stop the loop,
  remove listeners and the owned canvas, then dispose geometry, materials,
  textures, render targets, passes and controls once their last consumer lets
  go. Shared geometry, materials and textures are shared mutable state.
  Pausing an action or pass does not free it.
- **Color space.** Color images use their source color space, usually sRGB.
  Normal, roughness, metalness, AO, depth and other numeric maps use
  `NoColorSpace`. Convert to output color once, at the final render or
  composer output pass.
- **One loop, one clock.** Drive every system from one loop and one time
  sample per frame. Update each mixer once per step, in seconds. Don't mix
  frame counts with deltas.
- **Sizing.** Size from the canvas display box, not the window, and cap the
  pixel ratio. Update camera projection, renderer, composer, render targets
  and resolution uniforms together. CSS pixels and drawing-buffer pixels differ.
- **Dirty flags.** Mark changed attributes, instance matrices and textures
  dirty, and recompute normals and bounds they invalidate. Set
  `material.needsUpdate` only when shader structure changes (defines, or a map
  that was absent). Change uniforms through `.value`.
- **Fix the cause.** Missing or inverted faces are geometry problems. Material
  parameters can't make up for missing normals, UVs, lights or environment.
- **Measure first.** Find whether CPU updates, draw calls, fill rate or
  allocation is the bottleneck before merging, instancing, adding LOD,
  lowering resolution or cutting shader precision. `renderer.info` counts
  objects, not bytes.

## Topic defaults

- **Scene.** Perspective cameras scale with depth; orthographic ones don't.
  Fit near and far planes to protect depth precision. Keep local and world
  coordinates apart, and reparent on purpose.
- **Geometry.** Use a built-in shape when one fits, else `BufferGeometry`.
  Keep attribute type, item size, vertex count and index range consistent.
  Instance shared geometry and material pairs; merge static geometry when
  objects need no identity. Baking or centering changes every consumer of
  shared geometry.
- **Materials.** Basic for unlit, Standard for PBR, Physical only for its
  extra features. Prefer `alphaTest` for cutouts, since blending brings sort
  and depth problems. Clone a material only when its values or defines must
  differ.
- **Textures.** A texture, its source media, a render target and a blob URL
  each have their own lifetime. Pad atlases against mipmap bleed.
  GLTF-loaded textures already carry their format's settings.
- **Lighting.** Unlit materials ignore lights; PBR needs direct or
  environment light. Start with environment or fill, then the fewest key
  lights. Shadows need renderer support, a casting light, cast and receive
  flags and a fitted shadow camera. Fit the camera before raising map size.
- **Loading.** Prefer GLTF/GLB. Configure Draco or KTX2 decoders before
  loading. Use `loadAsync` with explicit dependencies; `LoadingManager`
  counts items, not bytes. Cache immutable inputs or load promises, and clone
  skinned models skeleton-aware. Log the asset and stage on failure; show the
  user the asset or action.
- **Animation.** Use procedural updates for unkeyed motion, clips and mixers
  for authored tracks. Keep clips reusable and action state per mixer.
  Cross-fade full-body clips; additive clips need the right base pose. Apply
  procedural offsets after the mixer.
- **Interaction.** Convert pointer coordinates through the canvas bounding
  rectangle. Raycast explicit targets and map helper hits to domain objects.
  Feed highlights and panels from one selection state. One gesture never moves
  both the camera and an object.
- **Shaders.** Start from the smallest shader that compiles and renders
  diagnostic output, and read compiler and linker logs before debugging the
  math. Extend a built-in material when its lighting must stay. A dot product
  only means something within one coordinate space.
- **Postprocessing.** In WebGL the composer owns the frame: scene, then
  ordered effects, then output. Don't also render the scene directly. Add
  effects for the intended image and pick one anti-aliasing method. Depth
  effects need matching depth data and camera parameters.

## References

Examples in these files are illustrative fragments, not tested apps. `scene`,
`camera`, `renderer`, `mesh` and assets come from your code. Read only the
file whose trigger matches:

| Read | When |
|---|---|
| `references/scene-and-lifecycle.md` | setting up scene, cameras, renderer, Object3D hierarchy, clock, resize or cleanup |
| `references/coordinates-and-math.md` | working with Vector3, Matrix4, Quaternion, Euler, Color or MathUtils |
| `references/performance.md` | a measured frame-time or memory problem needs a fix |
| `references/geometry-primitives.md` | choosing built-in shapes, text, edges, points or lines |
| `references/geometry-buffers.md` | writing or mutating BufferGeometry attributes, indices or morph attributes |
| `references/geometry-instancing.md` | drawing many copies with InstancedMesh or InstancedBufferGeometry |
| `references/material-families.md` | picking or tuning a material type and its parameters |
| `references/material-render-state.md` | setting transparency, depth, blending, stencil, multi-material or cloning |
| `references/texture-loading-and-sampling.md` | loading a texture or setting wrapping, repeat, filtering, flipY or mipmaps |
| `references/texture-sources-and-environments.md` | using data, canvas, video, cube or HDR/EXR textures, PMREM or CubeCamera |
| `references/texture-uv-and-maps.md` | working on UVs, atlases or PBR map sets |
| `references/render-targets-and-memory.md` | rendering to a texture, depth textures, MSAA targets or freeing texture memory |
| `references/light-selection.md` | choosing light types or a lighting setup |
| `references/shadows.md` | enabling or debugging shadows, or placing light helpers |
| `references/light-environment-and-motion.md` | using image-based lighting, light probes or animated lights |
| `references/loader-formats.md` | loading GLTF/GLB (with Draco or KTX2), OBJ, FBX, STL or PLY |
| `references/loader-async-and-caching.md` | coordinating loads, progress or asset caches |
| `references/loader-sources-and-errors.md` | loading from data URLs, blobs or buffers, or handling load errors |
| `references/animation-playback.md` | playing clips with AnimationMixer and actions, or GLTF animations |
| `references/animation-skeletons-and-morphs.md` | driving bones, attachments, morph targets or blending |
| `references/animation-procedural.md` | writing damping, spring or oscillation motion |
| `references/interaction-picking.md` | raycasting, selection, hover or world-to-screen conversion |
| `references/interaction-controls.md` | wiring Orbit, Map, PointerLock, Transform or Drag controls |
| `references/interaction-input.md` | handling keyboard input or event-listener lifecycle |
| `references/postprocessing-pipeline.md` | setting up EffectComposer, resize, multi-pass composition or WebGPU nodes |
| `references/postprocessing-effects.md` | adding bloom, AA, SSAO, depth of field or a custom ShaderPass |
| `references/shader-interfaces.md` | writing ShaderMaterial or RawShaderMaterial uniforms, varyings and attributes |
| `references/shader-effect-math.md` | writing displacement, fresnel, noise, dissolve or other GLSL effects |
| `references/shader-integration.md` | patching built-ins with onBeforeCompile, instanced shaders or shader debugging |

Adapted from CloudAI-X/threejs-skills (MIT); attribution is in this package's
`PROVENANCE.md` and `NOTICE`.
