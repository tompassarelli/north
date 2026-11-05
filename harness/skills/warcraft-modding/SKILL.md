---
name: warcraft-modding
description: >-
  Warcraft III map work: Wisp, Mak'gora ts/, LAN or Battle.net clients, captures, playtests, desyncs, TypeScriptToLua frame cost or GC, Lua32, WurstScript.
grounded: 2026-10-15
written: 2026-11-04
metadata:
  kind: domain
---

# Warcraft modding

## Every task

- Run `bun wisp ...` from the project's TypeScript directory. Find Wisp capabilities through wisp:docs/index.md (`node_modules/wisp/docs/index.md`).
- Follow the noun-first command vocabulary in wisp:docs/cli.md. Rename a noun together with all its callers and leave no alias. Register each new noun in the vocabulary and the feature index.
- Mak'gora: read makgora:AGENTS.md and makgora:docs/typescript.md before changing its pinned Wisp consumer in `ts/`. Find its commands through help and the docs index.
- `game-ip` decides where game and engine facts may come from and what may run on a client; load it before measuring native behaviour or when a task seems to need the game's internals.
- Use `makgora` for fighter animation, characters and stage art, and `effect-development` for Bun host tools that start processes, wait, retry, hold resources or parse outside data. Keep map code in plain TypeScript with no Effect imports.
- Never touch Tom's game install, except to bring it to the playtest lobby or to put builds he will test, each under a distinct in-map name, in his `Maps/00-Makgora` ([playtest-for-tom.md](references/playtest-for-tom.md)); south:agents/skills/battlenet-accounts/ says which clone shares his account, when it may run and when to sign it out.
- Close Tom's Battle.net or Warcraft III, once he approves, only with `north warcraft-close` and never by killing single processes, because a partial kill leaves Steam's launch half-alive and his next Play does nothing.

## Read when

- [map-code.md](references/map-code.md) before writing or changing Wisp map TypeScript, or building a map: hot reload, binary32 numerics, compiler limits, builds and tests.
- [native-clients.md](references/native-clients.md) before starting, leasing or driving a native client, taking a native capture, proving a parity, motion, visual or timed-effect fix, landing a `Verify: native` box, or chasing a desync.
- [playtest-for-tom.md](references/playtest-for-tom.md) before building for Tom, opening his game window, telling him what a build contains or calling a playtest ready.
- [lua-performance.md](references/lua-performance.md) for over-ceiling Lua32 tests, a failing `bun wisp perf compare`, frame cost, per-frame allocation or GC pressure.
- [wurst.md](references/wurst.md) for any WurstScript project, compiler, test, object-data or UI work.
- [warcraft-startup.md](references/warcraft-startup.md) when `wisp client doctor` stops and manual recovery is needed.
- south:agents/skills/battlenet-accounts/references/accounts-and-login.md for test sign-in or account questions.
- [pad-and-input-capture.md](references/pad-and-input-capture.md) for pad batches, pose captures or press-to-screen timing.
- [lan-discovery.md](references/lan-discovery.md) for pool caps or lobby discovery CPU costs.
- [renderer-features.md](references/renderer-features.md) for graphics natives.
- [bot-perception.md](references/bot-perception.md) for CPU observation, history or checksum changes.
- [effect-in-wisp.md](references/effect-in-wisp.md) for Effect integration in Wisp host tools.
- [failure-modes.md](references/failure-modes.md) for unfamiliar build or session symptoms.
- [hive-workshop.md](references/hive-workshop.md) to publish a map release on Hive Workshop: upload, page text and thread post.
