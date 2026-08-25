---
name: threejs-shaders-distilled
description: >-
  Three.js shaders - GLSL, ShaderMaterial, uniforms, custom effects. Use when creating custom visual effects, modifying vertices, writing fragment shaders, or extending built-in materials. Default skill for these tasks; load the reference sibling
  only when detailed procedures, examples, or tables are needed.
---

# Three.js Shaders Distilled

Use this distilled unit as the default shader guide. For GLSL examples, uniform and varying tables, noise and dissolve patterns, built-in-material injection points, shader chunks, instancing, or debugging recipes, run `agents path threejs-shaders-reference` and read only the relevant section.

## Hard boundaries

- Keep attribute, uniform, and varying names and types consistent across JavaScript, vertex shader, and fragment shader.
- Write every fragment output and every vertex position on all executable paths.
- Choose coordinate spaces deliberately and apply model, view, projection, normal, and instancing transforms consistently.
- Update uniform `.value` fields for runtime data; recompile only when shader source or compile-time defines change.
- Treat built-in shader chunks and `onBeforeCompile` injection points as Three.js-version-sensitive interfaces.
- Check renderer shader diagnostics and dispose shader materials when their final consumers release them.

## Decisions

- Use `ShaderMaterial` when Three.js-provided attributes, uniforms, and prefixes are useful; use `RawShaderMaterial` when the full GLSL interface must be explicit.
- Extend a built-in material when its lighting and PBR behavior should remain; use a custom shader when the whole shading model is yours.
- Put per-frame or per-object values in uniforms, per-vertex values in attributes and varyings, and large sampled data in textures.
- Perform work on the CPU when it changes rarely and reduces repeated fragment cost; keep parallel per-pixel or per-vertex effects on the GPU.
- Minimize branches, overdraw, texture samples, and precision only in response to a measured constraint.

## Minimum workflow

1. Define the visual result, vertex-to-fragment data flow, coordinate spaces, and required material integration.
2. Start with the smallest vertex and fragment shaders that compile and render a diagnostic output.
3. Add uniforms, attributes, varyings, and textures one responsibility at a time.
4. Update runtime uniform values without replacing their wrappers.
5. Inspect compiler and linker diagnostics before debugging the visual math.
6. Test transforms, lighting inputs, and edge cases on representative geometry, then profile the expensive stage.
7. Dispose the material and any uniquely owned textures or buffers when ownership ends.
