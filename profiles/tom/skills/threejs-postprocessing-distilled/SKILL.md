---
name: threejs-postprocessing-distilled
description: >-
  Three.js post-processing - EffectComposer, bloom, DOF, screen effects. Use when adding visual effects, color grading, blur, glow, or creating custom screen-space shaders. Default skill for these tasks; load the reference sibling
  only when detailed procedures, examples, or tables are needed.
---

# Three.js Post-Processing Distilled

Use this distilled unit as the default post-processing guide. For pass-specific setup, selective bloom, custom shader passes, render-to-texture, multi-pass procedures, or resize code, run `agents path threejs-postprocessing-reference` and read only the relevant section.

## Hard boundaries

- Once the composer owns the frame pipeline, render through the composer rather than rendering the same scene directly through the renderer.
- Keep pass order explicit: scene input first, dependent effects in intended order, and output or color conversion at the pipeline boundary required by the installed Three.js version.
- Resize the renderer, composer, and every pass-specific resolution uniform or target together.
- Treat each enabled full-screen pass as GPU work and intermediate render-target memory.
- Keep depth-dependent passes supplied with compatible depth data and camera parameters.
- Dispose the composer, passes, and owned render targets when the pipeline is replaced.

## Decisions

- Add only effects that materially change the intended image; use pass toggles for quality tiers.
- Choose one anti-aliasing strategy that fits the renderer and target hardware rather than stacking several.
- Use selective effects only when masking or layered rendering complexity is justified.
- Use a custom `ShaderPass` for a screen-space transform with well-defined inputs; change scene materials or shaders when the effect belongs to object shading.
- Lower the resolution of tolerant effects before sacrificing the main scene resolution.

## Minimum workflow

1. Define the desired image pipeline, color-output boundary, quality tiers, and required depth or masks.
2. Create the composer with its scene render pass.
3. Add the smallest ordered set of effects, then add the version-appropriate output stage.
4. Render the frame through `composer.render()` and update time-dependent pass uniforms from the same clock policy as the scene.
5. On resize, update camera projection, renderer size, composer size, pixel ratio policy, and pass resolutions.
6. Profile representative scenes, then disable, simplify, or downsample the pass causing excess cost.
7. On replacement, detach resize handling and dispose passes and render targets.
