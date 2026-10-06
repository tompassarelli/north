# UVs, atlases, and material maps

## Decisions and rationale

UV channels and texture transforms are shared state unless cloned. Atlas offsets select one tile for every consumer of that texture object; independent sprites need independent transforms or per-instance UV logic. Pad tiles for filter/mipmap bleed. Map channel conventions are versioned, not a universal uv2 requirement.

## Reading the examples

Each block is an independent illustrative fragment, not a tested complete app.
Unless declared inside the block, scene/camera/renderer/mesh and asset variables
come from the consumer. Match APIs to the installed Three.js release. Wire only
the selected pattern into the consumer's frame and cleanup ownership.

## UV Mapping

### Accessing UVs

```javascript
const uvs = geometry.attributes.uv;

// Read UV
const u = uvs.getX(vertexIndex);
const v = uvs.getY(vertexIndex);

// Modify UV
uvs.setXY(vertexIndex, newU, newV);
uvs.needsUpdate = true;
```

### Second UV Channel (for AO maps)

```javascript
// Modern versions: channel 1 selects the "uv1" attribute.
// Only create a second set when the asset needs one.
geometry.setAttribute("uv1", geometry.attributes.uv.clone());
aoTexture.channel = 1;

// Or create custom second UV
const secondUv = new Float32Array(vertexCount * 2);
// ... fill uv2 data
geometry.setAttribute("uv1", new THREE.BufferAttribute(secondUv, 2));
```

### UV Transform in Shader

```javascript
const material = new THREE.ShaderMaterial({
  uniforms: {
    map: { value: texture },
    uvOffset: { value: new THREE.Vector2(0, 0) },
    uvScale: { value: new THREE.Vector2(1, 1) },
  },
  vertexShader: `
    varying vec2 vUv;
    uniform vec2 uvOffset;
    uniform vec2 uvScale;

    void main() {
      vUv = uv * uvScale + uvOffset;
      gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
    }
  `,
  fragmentShader: `
    varying vec2 vUv;
    uniform sampler2D map;

    void main() {
      gl_FragColor = texture2D(map, vUv);
    }
  `,
});
```

## Texture Atlas

Multiple images in one texture.

```javascript
// Atlas with 4 sprites (2x2 grid)
const atlas = loader.load("atlas.png");
atlas.wrapS = THREE.ClampToEdgeWrapping;
atlas.wrapT = THREE.ClampToEdgeWrapping;

// Select sprite by UV offset/scale
function selectSprite(row, col, gridSize = 2) {
  atlas.offset.set(col / gridSize, 1 - (row + 1) / gridSize);
  atlas.repeat.set(1 / gridSize, 1 / gridSize);
}

// Select top-left sprite
selectSprite(0, 0);
```

## Material Texture Maps

### PBR Texture Set

```javascript
const material = new THREE.MeshStandardMaterial({
  // Base color (sRGB)
  map: colorTexture,

  // Surface detail (Linear)
  normalMap: normalTexture,
  normalScale: new THREE.Vector2(1, 1),

  // Roughness (Linear, grayscale)
  roughnessMap: roughnessTexture,
  roughness: 1, // Multiplier

  // Metalness (Linear, grayscale)
  metalnessMap: metalnessTexture,
  metalness: 1, // Multiplier

  // Ambient occlusion (numeric data; selected UV channel)
  aoMap: aoTexture,
  aoMapIntensity: 1,

  // Self-illumination (sRGB)
  emissiveMap: emissiveTexture,
  emissive: 0xffffff,
  emissiveIntensity: 1,

  // Vertex displacement (Linear)
  displacementMap: displacementTexture,
  displacementScale: 0.1,
  displacementBias: 0,

  // Alpha (Linear)
  alphaMap: alphaTexture,
  transparent: true,
});

// Match aoTexture.channel to the installed version and available geometry UVs.
```

### Normal Map Types

```javascript
// OpenGL style normals (default)
material.normalMapType = THREE.TangentSpaceNormalMap;

// Object space normals
material.normalMapType = THREE.ObjectSpaceNormalMap;
```
