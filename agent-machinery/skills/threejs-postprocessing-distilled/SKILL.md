---
name: threejs-postprocessing-distilled
description: Build Three.js postprocessing pipelines, bloom, depth effects, and custom screen passes.
---

# Three.js postprocessing

Define the image pipeline and color-output boundary. For WebGL, the composer
owns the frame: scene input → ordered effects → version-appropriate output.
Do not overwrite its result with a direct render of the same scene.

- Add effects for the intended image, not by default. Choose a fitting
  anti-aliasing strategy rather than stacking several.
- Supply depth effects with compatible depth data and camera parameters.
  Selective effects need explicit masking and final composition.
- Resize camera, renderer, composer, targets, and resolution uniforms together;
  distinguish CSS pixels from drawing-buffer pixels.
- Update animated uniforms from the scene's time policy.
- Each pass costs GPU time and intermediate memory. Measure before changing
  quality; downsample tolerant effects before the main scene.
- Dispose owned passes and render targets and remove resize handling.

Full notes: `agents path threejs-postprocessing-reference` for pipeline setup,
effect examples, composition, and WebGPU/version distinctions.
