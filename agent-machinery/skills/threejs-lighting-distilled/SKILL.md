---
name: threejs-lighting-distilled
description: Configure Three.js direct/environment lighting, shadows, and lighting performance.
---

# Three.js lighting

Match lights to the material: unlit materials ignore them; PBR needs useful
direct or environment illumination. Establish environment/fill, then the
smallest intentional set of key lights.

- Ambient/hemisphere provide fill; directional models distant sources;
  point/spot model local sources; area lights give broad highlights.
- Shadows need renderer support, a supported light, casting/receiving objects,
  and a fitted shadow camera. Start with one useful caster.
- Fit shadow volume before increasing map resolution. Bound lights, maps,
  ranges, and update frequency to the measured frame budget.
- Attach and update directional/spot targets. Use helpers to fit them, then
  remove helpers from shipped scenes.
- Adjust exposure, material response, and illumination together in representative
  views. Bake static lighting only when the content workflow permits.
- Release owned environment-processing and shadow resources at teardown.

Full notes: `agents path threejs-lighting-reference` for light selection,
shadow diagnosis, and environment/staging examples.
