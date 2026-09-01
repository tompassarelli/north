---
name: threejs-lighting-distilled
description: >-
  Three.js lighting - light types, shadows, environment lighting. Use when adding lights, configuring shadows, setting up IBL, or optimizing lighting performance. Default skill for these tasks; load the reference sibling
  only when detailed procedures, examples, or tables are needed.
---

# Three.js Lighting Distilled

Use this distilled unit as the default lighting guide. For light-type parameters, shadow-camera examples, environment-map procedures, helper usage, staged lighting setups, or animation recipes, run `agents path threejs-lighting-reference` and read only the relevant section.

## Hard boundaries

- Match the lighting model to the material: unlit materials ignore scene lights, while PBR materials need plausible direct or environment illumination.
- A shadow requires renderer shadow support, a shadow-capable light, casting objects, receiving objects, and a fitted shadow camera.
- Keep shadow-map resolution and shadow-camera volume bounded to the visible need; unfitted shadows waste memory and precision.
- Limit shadow-casting lights and real-time light count to the frame budget.
- Keep light targets attached and updated when directional or spot lights aim at them.
- Remove diagnostic helpers and dispose owned environment-processing resources when they are no longer needed.

## Decisions

- Use environment lighting for broad PBR reflections and fill; add direct lights for shaped illumination and shadows.
- Use ambient or hemisphere light as restrained fill, directional light for distant sources, point or spot light for local sources, and area light for broad soft highlights.
- Prefer one intentional key shadow before adding secondary shadow casters.
- Bake static lighting when scene content and delivery allow it; keep dynamic lights for behavior that must change.
- Add helpers while fitting positions, ranges, targets, and shadow volumes, then remove them from production output.

## Minimum workflow

1. Identify the material model, desired mood, dynamic objects, and which surfaces truly need shadows.
2. Establish environment or fill illumination, then add the smallest set of direct key lights.
3. Position each light and its target; bound range, decay, cone, or area to the scene.
4. Enable shadows only for the chosen lights and objects, then fit the shadow camera with a helper.
5. Render representative views and adjust exposure, intensities, and material response together.
6. Inspect renderer cost before adding lights, increasing shadow maps, or widening shadow volumes.
7. Remove helpers and release environment or shadow resources when their owner is torn down.
