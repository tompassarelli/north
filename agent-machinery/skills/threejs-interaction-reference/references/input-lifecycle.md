# Input lifecycle and cost

## Decisions and rationale

Retain listener ownership, clear pressed keys on blur, and avoid claiming shortcuts while a text input owns focus. Pointer events can cover mouse, pen, and touch; a click example alone is not a drag recognizer. Coalesce expensive hover work without dropping committed input.

## Reading the examples

Each block is an independent illustrative fragment, not a tested complete app.
Unless declared inside the block, scene/camera/renderer/mesh and asset variables
come from the consumer. Match APIs to the installed Three.js release. Wire only
the selected pattern into the consumer's frame and cleanup ownership.

## Keyboard Input

```javascript
const keys = {};

document.addEventListener("keydown", (event) => {
  keys[event.code] = true;
});

document.addEventListener("keyup", (event) => {
  keys[event.code] = false;
});

function update() {
  const speed = 0.1;

  if (keys["KeyW"]) player.position.z -= speed;
  if (keys["KeyS"]) player.position.z += speed;
  if (keys["KeyA"]) player.position.x -= speed;
  if (keys["KeyD"]) player.position.x += speed;
  if (keys["Space"]) player.position.y += speed;
  if (keys["ShiftLeft"]) player.position.y -= speed;
}
```

## Event Handling Best Practices

A bounded click binding can own its listeners without a half-implemented event
manager. Add gesture recognition separately when clicks and drags must differ.

```javascript
function bindPicking(canvas, camera, targets, onPick) {
  const events = new AbortController();
  const pointer = new THREE.Vector2();
  const ray = new THREE.Raycaster();
  canvas.addEventListener("click", (event) => {
    const rect = canvas.getBoundingClientRect();
    if (!rect.width || !rect.height) return;
    pointer.set((event.clientX - rect.left) / rect.width * 2 - 1,
                1 - (event.clientY - rect.top) / rect.height * 2);
    ray.setFromCamera(pointer, camera);
    onPick(ray.intersectObjects(targets, true)[0] ?? null);
  }, { signal: events.signal });
  return () => events.abort();
}
```

## Performance Examples

Apply the distilled sibling's performance decisions before selecting one of these detailed patterns.

```javascript
// Use simpler geometry for raycasting
const complexMesh = loadedModel;
const collisionMesh = new THREE.Mesh(
  new THREE.BoxGeometry(1, 1, 1),
  new THREE.MeshBasicMaterial({ visible: false }),
);
collisionMesh.userData.target = complexMesh;
clickables.push(collisionMesh);
```
