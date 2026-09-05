---
name: threejs-shaders-distilled
description: Write or debug Three.js GLSL, uniforms, custom materials, and built-in shader extensions.
---

# Three.js shaders

Define the visual result, coordinate spaces, and vertex-to-fragment data flow.
Start with the smallest shader that compiles and renders diagnostic output.

- ShaderMaterial supplies common declarations; RawShaderMaterial requires them
  explicitly. Extend a built-in when its lighting model must remain.
- Match attribute/uniform/varying names and types; write vertex position and
  fragment output on every path. Normalize interpolated normals when used.
- Keep position, normal, view, projection, and instancing transforms consistent.
  A dot product is meaningful only within one coordinate space.
- Update uniform `.value`; recompile for changed source or defines.
  Chunks and `onBeforeCompile` are version-sensitive, not stable APIs.
- Check compiler/linker diagnostics before visual math. Profile before reducing
  precision, branches, samples, or moving computation between CPU and GPU.
- Dispose final-use materials and uniquely owned buffers/textures.

Full notes: `agents path threejs-shaders-reference` for data interfaces,
effect math, built-in integration, and diagnostics.
