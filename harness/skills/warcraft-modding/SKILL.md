---
name: warcraft-modding
description: >-
  Use for any Warcraft III map work: Wisp TypeScript maps (hot reload, headless
  and 32-bit Lua tests), the native game (LAN and signed-in clients, startup
  recovery, desync debugging), and Smashcraft's ts/. Also use to cut frame cost,
  Lua instruction counts or per-frame allocation in TypeScriptToLua code on PUC
  Lua 5.3 (over-ceiling Lua32 tests, failing `bun wisp perf compare`, GC
  pressure), and to develop maps and tooling in WurstScript (Wurst projects,
  compiler workflows, tests, compile-time object data, WC3 UI).
grounded: 2026-10-15
written: 2026-10-15
metadata:
  kind: domain
---

# Warcraft modding

## Source and runtime

- Run `bun wisp ...` from the project's TypeScript directory. Find Wisp capabilities through wisp:docs/index.md (`node_modules/wisp/docs/index.md`).
- Follow the noun-first command vocabulary in wisp:docs/cli.md. Rename a noun together with all its callers and leave no alias. Register each new noun in the vocabulary and the feature index.
- Smashcraft: read smashcraft:AGENTS.md and smashcraft:docs/typescript.md before changing its pinned Wisp consumer in `ts/`. Find its commands through help and the docs index.
- Follow `game-ip` for where game facts may come from: never memory reads, debuggers, injection or decomp.
- Read [wurst.md](references/wurst.md) when the source or toolchain is Wurst and [lua-performance.md](references/lua-performance.md) for frame cost, allocation and Lua32 ceilings, because each holds measured or toolchain-specific rules. Use `smashcraft` for fighter animation, characters and stage art, and `effect-development` for Bun host tools that start processes, wait, retry, hold resources or parse outside data. Keep map code in plain TypeScript with no Effect imports.

## Fast iteration

- Keep `bun wisp dev` running: it reports type errors and runs affected tests and two-client headless journeys. Run affected tests locally. Send full Bun and 32-bit Lua suites, and heavy headless sweeps that need no private inputs or clients, to `bun wisp farm test --wait` for HEAD. [fast-native-iteration.md](references/fast-native-iteration.md) has the flags, measured costs and warm procedures.
- Prefer hot reload to a script-only rebuild, and a script-only rebuild to a full build. Build fully only for assets, object data or non-TypeScript sources.
- Reload safety: bind each engine callback once through `trampoline(name)`. Register every handler through `on(name, handler)` in the entry's `install()`, which runs at startup and after each reload; do no work at module scope.
- Keep state that must survive a reload in declared globals, not module locals. When a global's shape changes, convert it or start a fresh match. Preserve map files, object data, assets and handles across a reload.
- Write each client's payload before its manifest in `CustomMapData/<prefix>-hot`. All clients verify and install on the same frame, or all refuse.

## Engine constraints

- Numerics: model wrapping 32-bit integers and binary32 arithmetic, including `+` and `*` that do not round to nearest and division up to an ulp off. Use `floorDiv`/`floorMod` or the Wurst-semantic `idiv`/`imod`.
- Wrap each synchronized real operation separately in `f32()`. Write decimal literals through `f32()`, in tests too, and compile noninteger literals to exact hex floats.
- Avoid what the compiler rejects: `%`, `>>>`, `Math.floor(a / b)`, decimal literals that are not binary32, randomness, Date, JSON, Intl and host or DOM APIs. Outside tests, avoid `any`, `as unknown as` and the non-null `!`; use the project's `at()`.
- Give new content a new Preloader path: a path's first content stays cached for the whole session, whatever existence checks report.
- Copy optional fields one at a time, because Lua's `Object.assign` skips undefined fields. Create and free timers, triggers and callbacks only on synchronized turns, or checksums diverge.
- Errors: rely on recorded TypeScript throw sites, not on a Lua debug library, and enable the stack-traces plugin for full stacks. [api-gotchas.md](references/api-gotchas.md) covers patch-specific natives, assets, input, sync, FileIO and Warsmash investigation.
- Warsmash is AGPL-3.0. Confirm its behavior against the real game before changing Wisp, and record that behavior in wisp:docs/warsmash-notes.md; never copy or adapt its code into MIT Wisp.
- Build plumbing: declare typed UI frames through wisp:docs/ui.md. Measure existing compressed imports before re-encoding them (wisp:docs/asset-ingestion.md). Use wisp:docs/ci.md's reusable workflow, and keep private-runner .w3x builds out of public uploads.
- Tests: register `test()` in `*.tests.ts` for Bun and 32-bit Lua under the `verification` skill; it owns retention and bug-fix coverage.

## Native clients and checks

- Parity: a subsystem's headless check plus the weekly native spot batch meets its native box only when corpus divergence is zero. Keep the automatically recorded covered sessions in smashcraft:ts/test/corpus/, where `bun wisp parity corpus` checks them in Bun and 32-bit Lua on every push. Fix the first divergent frame or field before substituting headless coverage for a native check.
- Measure native truth only from outside the game: pictures, sound, War3Log and other files the game writes, replays, timing, and map-side Lua instrumentation inside our own map (frame number, checksums, per-frame cost) with stack-trace builds. Align native and Wisp frames by the map's own frame number. A gameplay check passes when native pad output matches headless through `bun wisp pad ... --compare`.
- Clients: Classic runs on the offline LAN pool at build 3.0.0.24268. Battle.net tests run on the live build, on the signed-in clones listed in south:agents/skills/battlenet-accounts/; use whichever hosts soonest. The pool keeps its own pinned source, never a signed-in clone.
- Check the live build with `curl http://us.patch.battle.net:1119/w3/versions` before a Battle.net-side run. The pool stays on 3.0.0.24268, where the LAN plugin is checked; move it only to a build the plugin has been checked on. Use integrity maps, not default dev maps, for pad chat setup. [lan-pair-batching.md](references/lan-pair-batching.md) covers pool setup, capacity, pair ownership and issue queues.
- Native work runs off Tom's machine, on the remote Warcraft host south:agents/skills/battlenet-accounts/ names, through that host's skill's path and pool recipe. An account signed in there is signed in nowhere else.
- Close Tom's Battle.net or Warcraft III, once he approves, only with `~/code/north/main/bin/tom-warcraft-close` and never by killing single processes, because a partial kill leaves Steam's launch half-alive and his next Play does nothing.
- Never touch Tom's game install, except to bring it to the playtest lobby below; south:agents/skills/battlenet-accounts/ says which clone shares his account, when it may run and when to sign it out.
- Prove a flicker, jitter or other motion fix with at least 10 s of native video at 60 fps or more, counting bad frames by frame diff before and after, with the mouse and with the pad; a screenshot never proves one.
- A playtest is ready for Tom only when his own Steam, Battle.net and Warcraft III are open and waiting in the custom-game lobby with the RC map selected, Classic graphics and the pad connected; never start the match. Write RC-READY, and relay a playtest to Tom, only after that lobby is confirmed (LOBBY-READY with a screenshot).
- Lifecycle: start signed-in clients as user services with `bun wisp client start [CLIENT...] --clients-file FILE`, never as shell or background tasks. Manage them with `client status` and `client stop`, and leave them at the menu for the next Battle.net check.
- Give each signed-in client its own worker for one-client captures; put a client pair under one worker only for sync, netplay and EX checks.
- Run a build's native spot checks as one batch sharded across every free client, judge once and tick every box it covers; keep tooling reference captures off native lanes.
- Never hold a landing for a native capture: a `Verify: native` box (brief line `Lands-before: native`) lands as soon as its sim, headless and Lua32 checks pass, then takes its native capture on main's build and fixes forward from what the capture shows; only a release candidate waits on native captures, and a `Category: native-check` box runs captures and has nothing to land (2026-10-30).
- Build a Smashcraft map once per commit and inputs: `bun wisp map build` copies a finished build of the same checkout state, profile, edition and arguments from `~/.cache/wisp/maps` and logs `reused map build HASH` (wisp:docs/compile-cache.md#map-reuse); `WISP_MAP_REUSE=off` forces a fresh build, and a before-measurement of origin/main is that reused build.
- Take a client pair from the shared queue, not by naming one: `bun wisp lan lease take --owner NAME` prints the next free `{host,pair}` (`--hosts any` adds the laptop while `north laptop status` says available), `lan lease release --host H --pair K` frees it, and `anim capture serve` leases one when started without `--pair` (wisp:docs/lan.md#leasing-a-pair).
- Never attach a debugger, trace, read or write game memory, inject code, or decrypt or dump the game on any client. Smashcraft Servers (north:harness/skills/game-ip) is the one exception. The private LAN switch and desync debugger under ~/.local/share/wisp-private/ only diagnose our own test clients; their output never decides Wisp or Smashcraft behaviour and their details never enter a repository, issue or doc.
- Desync: find the first divergent frame from map-side checksums, replays and War3Log. [native-sessions.md](references/native-sessions.md) covers manual diagnosis. Rerun a native session that a crash, a new desync report or a premature result made invalid.

## Play and recovery

- Build current main for Tom with `wisp play`, and route experiments through fresh, captures or accept. Confirm the roster, stage thumbnails and gameplay before calling a build ready.
- Verify a visual fix yourself with the cheapest evidence that settles it: look at a few native captures or frames with your own vision, not a purpose-built detector, and tell Tom the auto-installed build's file name once it reaches his `Maps/00-Smashcraft`, because Tom supervises five projects and is not the visual tester.
- Build through `bun wisp map build` with hashed private inputs in smashcraft:build-inputs.json. Add art with `bun wisp inputs add FAMILY DIR` and commit the result; never edit stored inputs in place.
- Recover clients with `wisp client doctor`, not by hand-driving known failure states. Check `client watch --once` or `client wait CLIENT STATE` before clicking, reading or waiting on a client. Set up through in-map commands, and observe through menu sockets, status files and War3Log, not pointer clicks.
- Host through the menu socket after the post-login ladder-map scan, so that `-loadfile` keeps imported assets.
- Search Hive Workshop first for a Warcraft bug, and change one variable per native experiment under `debugging`. Use off-monitor private desktops through `private-desktop-development`, and keep native tests on them.
- Keep one runtime per prefix, and launch Warcraft through signed-in Battle.net Play, not Warcraft III.exe. Fix an empty Options/Exit Game shell through launcher Play, not another sign-in. Count a launch as successful only once the real menu and one gameplay action work.
- Never print decrypted credentials.

## Conditional references

- [warcraft-startup.md](references/warcraft-startup.md) when doctor stops and manual recovery is needed.
- south:agents/skills/battlenet-accounts/references/accounts-and-login.md for test sign-in or account questions.
- [pad-and-input-capture.md](references/pad-and-input-capture.md) for pad batches, pose captures or press-to-screen timing.
- [lan-discovery.md](references/lan-discovery.md) for pool caps or lobby discovery CPU costs.
- [renderer-features.md](references/renderer-features.md) for graphics natives.
- [bot-perception.md](references/bot-perception.md) for CPU observation, history or checksum changes.
- [effect-in-wisp.md](references/effect-in-wisp.md) for Effect integration.
- [failure-modes.md](references/failure-modes.md) for unfamiliar build or session symptoms.
- [hive-workshop.md](references/hive-workshop.md) to publish a map release on Hive Workshop: upload, page text and thread post.
- [wurst.md](references/wurst.md) for any WurstScript project, compiler, test, object-data or UI work.
- [lua-performance.md](references/lua-performance.md) for over-ceiling Lua32 tests, a failing `bun wisp perf compare`, per-frame allocation or GC pressure.
