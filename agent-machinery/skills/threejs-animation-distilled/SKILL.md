---
name: threejs-animation-distilled
description: >-
  Three.js animation - keyframe animation, skeletal animation, morph targets, animation mixing. Use when animating objects, playing GLTF animations, creating procedural motion, or blending animations. Default skill for these tasks; load the reference sibling
  only when detailed procedures, examples, or tables are needed.
---

# Three.js Animation Distilled

Use this distilled unit as the default operating guide. For exact APIs, authored-animation patterns, skeletal and morph examples, blending recipes, or utility tables, run `agents path threejs-animation-reference` and read only the relevant section.

## Hard boundaries

- Advance each `AnimationMixer` once per rendered frame with elapsed seconds; do not mix fixed-frame increments with clock deltas.
- Keep action state local to its mixer and animated root. Treat reusable `AnimationClip` data as shared input rather than per-instance state.
- Preserve the imported bind pose and verify track property paths before changing a skeleton or retargeting a clip.
- Stop actions and release mixer bindings when an animated root leaves the scene; pausing an action is not teardown.
- Keep procedural motion deterministic from explicit time or delta values, and clamp large deltas after background-tab stalls when simulation stability matters.

## Decisions

- Use direct procedural updates for continuous motion with no authored timeline; use clips and a mixer for keyed, skeletal, or morph animation.
- Prefer one mixer per independently controlled animated root. Share clips across compatible roots.
- Use cross-fading for transitions between full-body actions. Use additive blending only when the additive clip and base pose were prepared for it.
- Choose loop mode, repetitions, clamp behavior, weight, and time scale explicitly. Do not rely on defaults when completion changes application state.
- Pause or simplify off-screen animation only when skipped time and event delivery are acceptable.

## Minimum workflow

1. Identify the animated root, available clips, property paths, and required playback semantics.
2. Create or select the clip, then create the root's mixer and action.
3. Configure loop, weight, time scale, and completion behavior before playing.
4. Update the mixer from the render loop and update procedural motion from the same clock policy.
5. Observe `loop` or `finished` only when application behavior depends on it.
6. On replacement or removal, stop actions, remove listeners, and uncache the clip or root bindings that are no longer used.
