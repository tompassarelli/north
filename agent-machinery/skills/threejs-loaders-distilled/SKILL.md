---
name: threejs-loaders-distilled
description: Load Three.js models, textures, compressed assets, and manage async preparation and caches.
---

# Three.js asset loading

Prefer GLTF/GLB for delivered scenes unless the source workflow needs another
format. Configure required decoders/transcoders before loading.

- Use `loadAsync` and explicit promise dependencies. Use LoadingManager when
  shared progress or URL policy is needed; counts are not byte percentages.
- Publish the asset after required preparation unless progressive appearance
  is intentional. Check scale, orientation, roots, maps, and color semantics.
- Report failure with asset and stage in developer diagnostics; keep user copy
  about the asset/action. Retry only under the intended bounded policy.
- Cache immutable inputs or load promises; clone stateful instances appropriately.
  Skinned clones need skeleton-aware cloning.
- Draco trades geometry transfer for decoding; KTX2 can also reduce GPU memory.
  Include decoder deployment and capability detection in that choice.
- On cancellation, failure, replacement, or eviction, detach consumers and release
  uniquely owned resources and blob URLs. Loader APIs differ in abort support.

Full notes: `agents path threejs-loaders-reference` for formats/decoders,
async loading, caching, and source/error handling.
