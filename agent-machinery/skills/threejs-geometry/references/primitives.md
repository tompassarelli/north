# Built-in shapes and render primitives

## Decisions and rationale

Choose topology from the intended surface, line, or point semantics. Segment counts determine both silhouette quality and cost. Wireframe triangles and visible shape edges answer different visual questions. WebGL line width is often effectively one pixel; thick lines require an appropriate supported rendering path.

## Reading the examples

Each block is an independent illustrative fragment, not a tested complete app.
Unless declared inside the block, scene/camera/renderer/mesh and asset variables
come from the consumer. Match APIs to the installed Three.js release. Wire only
the selected pattern into the consumer's frame and cleanup ownership.

## Basic Example

```javascript
import * as THREE from "three";

// Built-in geometry
const box = new THREE.BoxGeometry(1, 1, 1);
const sphere = new THREE.SphereGeometry(0.5, 32, 32);
const plane = new THREE.PlaneGeometry(10, 10);

// Create mesh
const material = new THREE.MeshStandardMaterial({ color: 0x00ff00 });
const mesh = new THREE.Mesh(box, material);
scene.add(mesh);
```

## Built-in Geometries

### Basic Shapes

```javascript
// Box - width, height, depth, widthSegments, heightSegments, depthSegments
new THREE.BoxGeometry(1, 1, 1, 1, 1, 1);

// Sphere - radius, widthSegments, heightSegments, phiStart, phiLength, thetaStart, thetaLength
new THREE.SphereGeometry(1, 32, 32);
new THREE.SphereGeometry(1, 32, 32, 0, Math.PI * 2, 0, Math.PI); // Full sphere
new THREE.SphereGeometry(1, 32, 32, 0, Math.PI); // Hemisphere

// Plane - width, height, widthSegments, heightSegments
new THREE.PlaneGeometry(10, 10, 1, 1);

// Circle - radius, segments, thetaStart, thetaLength
new THREE.CircleGeometry(1, 32);
new THREE.CircleGeometry(1, 32, 0, Math.PI); // Semicircle

// Cylinder - radiusTop, radiusBottom, height, radialSegments, heightSegments, openEnded
new THREE.CylinderGeometry(1, 1, 2, 32, 1, false);
new THREE.CylinderGeometry(0, 1, 2, 32); // Cone
new THREE.CylinderGeometry(1, 1, 2, 6); // Hexagonal prism

// Cone - radius, height, radialSegments, heightSegments, openEnded
new THREE.ConeGeometry(1, 2, 32, 1, false);

// Torus - radius, tube, radialSegments, tubularSegments, arc
new THREE.TorusGeometry(1, 0.4, 16, 100);

// TorusKnot - radius, tube, tubularSegments, radialSegments, p, q
new THREE.TorusKnotGeometry(1, 0.4, 100, 16, 2, 3);

// Ring - innerRadius, outerRadius, thetaSegments, phiSegments
new THREE.RingGeometry(0.5, 1, 32, 1);
```

### Advanced Shapes

```javascript
// Capsule - radius, length, capSegments, radialSegments
new THREE.CapsuleGeometry(0.5, 1, 4, 8);

// Dodecahedron - radius, detail
new THREE.DodecahedronGeometry(1, 0);

// Icosahedron - radius, detail (0 = 20 faces, higher = smoother)
new THREE.IcosahedronGeometry(1, 0);

// Octahedron - radius, detail
new THREE.OctahedronGeometry(1, 0);

// Tetrahedron - radius, detail
new THREE.TetrahedronGeometry(1, 0);

// Polyhedron - vertices, indices, radius, detail
const vertices = [1, 1, 1, -1, -1, 1, -1, 1, -1, 1, -1, -1];
const indices = [2, 1, 0, 0, 3, 2, 1, 3, 0, 2, 3, 1];
new THREE.PolyhedronGeometry(vertices, indices, 1, 0);
```

### Path-Based Shapes

```javascript
// Lathe - points[], segments, phiStart, phiLength
const points = [
  new THREE.Vector2(0, 0),
  new THREE.Vector2(0.5, 0),
  new THREE.Vector2(0.5, 1),
  new THREE.Vector2(0, 1),
];
new THREE.LatheGeometry(points, 32);

// Extrude - shape, options
const shape = new THREE.Shape();
shape.moveTo(0, 0);
shape.lineTo(1, 0);
shape.lineTo(1, 1);
shape.lineTo(0, 1);
shape.lineTo(0, 0);

const extrudeSettings = {
  steps: 2,
  depth: 1,
  bevelEnabled: true,
  bevelThickness: 0.1,
  bevelSize: 0.1,
  bevelSegments: 3,
};
new THREE.ExtrudeGeometry(shape, extrudeSettings);

// Tube - path, tubularSegments, radius, radialSegments, closed
const curve = new THREE.CatmullRomCurve3([
  new THREE.Vector3(-1, 0, 0),
  new THREE.Vector3(0, 1, 0),
  new THREE.Vector3(1, 0, 0),
]);
new THREE.TubeGeometry(curve, 64, 0.2, 8, false);
```

### Text Geometry

```javascript
import { FontLoader } from "three/addons/loaders/FontLoader.js";
import { TextGeometry } from "three/addons/geometries/TextGeometry.js";

const loader = new FontLoader();
loader.load("fonts/helvetiker_regular.typeface.json", (font) => {
  const geometry = new TextGeometry("Hello", {
    font: font,
    size: 1,
    depth: 0.2, // Was 'height' in older versions
    curveSegments: 12,
    bevelEnabled: true,
    bevelThickness: 0.03,
    bevelSize: 0.02,
    bevelSegments: 5,
  });

  // Center text
  geometry.computeBoundingBox();
  geometry.center();

  const mesh = new THREE.Mesh(geometry, material);
  scene.add(mesh);
});
```

## EdgesGeometry & WireframeGeometry

```javascript
// Edge lines (only hard edges)
const edges = new THREE.EdgesGeometry(boxGeometry, 15); // 15 = threshold angle
const edgeMesh = new THREE.LineSegments(
  edges,
  new THREE.LineBasicMaterial({ color: 0xffffff }),
);

// Wireframe (all triangles)
const wireframe = new THREE.WireframeGeometry(boxGeometry);
const wireMesh = new THREE.LineSegments(
  wireframe,
  new THREE.LineBasicMaterial({ color: 0xffffff }),
);
```

## Points

```javascript
// Create point cloud
const geometry = new THREE.BufferGeometry();
const positions = new Float32Array(1000 * 3);

for (let i = 0; i < 1000; i++) {
  positions[i * 3] = (Math.random() - 0.5) * 10;
  positions[i * 3 + 1] = (Math.random() - 0.5) * 10;
  positions[i * 3 + 2] = (Math.random() - 0.5) * 10;
}

geometry.setAttribute("position", new THREE.BufferAttribute(positions, 3));

const material = new THREE.PointsMaterial({
  size: 0.1,
  sizeAttenuation: true, // Size decreases with distance
  color: 0xffffff,
});

const points = new THREE.Points(geometry, material);
scene.add(points);
```

## Lines

```javascript
// Line (connected points)
const points = [
  new THREE.Vector3(-1, 0, 0),
  new THREE.Vector3(0, 1, 0),
  new THREE.Vector3(1, 0, 0),
];
const geometry = new THREE.BufferGeometry().setFromPoints(points);
const line = new THREE.Line(
  geometry,
  new THREE.LineBasicMaterial({ color: 0xff0000 }),
);

// LineLoop (closed loop)
const loop = new THREE.LineLoop(geometry, material);

// LineSegments (pairs of points)
const segmentsGeometry = new THREE.BufferGeometry();
segmentsGeometry.setAttribute(
  "position",
  new THREE.BufferAttribute(
    new Float32Array([
      -1,
      0,
      0,
      0,
      1,
      0, // segment 1
      0,
      1,
      0,
      1,
      0,
      0, // segment 2
    ]),
    3,
  ),
);
const segments = new THREE.LineSegments(segmentsGeometry, material);
```
