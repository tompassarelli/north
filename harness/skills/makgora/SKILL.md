---
name: makgora
description: >-
  Mak'gora fighters, moves, animation and stages: design, build, test, judge. Not other Warcraft maps.
grounded: 2026-10-15
written: 2026-10-15
metadata:
  kind: domain
---

# Mak'gora

## Which reference

- Fighter animation, poses, damage reactions or the animation scorecard: read [animation](references/animation.md).
- A new fighter, a move change, frame data, hit regions, rollback state or selection: read [character creation](references/character-creation.md).
- Stage art, composition, water, sky, light or a new stage: read [stage design](references/stage-design.md).

## Shared rules

- Read makgora:AGENTS.md and makgora:docs/typescript.md before editing makgora:ts/, because they pin the Wisp consumer and its commands; use `warcraft-modding` for the Wisp workflow, builds and native clients.
- Take Melee feel only from public frame data (SmashWiki, Smashboards, public frame-data sites) and from watching the game, per `game-ip`, because decomp names, addresses or code, values read from Melee's files, and Nintendo animation or art are not ours to use.
- Use original authored motion and art from licensed inputs, because everything shipped must be ours or licensed.
- Keep proprietary models, private art, references and recordings outside Git in the private build-input and reference stores, under `repo-safety`, because they are private or licensed inputs.
- Publish art with `bun wisp inputs add FAMILY DIR` and commit the result, and never edit generated modules or live input families in place, because inputs are hashed and generated files are rebuilt.
- Judge every visual change at gameplay side-view zoom, in both facings, because a pose or scene that reads in one view can fail in another. Mak'gora uses the Classic models only; never add Definitive Edition work (makgora:docs/design/roster.md).
- Show the result in the Wisp native window before shipping, because headless frames miss native rendering; Tom sees stage looks at match zoom in his game before they ship.
- Take every look, animation and VFX capture for iteration and for Tom's review in the Wisp native window, never in Warcraft III clients: on the GPU host (south:agents/skills/battlenet-accounts/ names it), from makgora:ts/, run `bun scripts/windowStrip.ts PAD.pad --out ~/.local/state/makgora/shots/NAME/NAME-low.jpg --row N,N,... [--row ...]` (private desktop, `machine-capacity run --class gpu`, no pair; options in makgora:docs/commands/native.md, "Look captures"), because Tom plays in that window and a Lava Raze effects lane spent most of 12 hours queued for a Warcraft LAN pair (Tom asked why look captures used Warcraft, 2026-11-01). Use Warcraft clients only for what only Warcraft shows: online desync and net checks, and the Classic acceptance pass before Tom cuts a release.
- Leave Forge fighters out of balance work, per-fighter quality gates and single-player content until promoted, per makgora:docs/gameplay-design.md, "The Forge", because a kit Tom has not designed is not worth balancing.
- Land with `safe-push --to main` and fix forward when CI's Lua, parity or perf suites fail, because main is the only release line.
