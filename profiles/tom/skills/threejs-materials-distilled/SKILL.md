---
name: threejs-materials-distilled
description: >-
  Three.js materials - PBR, basic, phong, shader materials, material properties. Use when styling meshes, working with textures, creating custom shaders, or optimizing material performance. Default skill for these tasks; load the reference sibling
  only when detailed procedures, examples, or tables are needed.
---

# Three.js Materials Distilled

Use this distilled unit as the default material-selection guide. For parameter tables, specialized material examples, shader-material setup, environment-map procedures, multiple-material handling, or cloning recipes, run `agents path threejs-materials-reference` and read only the relevant section.

## Hard boundaries

- Match the material to the primitive and lighting model; a correct material cannot compensate for missing normals, UVs, lights, or environment data.
- Assign color textures and non-color data textures with the correct color-space semantics.
- Treat shared materials as shared mutable state. Clone only when an object needs independent values or shader defines.
- Use transparency only when blending is required; prefer cutout alpha testing for hard-edged opacity.
- Set `needsUpdate` when a change alters shader defines or material structure, not for ordinary uniform-like property changes.
- Dispose a material and its owned textures only after their final consumers release them.

## Decisions

- Use `MeshBasicMaterial` for unlit output, Lambert or Phong for intentionally simple legacy lighting, `MeshStandardMaterial` as the default PBR choice, and `MeshPhysicalMaterial` only for its added physical features.
- Use toon, normal, depth, points, or line materials when their render semantics match the primitive.
- Use `ShaderMaterial` for a fully custom shader and built-in-material extension when the built-in lighting stack should remain.
- Reuse a material to reduce state churn; clone for true per-object independence.
- Balance roughness, metalness, environment lighting, and exposure as one appearance system.

## Minimum workflow

1. Identify the mesh primitive, desired lighting response, alpha mode, and available vertex attributes and maps.
2. Choose the simplest material model that produces the required result.
3. Configure base color, roughness or shininess, metalness, opacity, side, and maps with correct texture semantics.
4. Provide the lights or environment required by that model and inspect the result under representative exposure.
5. Reuse or clone the material according to mutation ownership.
6. Profile before moving to a more expensive material or enabling transparency.
7. When ownership ends, detach the material and dispose it plus any uniquely owned textures.
