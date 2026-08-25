---
name: threejs-fundamentals-distilled
description: >-
  Three.js scene setup, cameras, renderer, Object3D hierarchy, coordinate systems. Use when setting up 3D scenes, creating cameras, configuring renderers, managing object hierarchies, or working with transforms. Default skill for these tasks; load the reference sibling
  only when detailed procedures, examples, or tables are needed.
---

# Three.js Fundamentals Distilled

Use this distilled unit as the default scene-construction guide. For class APIs, transform and math examples, cleanup code, responsive-canvas patterns, or performance recipes, run `agents path threejs-fundamentals-reference` and read only the relevant section.

## Hard boundaries

- Give the scene, active camera, renderer, canvas, render loop, and resize listener clear lifecycle ownership.
- After changing a perspective camera's aspect or an orthographic camera's bounds, update its projection matrix before rendering.
- Distinguish local transforms from world transforms. Preserve world placement deliberately when reparenting objects.
- Cap pixel ratio to the quality budget instead of blindly using the device maximum.
- Dispose owned geometries, materials, textures, and render targets, and remove owned listeners when the scene is torn down.
- Keep camera near and far planes as tight as the scene permits to protect depth precision.

## Decisions

- Use a perspective camera for depth-scaled views and an orthographic camera for scale-stable diagrams, UI, or isometric views.
- Use groups and parent transforms when children share a coordinate frame; avoid deep hierarchies that exist only for convenience.
- Select meshes, lines, points, sprites, or instancing according to visual semantics and draw-call count.
- Merge static compatible geometry, instance repeated geometry/material pairs, or add LOD only after the scene's actual bottleneck is known.
- Reuse a single animation loop and clock policy for systems that must stay synchronized.

## Minimum workflow

1. Create the scene, renderer, owned canvas, and the camera appropriate to the view.
2. Set renderer size and a bounded pixel ratio; position the camera and establish its clipping range.
3. Create objects with explicit geometry and material ownership, then attach them under the intended parent.
4. Add only the lighting and helpers needed by the selected materials and current diagnosis.
5. In one frame loop, update state and render the active scene and camera.
6. On resize, update camera projection and renderer dimensions from the canvas display size.
7. On teardown, cancel the loop, detach listeners and canvas ownership, and dispose GPU resources no longer shared.
