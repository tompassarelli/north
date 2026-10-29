---
name: smashcraft
description: >-
  Smashcraft fighter and stage work. Use to author and improve fighter animation for quality and action coverage; to design, build and test a Smashcraft fighter or move for a new fighter or move change, not other Warcraft maps; and to design, dress and judge a stage's look with the Classic models, for any stage art, composition, look issue or new stage.
grounded: 2026-10-15
written: 2026-10-15
metadata:
  kind: domain
---

# Smashcraft

## Which reference

- Fighter animation, poses, damage reactions or the animation scorecard: read [animation](references/animation.md).
- A new fighter, a move change, frame data, hit regions, rollback state or selection: read [character creation](references/character-creation.md).
- Stage art, composition, water, sky, light or a new stage: read [stage design](references/stage-design.md).

## Shared rules

- Read smashcraft:AGENTS.md and smashcraft:docs/typescript.md before editing smashcraft:ts/, because they pin the Wisp consumer and its commands; use `warcraft-modding` for the Wisp workflow, builds and native clients.
- Take Melee feel only from public frame data (SmashWiki, Smashboards, public frame-data sites) and from watching the game, per `game-ip`, because decomp names, addresses or code, values read from Melee's files, and Nintendo animation or art are not ours to use.
- Use original authored motion and art from licensed inputs, because everything shipped must be ours or licensed.
- Keep proprietary models, private art, references and recordings outside Git in the private build-input and reference stores, under `repo-safety`, because they are private or licensed inputs.
- Publish art with `bun wisp inputs add FAMILY DIR` and commit the result, and never edit generated modules or live input families in place, because inputs are hashed and generated files are rebuilt.
- Judge every visual change at gameplay side-view zoom, in both facings, because a pose or scene that reads in one view can fail in another. Smashcraft uses the Classic models only; never add Definitive Edition work (smashcraft:docs/design/roster.md).
- Show the result in the real game before shipping, because headless frames miss native rendering; Tom sees stage looks at match zoom in his game before they ship.
- Land with `safe-push --to main` and fix forward when CI's Lua, parity or perf suites fail, because main is the only release line.
