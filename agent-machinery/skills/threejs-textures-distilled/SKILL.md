---
name: threejs-textures-distilled
description: Configure Three.js texture color spaces, UVs, environments, render targets, and memory use.
---

# Three.js textures

Identify color versus numeric data, orientation/UV convention, scale range,
and ownership before choosing texture settings.

- Color images use their source color space (commonly sRGB); normals, roughness,
  metalness, AO, depth, and other numeric maps use NoColorSpace.
- Choose image/data/canvas/video/cube/HDR/target sources for the producer.
  Texture, media producer, render target, and blob URL have separate lifetimes.
- Match flip and UV-channel behavior to the installed version and asset format.
  GLTF-loaded textures already carry format-specific settings.
- Keep size, filters, mipmaps, wrapping, anisotropy, and compression within the
  target budget. Atlases need padding against filtering/mipmap bleed.
- Mark upload/sampler changes dirty; mark materials dirty when adding a
  previously absent map changes shader structure.
- Share textures unless sampler state, transform, or lifecycle must differ.
  Dispose only after the final material, cache, or pipeline consumer releases it.

Full notes: `agents path threejs-textures-reference` for sampling, source types,
environments, UV/material maps, and resource ownership.
