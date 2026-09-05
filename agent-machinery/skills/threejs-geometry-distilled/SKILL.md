---
name: threejs-geometry-distilled
description: Build or modify Three.js geometry, attributes, topology, lines, points, and instances.
---

# Three.js geometry

Use a built-in shape when it fits; custom BufferGeometry for custom topology.
Bound segments to the visible result, not maximum smoothness.

- Keep attribute type, item size, vertex count, and index range consistent.
  Share indexed vertices only when all per-corner attributes agree.
- Make winding and coordinate space explicit. Diagnose missing/inverted faces
  in the geometry before compensating with materials.
- Mark mutated attributes/instance matrices dirty and refresh invalidated
  normals and bounds. Update only the owned buffer range.
- Meshes represent surfaces; lines represent paths; points represent samples.
  Instance shared geometry/material pairs with independent transforms; merge
  static compatible geometry when individual identity is unnecessary.
- Baking transforms or centering changes the geometry's local frame and every
  consumer of shared geometry.
- Dispose only after the final mesh, cache, or other consumer releases it.

Full notes: `agents path threejs-geometry-reference` for topology, primitive
catalogs, instancing, and geometry utilities.
