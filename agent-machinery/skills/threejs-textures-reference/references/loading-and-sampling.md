# Loading, color, and sampling

## Decisions and rationale

Color textures and data textures need different decoding semantics. Compressed file size does not predict GPU memory: dimensions, format, faces, and mip levels do. Mipmaps reduce minification aliasing; anisotropy helps oblique sampling at a cost. Do not apply legacy WebGL1 power-of-two restrictions to all current renderers.

## Reading the examples

Each block is an independent illustrative fragment, not a tested complete app.
Unless declared inside the block, scene/camera/renderer/mesh and asset variables
come from the consumer. Match APIs to the installed Three.js release. Wire only
the selected pattern into the consumer's frame and cleanup ownership.

## Basic Example

```javascript
import * as THREE from "three";

// Load texture
const loader = new THREE.TextureLoader();
const texture = loader.load("texture.jpg");

// Apply to material
const material = new THREE.MeshStandardMaterial({
  map: texture,
});
```

## Texture Loading

### Basic Loading

```javascript
const loader = new THREE.TextureLoader();

// Async with callbacks
loader.load(
  "texture.jpg",
  (texture) => console.log("Loaded"),
  (progress) => console.log("Progress"),
  (error) => console.error("Error"),
);

// Synchronous style (loads async internally)
const texture = loader.load("texture.jpg");
material.map = texture;
```

### Promise Wrapper

```javascript
function loadTexture(url) {
  return new Promise((resolve, reject) => {
    new THREE.TextureLoader().load(url, resolve, undefined, reject);
  });
}

// Usage
const [colorMap, normalMap, roughnessMap] = await Promise.all([
  loadTexture("color.jpg"),
  loadTexture("normal.jpg"),
  loadTexture("roughness.jpg"),
]);
```

## Texture Configuration

### Color Space

Critical for accurate color reproduction.

```javascript
// Color/albedo textures - use sRGB
colorTexture.colorSpace = THREE.SRGBColorSpace;

// Data textures (normal, roughness, metalness, AO) - leave as default
// Do NOT set colorSpace for data textures (NoColorSpace is default)
```

### Wrapping Modes

```javascript
texture.wrapS = THREE.RepeatWrapping; // Horizontal
texture.wrapT = THREE.RepeatWrapping; // Vertical

// Options:
// THREE.ClampToEdgeWrapping - Stretches edge pixels (default)
// THREE.RepeatWrapping - Tiles the texture
// THREE.MirroredRepeatWrapping - Tiles with mirror flip
```

### Repeat, Offset, Rotation

```javascript
// Tile texture 4x4
texture.repeat.set(4, 4);
texture.wrapS = THREE.RepeatWrapping;
texture.wrapT = THREE.RepeatWrapping;

// Offset (0-1 range)
texture.offset.set(0.5, 0.5);

// Rotation (radians, around center)
texture.rotation = Math.PI / 4;
texture.center.set(0.5, 0.5); // Rotation pivot
```

### Filtering

```javascript
// Minification (texture larger than screen pixels)
texture.minFilter = THREE.LinearMipmapLinearFilter; // Default, smooth
texture.minFilter = THREE.NearestFilter; // Pixelated
texture.minFilter = THREE.LinearFilter; // Smooth, no mipmaps

// Magnification (texture smaller than screen pixels)
texture.magFilter = THREE.LinearFilter; // Smooth (default)
texture.magFilter = THREE.NearestFilter; // Pixelated (retro games)

// Anisotropic filtering (sharper at angles)
texture.anisotropy = renderer.capabilities.getMaxAnisotropy();
```

### Generate Mipmaps

```javascript
// Usually true by default
texture.generateMipmaps = true;

// Disable only when the producer/data semantics or active backend requires it
texture.generateMipmaps = false;
texture.minFilter = THREE.LinearFilter;
```

## Performance Examples

Apply the distilled sibling's performance decisions before selecting one of these detailed patterns.

```javascript
// Count texture objects, not GPU bytes
console.log(renderer.info.memory.textures);

// Optimize for mobile
const maxSize = renderer.capabilities.maxTextureSize;
const isMobile = /iPhone|iPad|Android/i.test(navigator.userAgent);
const textureSize = isMobile ? 1024 : 2048;
```
