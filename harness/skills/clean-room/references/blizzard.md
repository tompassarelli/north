# Blizzard: Warcraft III

Read before Wisp, Mak'gora or makgora-client work that measures the client, touches online play or ships art. It adds Blizzard's terms to the rules in [SKILL.md](../SKILL.md).

## Where facts come from

- Black-box sources and map-side instrumentation as in SKILL.md.
- Public documentation and public community data: patch notes and Hive Workshop. Cite them and record facts in our own words.

## Shipping Blizzard's art

Blizzard owns the game's code, art, models, textures, sounds, music, data tables, script bodies and text. The one exception: credited Hive Workshop models, including ones their authors ported from other Blizzard games, may ship inside the Mak'gora map, because that map ships only on Hive Workshop and Battle.net as the Warcraft III community's maps do (Tom's decision, 2026-11-04); they never enter Git or a GitHub release (makgora's after-landing `release-assets` check), and Wisp's standalone engine keeps the ban.

## Online play: Battle.net in Warcraft, Wisp's window outside it

Warcraft III online play is Battle.net only, in Warcraft's own menus. Mak'gora provides no online play inside the Warcraft client. Online play is Wisp's own window over Wisp's relay, with Warcraft III closed, because that route never touches Warcraft's memory.

Mak'gora Servers, an in-game LAN switch for players, is withdrawn: an outside review named it the project's one real legal exposure (a EULA's reverse-engineering ban and DMCA anti-circumvention). The decision and its reasoning are in south:agents/decisions/smashcraft/smashcraft-servers-stopped-2026-10-31.md. So:

- No player build turns on Warcraft III's LAN network provider, writes into a running game, or drives Warcraft III to host or join online games for the player.
- The private LAN switch (wisp:host/lan/plugin.ts; its private location is in that south document) turns on the LAN provider only on our own offline test clients that hold no Battle.net account. This is the one exception to SKILL.md's ban on touching a game process. Locating the provider on a Warcraft build, and writing a missing provider into the running game, are allowed only there too, because no account and no player is at stake.
- Battle.net and real LAN inside Warcraft are unaffected: players host and join there through Warcraft's own menus.

Everything else stays forbidden even on those test clients:

- Any other read or write of game memory, and reading hidden game state.
- Sending, changing or timing match input from inside the game.
- Evading anti-cheat: if the game refuses or detects the switch, it reports unavailable and stops.
- Touching ranked or ladder play.
- Copying W3Champions code; FLO is a reference for behaviour only (external-code skill).
- Deciding Wisp or Mak'gora behaviour from what the switch shows.

The switch's addresses, offsets, byte patterns, procedures and logs live only in a private repository and never ship in any client. North, Wisp, Mak'gora and makgora-client are public, so their code, issues, PRs and docs never see the internals. Shipping the switch to players again needs a new decision from Tom.

The desync debugger and the pool's LAN switch only serve our own test clients (for example, which of our clients diverged). Their output never decides Wisp or Mak'gora behaviour, and their details never enter a repository, issue or doc, because they would otherwise become a route to the game's internals.
