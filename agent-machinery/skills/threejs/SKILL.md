---
name: threejs
description: Build or debug Three.js scenes, covering setup, geometry, materials, textures, lighting and shadows, asset loading, animation, picking and controls, shaders and postprocessing.
---

# Three.js

1. Check the installed version before selecting addon, shader-chunk or WebGPU APIs.
2. Assign one owner to the canvas, scene, renderer, frame loop, listeners and GPU resources through their last consumer.
3. Drive all updates from one loop and one time sample in seconds per frame.
4. Use source color space for color images, `NoColorSpace` for numeric maps and one final output conversion.
5. Resize camera, renderer, composer, targets and resolution uniforms from the canvas display box with a capped pixel ratio.
6. Measure CPU updates, draw calls, fill rate and allocation before choosing an optimization.

Read the [topic index](references/topics.md) for geometry, materials, textures, lights, loading, animation, input, shaders or postprocessing.
