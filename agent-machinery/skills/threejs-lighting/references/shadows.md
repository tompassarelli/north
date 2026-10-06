# Shadow setup and diagnosis

## Decisions and rationale

A shadow is a second view of the scene. Fit that view before increasing resolution; bias trades acne against detached contact. Point-light shadows require several faces, so one extra caster can cost much more than one extra direct light. Layers control camera visibility, not general per-object light inclusion masks.

## Reading the examples

Each block is an independent illustrative fragment, not a tested complete app.
Unless declared inside the block, scene/camera/renderer/mesh and asset variables
come from the consumer. Match APIs to the installed Three.js release. Wire only
the selected pattern into the consumer's frame and cleanup ownership.

## Shadow Setup

### Enable Shadows

```javascript
// 1. Enable on renderer
renderer.shadowMap.enabled = true;
renderer.shadowMap.type = THREE.PCFSoftShadowMap;

// Shadow map types:
// THREE.BasicShadowMap - fastest, low quality
// THREE.PCFShadowMap - default, filtered
// THREE.PCFSoftShadowMap - softer edges
// THREE.VSMShadowMap - variance shadow map

// 2. Enable on light
light.castShadow = true;

// 3. Enable on objects
mesh.castShadow = true;
mesh.receiveShadow = true;

// Ground plane
floor.receiveShadow = true;
floor.castShadow = false; // Usually false for floors
```

### Optimizing Shadows

```javascript
// Tight shadow camera frustum
const d = 10;
dirLight.shadow.camera.left = -d;
dirLight.shadow.camera.right = d;
dirLight.shadow.camera.top = d;
dirLight.shadow.camera.bottom = -d;
dirLight.shadow.camera.near = 0.5;
dirLight.shadow.camera.far = 30;

// Fix shadow acne
dirLight.shadow.bias = -0.0001; // Depth bias
dirLight.shadow.normalBias = 0.02; // Bias along normal

// Shadow map size (balance quality vs performance)
// 512 - low quality
// 1024 - medium quality
// 2048 - high quality
// 4096 - very high quality (expensive)
```

### Contact Shadows (Fake, Fast)

Contact shadows are an approximation, not a core Three.js ContactShadows
addon. A baked plane/texture or a deliberately implemented depth-and-blur
pass can suit static contact. Framework packages may supply their own
component; do not import a nonexistent Three.js object or silently add one.

## Light Helpers

```javascript
import { RectAreaLightHelper } from "three/addons/helpers/RectAreaLightHelper.js";

// DirectionalLight helper
const dirHelper = new THREE.DirectionalLightHelper(dirLight, 5);
scene.add(dirHelper);

// PointLight helper
const pointHelper = new THREE.PointLightHelper(pointLight, 1);
scene.add(pointHelper);

// SpotLight helper
const spotHelper = new THREE.SpotLightHelper(spotLight);
scene.add(spotHelper);

// Hemisphere helper
const hemiHelper = new THREE.HemisphereLightHelper(hemiLight, 5);
scene.add(hemiHelper);

// RectAreaLight helper
const rectHelper = new RectAreaLightHelper(rectLight);
rectLight.add(rectHelper);

// Update helpers when light changes
dirHelper.update();
spotHelper.update();
```

## Performance Examples

Layers filter visibility against a camera. They do not generally select
which objects a light illuminates within one WebGL scene render. Use a
deliberate separate-render/material design if selective illumination is required.

```javascript
mesh.castShadow = true;
mesh.receiveShadow = true;
decorMesh.castShadow = false;
```
