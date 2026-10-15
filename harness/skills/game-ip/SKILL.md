---
name: game-ip
description: >-
  Respect Blizzard's and Nintendo's IP in any game-modding or game-feel work
  (Warcraft III, Wisp, Smashcraft, Melee): where engine or game facts may come
  from, and what to do when a task seems to need the game's internals.
grounded: 2026-10-15
written: 2026-10-15
metadata:
  kind: domain
---

# Game IP

Tom's standing policy for Wisp, Smashcraft, padbridge and every other project that mods, imitates or measures a commercial game. It keeps each project clean-room, so nothing we ship or publish derives from a game's protected internals.

## Never

- Read, write, trace or attach to a game's memory; use a debugger, injection, traps or hooks on a game process; disassemble, decompile, decrypt or dump a game executable, because that derives our work from the game's protected code.
- Use decomp output: decomp-style function names, addresses, struct offsets or decomp code excerpts, because it is the same derivation by another route.
- Read values from Melee's disc files, or from game binaries, to decide behaviour, because behaviour must come from observation and public facts, not extracted data.
- Commit, post in issues or PRs, or quote in docs any address, offset, byte pattern, dump or procedure for reading a game's internals, because publishing it distributes the forbidden knowledge.
- Copy or distribute the game's code, art, models, textures, sounds, music, data tables, script bodies or text, because they belong to Blizzard and Nintendo; players load art from their own install at run time.
- Bypass DRM, emulate Battle.net sign-in, cheat, evade anti-cheat, or touch other players' games or Blizzard's or Nintendo's services, because that harms players and the owners' services and breaks their terms.

## Where facts come from

- Black-box observation of the game as a player sees it: pictures, sound, logs and files the game writes, replays, timing, and LAN traffic of games we host, because that is what any player may legitimately see.
- Instrumentation inside our own map: map-side Lua counters, checksums and frame numbers, because it runs in our code and never touches the game process.
- Public documentation and public community data: patch notes, Hive Workshop, and for Melee, SmashWiki, Smashboards and public frame-data sites. Cite them and record facts in our own words as our design targets, so our sources are traceable and our expression is our own.

## When a task seems to need the forbidden route

Stop and measure from outside instead (a map-side probe, a replay, a timed capture or a public-source lookup), because an outside measurement keeps the work clean. If none can answer, report the gap; never fall back to memory reads or decomp, because a missing fact costs less than tainted work.

The private LAN switch and desync debugger under `~/.local/share/wisp-private/` only diagnose our own test clients (for example, which of our clients diverged). Their output never decides Wisp or Smashcraft behaviour, and their details never enter a repository, issue or doc, because they would otherwise become a route to the game's internals.

Each repository's README section "Principles" (wisp:docs/clean-room.md in long form) is the public statement of this policy.
