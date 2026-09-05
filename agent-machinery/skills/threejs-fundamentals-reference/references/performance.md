# Performance decisions

## Decisions and rationale

First distinguish CPU update cost, draw submissions, fill rate, and allocation. Instancing, merging, lower resolution, and reduced updates solve different bottlenecks. Renderer memory counters are object counts, not byte measurements; sample the real scene before selecting an optimization.

## Reading the examples

Each block is an independent illustrative fragment, not a tested complete app.
Unless declared inside the block, scene/camera/renderer/mesh and asset variables
come from the consumer. Match APIs to the installed Three.js release. Wire only
the selected pattern into the consumer's frame and cleanup ownership.

## Performance Examples

Apply the distilled sibling's performance decisions before selecting one of these detailed patterns.

```javascript
// Merge static geometries
import { mergeGeometries } from "three/addons/utils/BufferGeometryUtils.js";
const merged = mergeGeometries([geo1, geo2, geo3]);

// LOD
const lod = new THREE.LOD();
lod.addLevel(highDetailMesh, 0);
lod.addLevel(medDetailMesh, 50);
lod.addLevel(lowDetailMesh, 100);
scene.add(lod);
```
