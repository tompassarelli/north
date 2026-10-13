---
name: game-ip
description: >-
  Respect Blizzard's and Nintendo's IP in any game-modding or game-feel work
  (Warcraft III, Wisp, Smashcraft, Melee): where engine or game facts may come
  from, and what to do when a task seems to need the game's internals.
grounded: 2026-10-13
written: 2026-10-13
metadata:
  kind: domain
---

# Game IP

Tom's standing policy for Wisp, Smashcraft, padbridge and every other project that mods, imitates or measures a commercial game.

## Never

- Read, write, trace or attach to a game's memory; use a debugger, injection, traps or hooks on a game process; disassemble, decompile, decrypt or dump a game executable.
- Use decomp output: decomp-style function names, addresses, struct offsets or decomp code excerpts.
- Read values from Melee's disc files, or from game binaries, to decide behaviour.
- Commit, post in issues or PRs, or quote in docs any address, offset, byte pattern, dump or procedure for reading a game's internals.
- Copy or distribute the game's code, art, models, textures, sounds, music, data tables, script bodies or text. Players load art from their own install at run time.
- Bypass DRM, emulate Battle.net sign-in, cheat, evade anti-cheat, or touch other players' games or Blizzard's or Nintendo's services.

## Where facts come from

- Black-box observation of the game as a player sees it: pictures, sound, logs and files the game writes, replays, timing, and LAN traffic of games we host.
- Instrumentation inside our own map: map-side Lua counters, checksums and frame numbers.
- Public documentation and public community data: patch notes, Hive Workshop, and for Melee, SmashWiki, Smashboards and public frame-data sites. Cite them and record facts in our own words as our design targets.

## When a task seems to need the forbidden route

Stop and measure from outside instead: design a map-side probe, a replay, a timed capture or a public-source lookup. If no outside measurement can answer, report the gap; never fall back to memory reads or decomp.

The private LAN switch and desync debugger under `~/.local/share/wisp-private/` only diagnose our own test clients (for example, which of our clients diverged). Their output never decides Wisp or Smashcraft behaviour, and their details never enter a repository, issue or doc.

Each repository's README section "Respecting Blizzard's and Nintendo's IP" (wisp:docs/clean-room.md in long form) is the public statement of this policy.
