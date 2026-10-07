# Pipeline, output, resize, and backends

## Decisions and rationale

Render through one final pipeline. Effects generally operate on intermediate linear images; output conversion belongs at the version-appropriate final boundary, not repeatedly between passes. Keep CSS dimensions, pixel ratio, drawing-buffer dimensions, and inverse-resolution uniforms distinct.

An intermediate target is an input, not a screen overlay. Combining scenes/effects requires an explicit compositing operation with alpha, depth, and color-space behavior. WebGPU's node pipeline is a different API from WebGL EffectComposer; a minimum release number does not make their examples interchangeable.

## Basic Example

```javascript
import * as THREE from "three";
import { EffectComposer } from "three/addons/postprocessing/EffectComposer.js";
import { RenderPass } from "three/addons/postprocessing/RenderPass.js";
import { UnrealBloomPass } from "three/addons/postprocessing/UnrealBloomPass.js";

// Setup composer
const composer = new EffectComposer(renderer);

// Render scene
const renderPass = new RenderPass(scene, camera);
composer.addPass(renderPass);

// Add bloom
const bloomPass = new UnrealBloomPass(
  new THREE.Vector2(window.innerWidth, window.innerHeight),
  1.5, // strength
  0.4, // radius
  0.85, // threshold
);
composer.addPass(bloomPass);

// Animation loop - use composer instead of renderer
function animate() {
  requestAnimationFrame(animate);
  composer.render(); // NOT renderer.render()
}
```

## EffectComposer Setup

```javascript
import { EffectComposer } from "three/addons/postprocessing/EffectComposer.js";
import { RenderPass } from "three/addons/postprocessing/RenderPass.js";

const composer = new EffectComposer(renderer);

// First pass: render scene
const renderPass = new RenderPass(scene, camera);
composer.addPass(renderPass);

// Add more passes...
composer.addPass(effectPass);

// Last pass should render to screen
effectPass.renderToScreen = true; // Default for last pass

// Handle resize
function onResize() {
  const width = window.innerWidth;
  const height = window.innerHeight;

  camera.aspect = width / height;
  camera.updateProjectionMatrix();

  renderer.setSize(width, height);
  composer.setSize(width, height);
}
```

## Handle Resize

```javascript
function onWindowResize() {
  const width = window.innerWidth;
  const height = window.innerHeight;
  const pixelRatio = renderer.getPixelRatio();

  camera.aspect = width / height;
  camera.updateProjectionMatrix();

  renderer.setSize(width, height);
  composer.setSize(width, height);

  // Update pass-specific resolutions
  if (fxaaPass) {
    fxaaPass.material.uniforms["resolution"].value.set(
      1 / (width * pixelRatio),
      1 / (height * pixelRatio),
    );
  }

  if (bloomPass) {
    bloomPass.resolution.set(width, height);
  }
}

window.addEventListener("resize", onWindowResize);
```

## Multi-Pass Rendering

Render each required scene/layer into owned off-screen targets, then combine
them through one explicit shader/pass. Choose additive light, alpha-over, or
another product-appropriate operation; account for premultiplied alpha,
occlusion/depth, and linear color.

Sequential composers rendering to the screen do not automatically compose:
clears and opaque full-screen passes can replace previous pixels. Treat
transparent overlay passes as an intentional pipeline with tested clear/depth
behavior, not a universal autoClear=false recipe.

## WebGPU node pipeline

WebGPU uses a version-specific node/TSL pipeline rather than WebGL
EffectComposer. Resolve the installed renderer, node helpers, and effect
exports from its documentation/source; do not import an old Nodes.js bundle or
assume one release threshold defines compatibility.

The conceptual path is scene pass → effect nodes → output node → render.
Preserve one frame owner, explicit output conversion, resize, and resource
cleanup. Do not mix a WebGL pass instance into that graph.
