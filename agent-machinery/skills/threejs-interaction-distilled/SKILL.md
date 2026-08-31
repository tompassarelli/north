---
name: threejs-interaction-distilled
description: >-
  Three.js interaction - raycasting, controls, mouse/touch input, object selection. Use when handling user input, implementing click detection, adding camera controls, or creating interactive 3D experiences. Default skill for these tasks; load the reference sibling
  only when detailed procedures, examples, or tables are needed.
---

# Three.js Interaction Distilled

Use this distilled unit as the default interaction guide. For control-specific setup, selection implementations, coordinate-conversion examples, drag and transform procedures, or event-handling recipes, run `agents path threejs-interaction-reference` and read only the relevant section.

## Hard boundaries

- Convert pointer coordinates from the renderer canvas bounding rectangle, not from the browser window, before raycasting.
- Raycast an explicit target set and choose recursive traversal deliberately; scene-wide intersection on every pointer move is not a default.
- Keep one canonical selection state. Visual highlighting, outlines, and UI panels must reflect it rather than each owning a separate selection.
- Resolve gesture ownership between camera controls, drag controls, transform controls, and application handlers so one gesture does not drive several systems.
- Remove listeners, release pointer capture, and dispose or detach controls when their owner is removed.
- Treat hover and move work as frame-budgeted input; coalesce or throttle it when necessary without dropping required click semantics.

## Decisions

- Use `OrbitControls` for inspectable scenes, map controls for planar navigation, pointer lock or first-person controls for embodied movement, and custom input only when their behavior does not fit.
- Use a `Raycaster` for rendered-object picking; use simpler proxy geometry or a domain spatial index when detailed meshes make picking expensive.
- Use `TransformControls` for editor-like transforms and `DragControls` for direct object dragging.
- Distinguish click, drag, hover, and box-selection state transitions explicitly.
- Convert world to screen for overlays; cast screen to a defined plane or surface when placing objects in world space.

## Minimum workflow

1. Define the interaction states, eligible targets, camera, canvas, and gesture ownership.
2. Attach pointer or keyboard listeners to the narrowest owning element and record teardown.
3. Convert the pointer through the canvas rectangle to normalized device coordinates.
4. Configure the ray from the active camera, intersect the explicit target set, and map helper hits to domain objects.
5. Update canonical hover or selection state, then render its visual and UI consequences.
6. Update enabled controls once per frame when their damping or motion requires it.
7. On teardown, disable controls, remove listeners, release captures, and clear stale selected references.
