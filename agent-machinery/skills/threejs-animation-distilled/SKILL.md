---
name: threejs-animation-distilled
description: Animate Three.js objects, GLTF clips, skeletons, morph targets, and transitions.
---

# Three.js animation

Use procedural updates for unkeyed motion; clips and mixers for authored tracks.
Keep clips reusable and action state local to each mixer/root.

- Drive synchronized systems from one time sample. Update each mixer once per
  simulation step with seconds; do not combine frame increments with deltas.
  Bound resumed deltas only when the simulation requires it.
- Verify track paths and preserve bind poses before retargeting. For independent
  skinned instances, clone skeleton bindings, not only the scene hierarchy.
- Set loop, repetitions, weight, time scale, and completion behavior deliberately.
  Cross-fade full-body clips; use additive clips only with the correct base pose.
- Apply procedural offsets after the mixer if they must survive keyed updates.
  Off-screen throttling must preserve required time and event semantics.
- At teardown, stop actions, remove listeners, and uncache unused bindings.
  Pausing is not resource release.

Full notes: `agents path threejs-animation-reference`. Select playback,
skeleton/morph, or procedural details there.
