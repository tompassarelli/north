---
name: threejs-geometry-distilled
description: >-
  Three.js geometry creation - built-in shapes, BufferGeometry, custom geometry, instancing. Use when creating 3D shapes, working with vertices, building custom meshes, or optimizing with instanced rendering. Default skill for these tasks; load the reference sibling
  only when detailed procedures, examples, or tables are needed.
---

# Three.js Geometry Distilled

Use this distilled unit as the default geometry guide. For constructor catalogs, BufferGeometry examples, lines and points, instancing procedures, morph layouts, or utility recipes, run `agents path threejs-geometry-reference` and read only the relevant section.

## Hard boundaries

- Keep each attribute's item size, scalar type, vertex count, and semantic aligned; keep every index within the vertex range.
- Mark changed attributes or instance matrices dirty, and recompute normals or bounds when the mutation invalidates them.
- Use consistent winding and coordinate space. A missing face or inverted light response is geometry data until proved otherwise.
- Dispose geometry when its final consumer releases it.
- Use `InstancedMesh` only when instances share geometry and material; store per-instance variation in supported instance attributes or uniforms.
- Bound segment counts and generated vertex volume to the visible result rather than maximum smoothness.

## Decisions

- Prefer a built-in geometry when it expresses the shape; use custom `BufferGeometry` when topology or attributes are genuinely custom.
- Prefer indexed geometry when vertices can be shared without conflicting per-corner attributes.
- Use a mesh for surfaces, line primitives for paths and wire forms, and points for particle-like vertices.
- Merge static compatible objects to reduce draw calls; instance repeated objects that still need independent transforms.
- Center, scale, or bake transforms into geometry only when changing the geometry's local frame is intended.

## Minimum workflow

1. Define the primitive, topology, coordinate frame, required attributes, and expected bounds.
2. Construct a built-in geometry or create position and optional index, normal, UV, color, or custom attributes.
3. Validate counts and indices, then compute missing normals and bounding volumes where required.
4. Pair the geometry with a compatible material and primitive type, then add it under the correct transform.
5. For runtime edits, update the smallest owned buffer range, mark it dirty, and refresh invalidated derived data.
6. Reuse, merge, or instance only after object independence and update frequency are clear.
7. Dispose geometry when no remaining mesh, line, points object, or cache owns it.
