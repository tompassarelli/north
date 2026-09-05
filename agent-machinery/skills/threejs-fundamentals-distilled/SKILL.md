---
name: threejs-fundamentals-distilled
description: Set up Three.js scenes, cameras, render loops, transforms, resize, and resource ownership.
---

# Three.js scenes

Give the canvas, scene, camera, renderer, frame loop, and listeners one clear
owner. Check the installed Three.js version before copying addon examples.

- Perspective gives depth scaling; orthographic gives scale-stable views.
  Fit clipping planes to protect depth precision.
- Separate local and world coordinates. Reparenting preserves world placement
  only when performed deliberately.
- Size from the canvas display box, cap pixel ratio to the quality budget, and
  update camera projection after aspect/bounds changes.
- Use one coordinated loop. Choose meshes, lines, points, sprites, or instances
  for the visual primitive; add lights only for materials that use them.
- Profile the real bottleneck before merging, instancing, or adding LOD.
- On teardown, stop the loop, remove listeners and owned canvas, then dispose
  GPU resources only after their final consumer releases them.

Full notes: `agents path threejs-fundamentals-reference` for scene lifecycle,
coordinate math, and diagnostic examples.
