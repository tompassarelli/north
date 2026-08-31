---
name: threejs-textures-distilled
description: >-
  Three.js textures - texture types, UV mapping, environment maps, texture settings. Use when working with images, UV coordinates, cubemaps, HDR environments, or texture optimization. Default skill for these tasks; load the reference sibling
  only when detailed procedures, examples, or tables are needed.
---

# Three.js Textures Distilled

Use this distilled unit as the default texture guide. For loader and configuration examples, specialized texture types, cube and HDR procedures, render targets, UV transforms, atlases, material-map tables, or memory recipes, run `agents path threejs-textures-reference` and read only the relevant section.

## Hard boundaries

- Mark color images with color-space semantics appropriate to their source; keep normal, roughness, metalness, AO, depth, and other data maps in non-color space.
- Match texture orientation and UV conventions to the asset pipeline, especially for imported GLTF content and render targets.
- Keep dimensions, filtering, mipmaps, wrapping, anisotropy, and compression within renderer capabilities and the target device budget.
- Set `needsUpdate` when CPU-side image or sampler state changes require upload; set the material dirty when a previously absent map slot changes shader structure.
- Treat render targets, video elements, canvas producers, blob URLs, and textures as separately owned resources.
- Dispose a texture or render target only after its final material, cache, or pipeline consumer releases it.

## Decisions

- Use regular image textures for static sampled images, data textures for generated arrays, canvas or video textures for changing media, cube or HDR textures for environments, and render targets for GPU-produced images.
- Choose wrapping and repeat from the intended UV domain; choose minification filters and mipmaps from the scale range.
- Prefer compressed textures for delivery and GPU memory when the deployment can provide a supported transcode path.
- Use atlases to reduce texture switches when UV complexity and edge padding are manageable.
- Reuse texture instances unless consumers need independent sampler state, transforms, or lifecycle.

## Minimum workflow

1. Identify whether the source is color or data, its UV and orientation convention, expected scale range, and lifecycle owner.
2. Load or create the appropriate texture type and report asynchronous failure with the source identity.
3. Configure color space, flip, wrap, repeat, filtering, mipmaps, and anisotropy before first use when possible.
4. Attach the texture to the correct material slot and verify the geometry has the required UV channel.
5. Inspect appearance at near and far distances and check renderer capability and memory limits before increasing size or quality.
6. Reuse or cache according to ownership; update dynamic sources only when their producer changes.
7. On release, detach consumers and dispose the texture, render target, media producer, or temporary URL that is no longer owned.
