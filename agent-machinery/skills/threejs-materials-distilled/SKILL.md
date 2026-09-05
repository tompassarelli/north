---
name: threejs-materials-distilled
description: Choose and configure Three.js materials, PBR appearance, alpha modes, and shared state.
---

# Three.js materials

Choose the simplest material matching the primitive and lighting response:
Basic for unlit, Standard for PBR, Physical for needed extra features;
Lambert/Phong for deliberate simpler lighting. Use specialized line/point/toon/
depth materials when their semantics fit.

- Missing normals, UVs, lights, or environment data cannot be fixed by material
  parameters alone. Adjust roughness, metalness, exposure, and lighting together.
- Color maps use their source color space; numeric data maps use NoColorSpace.
- Shared materials are shared mutable state. Clone for independent values or
  defines, not reflexively for every object.
- Prefer alpha test for cutouts; blending introduces ordering and depth issues.
- Change `needsUpdate` for shader structure/defines, not ordinary uniform-like
  values. Extend built-ins to preserve their lighting; own shaders only when
  the shading model itself is custom.
- Dispose materials and uniquely owned textures only after final use.

Full notes: `agents path threejs-materials-reference` for material families,
render state, maps, cloning, and custom shader boundaries.
