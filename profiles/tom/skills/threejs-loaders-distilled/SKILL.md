---
name: threejs-loaders-distilled
description: >-
  Three.js asset loading - GLTF, textures, images, models, async patterns. Use when loading 3D models, textures, HDR environments, or managing loading progress. Default skill for these tasks; load the reference sibling
  only when detailed procedures, examples, or tables are needed.
---

# Three.js Loaders Distilled

Use this distilled unit as the default asset-loading guide. For loader-specific APIs, Draco and KTX2 setup, model-format examples, cache implementations, alternate source types, or error-handling recipes, run `agents path threejs-loaders-reference` and read only the relevant section.

## Hard boundaries

- Treat every load as asynchronous and surface failures with the asset identity and failing stage.
- Configure decoder or transcoder dependencies before starting loads that require them.
- Do not attach partially prepared assets to live scene state unless progressive appearance is intentional.
- Validate imported scale, orientation, animation roots, material maps, and color-space assumptions before relying on the asset.
- Keep cache ownership explicit; a cached scene, texture, or buffer remains live until the cache releases it.
- Revoke owned blob URLs and dispose loaded GPU resources when a failed, cancelled, replaced, or evicted asset has no consumer.

## Decisions

- Prefer GLTF or GLB for general scene delivery; use other model formats only when the source workflow requires them.
- Use Draco for geometry transfer savings and KTX2/Basis for texture transfer and GPU-format savings when their decoder cost and deployment are justified.
- Use a shared `LoadingManager` when aggregate progress, URL rewriting, or coordinated errors matter.
- Use `loadAsync` and explicit promise composition for dependent workflows; use callbacks only where the surrounding API requires them.
- Cache immutable source assets, then clone scene or stateful instances according to their mutation and animation needs.

## Minimum workflow

1. Identify the asset format, expected coordinate and material conventions, compression, and cancellation or retry policy.
2. Create a loading manager if progress or URL policy is shared, then configure the required loaders and decoders.
3. Start the load through one async boundary and report progress only at a granularity the UI can honestly represent.
4. Validate the result, traverse it to apply shadow, material, animation, or metadata policy, and only then publish it to scene state.
5. Handle failure once with actionable context and release any partial resources.
6. Record cache ownership and instance-cloning behavior if the asset is reused.
7. On replacement or eviction, detach consumers, revoke temporary URLs, and dispose resources whose final owner released them.
