# Native clients and checks

Read before starting, leasing or driving a native client, taking a native capture, proving a parity, motion, visual or timed-effect fix, landing a `Verify: native` box, or chasing a desync. `game-ip` owns where native facts may come from and what may run on a client.

## Clients and hosts

- Clients: Classic runs on the offline LAN pool at build 3.0.0.24268. Battle.net tests run on the live build, on the signed-in clones listed in south:agents/skills/battlenet-accounts/; use whichever hosts soonest. The pool keeps its own pinned source, never a signed-in clone.
- Check the live build with `curl http://us.patch.battle.net:1119/w3/versions` before a Battle.net-side run. The pool stays on 3.0.0.24268, where the LAN plugin is checked; move it only to a build the plugin has been checked on. Use integrity maps, not default dev maps, for pad chat setup. [lan-pair-batching.md](lan-pair-batching.md) covers pool setup, capacity, pair ownership and issue queues.
- Native work runs off Tom's machine, on the remote Warcraft host south:agents/skills/battlenet-accounts/ names, through that host's skill's path and pool recipe. An account signed in there is signed in nowhere else.
- Take a client pair from the shared queue, not by naming one: `bun wisp lan lease take --owner NAME` prints the next free `{host,pair}` (`--hosts any` adds the laptop while `north laptop status` says available), `lan lease release --host H --pair K` frees it, and `anim capture serve` leases one when started without `--pair` (wisp:docs/lan.md#leasing-a-pair).
- Lifecycle: start signed-in clients as user services with `bun wisp client start [CLIENT...] --clients-file FILE`, never as shell or background tasks. Manage them with `client status` and `client stop`, and leave them at the menu for the next Battle.net check.
- Give each signed-in client its own worker for one-client captures; put a client pair under one worker only for sync, netplay and EX checks.

## Driving and recovery

- Recover clients with `wisp client doctor`, not by hand-driving known failure states. Check `client watch --once` or `client wait CLIENT STATE` before clicking, reading or waiting on a client. Set up through in-map commands, and observe through menu sockets, status files and War3Log, not pointer clicks.
- Host through the menu socket after the post-login ladder-map scan, so that `-loadfile` keeps imported assets.
- Search Hive Workshop first for a Warcraft bug, and change one variable per native experiment under `debugging`. Use off-monitor private desktops through `private-desktop-development`, and keep native tests on them.
- Keep one runtime per prefix, and launch Warcraft through signed-in Battle.net Play, not Warcraft III.exe. Fix an empty Options/Exit Game shell through launcher Play, not another sign-in. Count a launch as successful only once the real menu and one gameplay action work.

## Captures and proof

- Parity: a subsystem's headless check plus the weekly native spot batch meets its native box only when corpus divergence is zero. Keep the automatically recorded covered sessions in makgora:ts/test/corpus/, where `bun wisp parity corpus` checks them in Bun and 32-bit Lua on every push. Fix the first divergent frame or field before substituting headless coverage for a native check.
- Use stack-trace builds for map-side instrumentation. Align native and Wisp frames by the map's own frame number. A gameplay check passes when native pad output matches headless through `bun wisp pad ... --compare`.
- Verify a visual fix yourself with the cheapest evidence that settles it: look at a few native captures or frames with your own vision, not a purpose-built detector, because Tom supervises five projects and is not the visual tester.
- Prove a flicker, jitter or other motion fix with at least 10 s of native video at 60 fps or more, counting bad frames by frame diff before and after, with the mouse and with the pad; a screenshot never proves one.
- Judge a timed effect (a Birth or decay a model plays out over real time) from real-time 60 fps video, Mak'gora's `bun wisp native-capture realtime`, never from held-frame captures: a held frame pauses the game while the effect keeps playing, which misjudged Archimonde's DoomDeath and several fighter effects (2026-11-04; Mak'gora commits e1c87a1efc, the verdict, and ea98840864, the command).
- Run a build's native spot checks as one batch sharded across every free client, judge once and tick every box it covers; keep tooling reference captures off native lanes.
- Never hold a landing for a native capture: a `Verify: native` box (brief line `Lands-before: native`) lands as soon as its sim, headless and Lua32 checks pass, then takes its native capture on main's build and fixes forward from what the capture shows; only a release candidate waits on native captures, and a `Category: native-check` box runs captures and has nothing to land (2026-10-30).
- Desync: find the first divergent frame from map-side checksums, replays and War3Log. [native-sessions.md](native-sessions.md) covers manual diagnosis. Rerun a native session that a crash, a new desync report or a premature result made invalid.
