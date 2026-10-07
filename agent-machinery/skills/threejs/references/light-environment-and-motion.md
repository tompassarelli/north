# Environment lighting, probes, and motion

## Decisions and rationale

A visible background and an illumination environment are independent choices. PMREM prepares filtered environment radiance for rough surfaces; it is not just an equirectangular-to-cube image converter. Keep generated render-target ownership so resources can be released after their final consumer.

## Environment Lighting (IBL)

Image-based lighting from an HDR environment map. Prefilter it with
PMREMGenerator for rough surfaces; loading code for cube, HDR, EXR and PMREM
is in `texture-sources-and-environments.md`.

```javascript
// Set as scene environment (affects all PBR materials)
scene.environment = envMap;

// Optional: also use as background
scene.background = envMap;
scene.backgroundBlurriness = 0; // 0-1, blur the background
scene.backgroundIntensity = 1;
```

## Light Probes (Advanced)

Capture lighting from a point in space for ambient lighting.

```javascript
import { LightProbeGenerator } from "three/addons/lights/LightProbeGenerator.js";

// Generate from cube texture
const lightProbe = new THREE.LightProbe();
scene.add(lightProbe);

lightProbe.copy(LightProbeGenerator.fromCubeTexture(cubeTexture));

// Or from render target
const cubeCamera = new THREE.CubeCamera(
  0.1,
  100,
  new THREE.WebGLCubeRenderTarget(256),
);
cubeCamera.update(renderer, scene);
lightProbe.copy(
  LightProbeGenerator.fromCubeRenderTarget(renderer, cubeCamera.renderTarget),
);
```

## Light Animation

```javascript
const clock = new THREE.Clock();

function animate() {
  const time = clock.getElapsedTime();

  // Orbit light around scene
  light.position.x = Math.cos(time) * 5;
  light.position.z = Math.sin(time) * 5;

  // Pulsing intensity
  light.intensity = 1 + Math.sin(time * 2) * 0.5;

  // Color cycling
  light.color.setHSL((time * 0.1) % 1, 1, 0.5);

  // Update helpers if using
  lightHelper.update();
}
```
