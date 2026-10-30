---
name: game-ip
description: >-
  Respect Blizzard's and Nintendo's IP in any game-modding or game-feel work
  (Warcraft III, Wisp, Smashcraft, Melee): where engine or game facts may come
  from, and what to do when a task seems to need the game's internals.
grounded: 2026-10-31
written: 2026-10-31
metadata:
  kind: domain
---

# Game IP

Tom's standing policy for Wisp, Smashcraft, padbridge and every other project that mods, imitates or measures a commercial game. It keeps each project clean-room, so nothing we ship or publish derives from a game's protected internals.

## Never

- Read, write, trace or attach to a game's memory; use a debugger, injection, traps or hooks on a game process; disassemble, decompile, decrypt or dump a game executable, because that derives our work from the game's protected code. The LAN switch on our own offline test clients, below, is the only exception.
- Use decomp output: decomp-style function names, addresses, struct offsets or decomp code excerpts, because it is the same derivation by another route.
- Read values from Melee's disc files, or from game binaries, to decide behaviour, because behaviour must come from observation and public facts, not extracted data.
- Commit, post in issues or PRs, or quote in docs any address, offset, byte pattern, dump or procedure for reading or changing a game's internals, because publishing it distributes the forbidden knowledge.
- Copy or distribute the game's code, art, models, textures, sounds, music, data tables, script bodies or text, because they belong to Blizzard and Nintendo; players load art from their own install at run time.
- Bypass DRM, emulate Battle.net sign-in, cheat, evade anti-cheat, or touch other players' games or Blizzard's or Nintendo's services, because that harms players and the owners' services and breaks their terms.

## Where facts come from

- Black-box observation of the game as a player sees it: pictures, sound, logs and files the game writes, replays, timing, and LAN traffic of games we host, because that is what any player may legitimately see.
- Instrumentation inside our own map: map-side Lua counters, checksums and frame numbers, because it runs in our code and never touches the game process.
- Public documentation and public community data: patch notes, Hive Workshop, and for Melee, SmashWiki, Smashboards and public frame-data sites. Cite them and record facts in our own words as our design targets, so our sources are traceable and our expression is our own.

## Online play: Battle.net in Warcraft, Wisp's window outside it

Warcraft III online play is Battle.net only, in Warcraft's own menus. Smashcraft provides no online play inside the Warcraft client. Online play is Wisp's own window over Wisp's relay, with Warcraft III closed, because that route never touches Warcraft's memory.

Tom approved Smashcraft Servers, an in-game LAN switch for players as an opt-in in smashcraft-client, on 2026-10-25, and withdrew that approval on 2026-10-31 after an outside review named the switch the project's one real legal exposure (the bnetd and Glider cases: a EULA's reverse-engineering ban and DMCA anti-circumvention). So:

- No player build turns on Warcraft III's LAN network provider, writes into a running game, or drives Warcraft III to host or join online games for the player.
- The private LAN switch under `~/.local/share/wisp-private/` (wisp:host/lan/plugin.ts) turns on the LAN provider only on our own offline test clients that hold no Battle.net account. Locating the provider on a Warcraft build, and writing a missing provider into the running game, are allowed only there too, because no account and no player is at stake.
- Battle.net and real LAN inside Warcraft are unaffected: players host and join there through Warcraft's own menus.

Everything else stays forbidden even on those test clients:

- Any other read or write of game memory, and reading hidden game state.
- Sending, changing or timing match input from inside the game.
- Evading anti-cheat: if the game refuses or detects the switch, it reports unavailable and stops.
- Touching ranked or ladder play.
- Copying W3Champions code; FLO is a reference for behaviour only (external-code skill).
- Deciding Wisp or Smashcraft behaviour from what the switch shows.

The switch's addresses, offsets, byte patterns, procedures and logs live only in a private repository and never ship in any client. North, Wisp, Smashcraft and smashcraft-client are public, so their code, issues, PRs and docs never see the internals. Shipping the switch to players again needs a new decision from Tom.

## When a task seems to need the forbidden route

Stop and measure from outside instead (a map-side probe, a replay, a timed capture or a public-source lookup), because an outside measurement keeps the work clean. If none can answer, report the gap; never fall back to memory reads or decomp, because a missing fact costs less than tainted work. A task that needs more than the offline test switch allows is outside the exception; stop and report it rather than widen it.

The desync debugger and the pool's LAN switch under `~/.local/share/wisp-private/` only serve our own test clients (for example, which of our clients diverged). Their output never decides Wisp or Smashcraft behaviour, and their details never enter a repository, issue or doc, because they would otherwise become a route to the game's internals.

Each repository's README section "Principles" (wisp:docs/clean-room.md in long form) is the public statement of this policy.
