# Playtests for Tom

Read before building for Tom, opening his game window, telling him what a build contains or calling a playtest ready.

- Build current main for Tom with `wisp play` ([map-code.md](map-code.md) "Builds and tests" covers map builds), and route experiments through fresh, captures or accept. Confirm the roster, stage thumbnails and gameplay before calling a build ready.
- Open Tom's game window only with the `app.slice` launch line in wisp:docs/play.md, never under `run-bounded` or a capacity lease, which put his match in `agent.slice`, and leave `WISP_WINDOW_PRESENT` unset (2026-10-31).
- Check a claim about game content (clip names, which animation a move plays, what landed in a build) against the code or the build before telling Tom: Maiev's run was reported as stock when Mak'gora had authored clip 78 since 2026-10-30 (2026-10-31).
- Tell Tom the auto-installed build's file name once it reaches his `Maps/00-Makgora`.
- A playtest is ready for Tom only when his own Steam, Battle.net and Warcraft III are open and waiting in the custom-game lobby with the RC map selected, Classic graphics and the pad connected; never start the match. Write RC-READY, and relay a playtest to Tom, only after that lobby is confirmed (LOBBY-READY with a screenshot).
