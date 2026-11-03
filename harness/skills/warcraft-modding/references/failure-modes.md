# Failure modes seen in Mak'gora

Symptom, cause, rule. Mak'gora, 6-7 Oct 2026.

- "no element 1 in a table of 1" every tick at stage select: presentation built
  for one selection (stage decks drawn at shell start) outlived it. Key
  persistent presentation by the selection it was built for; headless journeys
  step through stage and fighter selection with computer slots
  (makgora:ts/test/bot-selection.test.ts).
- Bun and Lua tapes diverge (stray element, electric): a scratch object reused
  across hits/throws leaked fields; Bun runs all tapes in one process, Lua one
  process per tape. Reset every field of a reused scratch object; tapes catch it
  only when run in sequence.
- Every lane's Lua suite breaks on a literal like 0.02 or 1.4: not binary32 per
  the emitted-Lua number check. Wrap non-integer literals in `f32(...)` in tests too.
- 100+ MB Lua allocation spikes: per-kit text rebuilt by string concatenation in
  every checksum. Precompute static digests at load; no per-frame string building.
- Desync hunt ran 3.5 hours of one-variable native experiments (#158): nobody
  compared map-side checksums first. Bisect with map-side checksums and replays.
- Green HUD, invisible stage, "menus could not load": `-loadfile` raced the
  post-login ladder-map scan. Host via the menu socket after the scan; treat any
  "model creation failed - war3mapImported" as a failed load.
- Private test client at the wrong aspect, pointer clamps: main-display play
  sharing its prefix rewrote War3Preferences on exit. One prefix per display
  role, or save/restore War3Preferences (wisp:docs/display-settings.md).
- Missing log line read as a state (no LoginDoorClose): War3Log is written in
  bursts. The menu socket outranks the log for sign-in and menus.
- Grey sky for seconds in the first match after a cold start: models draw late.
  Preload stage/scene models; take evidence frames only after a receipt says
  the scene is drawn.
- Missed clicks, wrong pointer targets, slow OCR: XTEST clicks shorter than one
  frame are missed by per-frame-sampled UI. Drive with in-map commands and
  receipts plus the menu socket.
- Receipt wait times out on a fresh game: it rewrote an identical receipt file.
  Wait on a counter or timestamp, never text equality.
- `bun wisp play` broke for every lane ("extract war3mapImported\\...Clip47
  failed", "Cannot open map archive"): ~20 lanes hand-edited one global input
  pointer, and concurrent builders shared an output folder, a `.next` file and
  a build worktree. Content-address shared inputs and name them per revision in
  Git; build into private staging, publish by one rename under a per-key lock.
- A panel button draws and passes every headless test, but clicks are dead or
  partly live (Controls "Done" took clicks on its leftmost ~5% at 2880x1920,
  2026-11-03; makgora#1047): Warcraft's stock command, item, hero, minimap
  and system-button frames lie over the panel and take clicks even when not
  drawn; `BlzHideOriginFrames(true)` leaves them in place, and the headless
  frame model holds only map-made frames, so it cannot see them. A native
  bisect at 3:2 found `ConsoleUI` children 1, 5 and 7 (the command bar and two
  sibling bars) as the frames that swallowed the clicks. Hide and disable each
  stock frame by name at startup (makgora:ts/src/game/ui/stockClicks.ts and
  stockFrames.ts) and click every panel button natively at several points
  across its width.
  Never hide a stock frame as a group: `ConsoleUIBackdrop` is the parent of our
  full-window backdrops, the pause screen and the fighter markers, the game UI
  holds our panels, and the chat box and world view stop working when the stock
  UI is hidden wholesale (makgora:docs/stock-frames.md). Test the all-hidden
  state before shipping.
