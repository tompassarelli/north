---
name: clean-room
description: >-
  Work that mods, imitates or measures a commercial game: reading game memory or binaries, decompiled code, where game facts may come from, shipping game assets, tasks that seem to need internals.
grounded: 2026-11-04
written: 2026-11-08
metadata:
  kind: domain
---

# Clean room

Tom's standing policy for Wisp, Mak'gora, padbridge and every other project that mods, imitates or measures a commercial game. It keeps each project clean-room, so nothing we ship or publish derives from a game's protected internals. These rules hold for every game; each rights holder adds its own terms in a reference.

## Games and their rights holders

Read the row's reference before any work on that game, because terms, legal history and allowed exceptions differ by owner.

- Warcraft III (Wisp, Mak'gora, makgora-client): [blizzard.md](references/blizzard.md).
- Melee (padbridge and the Melee work in Mak'gora): [nintendo.md](references/nintendo.md).
- A game with no row: write its rights-holder reference from that owner's terms and add the row before any work starts. Until then, apply only the rules below and grant no exception.

## Never

- Read, write, trace or attach to a game's memory; use a debugger, injection, traps or hooks on a game process; disassemble, decompile, decrypt or dump a game executable, because that derives our work from the game's protected code. Only an exception a rights-holder reference grants applies, and only as it words it.
- Use decomp output: decomp-style function names, addresses, struct offsets or decomp code excerpts, because it is the same derivation by another route.
- Read values from a game's disc or install files, or from its binaries, to decide behaviour, because behaviour must come from observation and public facts, not extracted data.
- Commit, post in issues or PRs, or quote in docs any address, offset, byte pattern, dump or procedure for reading or changing a game's internals, because publishing it distributes the forbidden knowledge.
- Copy or distribute the game's code, art, models, textures, sounds, music, data tables, script bodies or text, because they belong to the rights holder; players load art from their own install at run time. Only an exception a rights-holder reference grants applies.
- Bypass DRM, emulate a publisher's sign-in, cheat, evade anti-cheat, or touch other players' games or the rights holder's services, because that harms players and the owners' services and breaks their terms.

## Where facts come from

- Black-box observation of the game as a player sees it: pictures, sound, logs and files the game writes, replays, timing, and LAN traffic of games we host, because that is what any player may legitimately see.
- Instrumentation inside our own map or mod: map-side Lua counters, checksums and frame numbers, because it runs in our code and never touches the game process.
- Public documentation and public community data, which the game's reference names. Cite them and record facts in our own words as our design targets, so our sources are traceable and our expression is our own.

## When a task seems to need the forbidden route

Stop and measure from outside instead (a map-side probe, a replay, a timed capture or a public-source lookup), because an outside measurement keeps the work clean. If none can answer, report the gap; never fall back to memory reads or decomp, because a missing fact costs less than tainted work. A task that needs more than a reference's exception allows is outside the exception; stop and report it rather than widen it.

Each repository's README section "Principles" (wisp:docs/clean-room.md in long form) is the public statement of this policy.
