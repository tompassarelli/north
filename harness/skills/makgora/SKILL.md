---
name: makgora
description: >-
  Mak'gora fighters, moves, animation and stages: design, build, test, judge. Not other Warcraft maps.
grounded: 2026-11-10
written: 2026-11-10
metadata:
  kind: domain
---

# Mak'gora

## Which reference

- Fighter animation, poses, damage reactions or the animation scorecard: read [animation](references/animation.md).
- A new fighter, a move change, frame data, hit regions, rollback state or selection: read [character creation](references/character-creation.md).
- Stage art, composition, water, sky, light or a new stage: read [stage design](references/stage-design.md).

## Shared rules

- Before editing makgora:ts/, read makgora:AGENTS.md and makgora:docs/typescript.md: they pin the Wisp consumer and its commands. Wisp workflow, builds and native clients: `warcraft-modding`.
- Melee feel comes only from public frame data and from watching the game, under `clean-room` and its [Nintendo reference](../clean-room/references/nintendo.md). Decomp names, addresses or code, values read from Melee's files, and Nintendo animation or art are not ours to use.
- Shipped motion and art are original and authored, or from licensed inputs. Proprietary models, private art, references and recordings stay out of Git, in the private stores `~/.local/share/makgora-build-inputs/` and `~/.local/share/makgora-animation-reference/`, under `clean-room` and `repo-safety`.
- Publish art through the Wisp input store as `warcraft-modding` says, and commit the result. Never edit generated modules or live input families in place: inputs are hashed and generated files are rebuilt.
- Judge every visual change at gameplay side-view zoom in both facings: a pose or scene that reads in one view can fail in the other. Mak'gora uses the Classic models only; never add Definitive Edition work (makgora:docs/design/roster.md).
- Show visual results in the Wisp native window, because headless frames miss native rendering. A visual box lands before its capture and closes on it; only a release candidate waits for captures (makgora:AGENTS.md, line 13).
- Take every look, animation and VFX capture in the Wisp native window, never in Warcraft III clients. From makgora:ts/ on the GPU host (`south:agents/skills/battlenet-accounts/` names it): `bun scripts/windowStrip.ts PAD.pad --out ~/.local/state/makgora/shots/NAME/NAME-low.jpg --row N,N,... [--row ...]`, which runs under `north capacity host run --class gpu` on a private desktop with no Warcraft pair. Options and the remote route: makgora:docs/commands/native.md, "Look captures". Warcraft clients only for what only Warcraft shows: online desync and net checks, and the Classic acceptance pass before a release.
- Leave Forge fighters out of balance work, per-fighter quality gates and single-player content until promoted (makgora:docs/gameplay-design.md, "The Forge").
- Land with `safe-push --to main`. When a landing turns main red, the lander reverts the first red lane (makgora:docs/ci.md, "Landing"), because main is the only release line.
