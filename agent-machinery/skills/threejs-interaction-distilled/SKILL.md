---
name: threejs-interaction-distilled
description: Implement Three.js picking, camera controls, drag/transform gestures, and selection.
---

# Three.js interaction

Define the interaction states, eligible targets, and gesture owner first.
Use Orbit/Map/PointerLock controls for their intended navigation; DragControls
or TransformControls for object manipulation.

- Convert pointer coordinates through the canvas bounding rectangle, not the
  window. Raycast explicit targets and choose recursive traversal deliberately.
- Map helper hits to domain objects. Keep one selection state feeding highlights,
  outlines, and panels.
- Prevent one gesture from moving both camera and object. Distinguish click,
  drag, hover, and box selection; coalesce move work within the frame budget.
- Update controls when damping/motion requires it. Use proxy geometry or a
  domain spatial index only when detailed picking is measurably costly.
- For placement, intersect a defined plane/surface. For overlays, project into
  the canvas box, including its page offset.
- Remove listeners, release capture, dispose controls, and clear stale selection
  references at teardown.

Full notes: `agents path threejs-interaction-reference` for picking, controls,
and input lifecycle examples.
