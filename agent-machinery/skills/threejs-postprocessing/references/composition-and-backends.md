# Targets, composition, and backend differences

## Decisions and rationale

An intermediate target is an input, not a screen overlay. Combining scenes/effects requires an explicit compositing operation with alpha, depth, and color-space behavior. WebGPU's node pipeline is a different API from WebGL EffectComposer; a minimum release number does not make their examples interchangeable.

## Reading the examples

Each block is an independent illustrative fragment, not a tested complete app.
Unless declared inside the block, scene/camera/renderer/mesh and asset variables
come from the consumer. Match APIs to the installed Three.js release. Wire only
the selected pattern into the consumer's frame and cleanup ownership.

## Render to Texture

```javascript
// Create render target
const renderTarget = new THREE.WebGLRenderTarget(512, 512);

// Render scene to target
renderer.setRenderTarget(renderTarget);
renderer.render(scene, camera);
renderer.setRenderTarget(null);

// Use texture
const texture = renderTarget.texture;
otherMaterial.map = texture;
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
