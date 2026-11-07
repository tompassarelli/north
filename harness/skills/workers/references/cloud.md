# Cloud workers (Claude-only)

Read [Placement](#placement) for where a box runs. Read the rest when `north fleet launch` refuses a box for `anthropic-cloud` (or `agents plan --place` prints `target anthropic-cloud`). Both lanes run Claude in Anthropic's cloud.

## Placement

Read this section when a box may need a local client, a GPU or the laptop, when placement holds it, or when you override it.

- A broken or slow cloud lane is an incident to fix the same tick, never a reason to run code-only work on the laptop.
- Declare `Surface:` on every brief and mark a box that is not code-only with `Needs: warcraft|native|sign-ins|private|local-state|tui|fleet`, because an undeclared box stays local. `agents plan --place BRIEF` prints the verdict `north fleet launch` will act on; override only with `--local "REASON"` or `--account A --pin "REASON"`, which the session records.
- Send a worker's native-window or GPU work to the remote Warcraft host (south:agents/skills/battlenet-accounts/) while Tom is present, because an agent's 5696x3680 native window held the laptop GPU at 100% and made his desktop choppy (2026-10-31); capacity admission queues it on the laptop.

## Running a cloud box

The account, login, host and environment IDs are in south:agents/skills/claude-accounts/.

- Run it on the host the verdict names, with `CLAUDE_CONFIG_DIR=$HOME/.local/state/north/profiles/claude-LABEL` set so `north cloud` uses that account's login (the printed command does), and never change or copy a login.
- Start: write the brief to `/tmp/cloud/NAME.md` on the launch host and run `north cloud launch REPO NAME /tmp/cloud/NAME.md` there (`north fleet dispatch` does the same for makgora and wisp). Record the printed `session_...` ID and `claude/land-NAME` in the brief's thread.
- Every tick of a lead with cloud sessions out runs `north cloud sweep /tmp/cloud/swept` on the cloud host, so a prompt never sits waiting on Tom; it never touches a Remote Control session (kind `bridge`), because archiving one cut the proxy's link to Tom on 15 Oct. Restaff a swept box with the blocked step named.
- Never teleport a cloud session, because `claude --teleport <id>` stops it (4 of 6 on 15 Oct ended `Interrupted` and lost their work). Read sessions with `north cloud list` and `north cloud events <id> OUT.jsonl` (whole transcript, without stopping it). Collect a session once it is `idle`; one still `running` at twice its ETA is restaffed with `Follows:` and a smaller scope.
- Wait for a session whose push failed with `north cloud wait <id> OUT.jsonl [--after UUID] [--timeout SECS]`, and never watch for `Done:` wording instead, because a report whose `Done:` paragraph came before trailing notes delivered the #53 handback about 2 h late.
- A session that went `idle` mid-task without pushing or handing back is resumed with `north cloud send <id> FILE` (FILE is the next instruction, for example "finish the brief and push its land branch"); pass the printed uuid to `wait --after`, then `wait` again. Never restaff it instead, because that loses its unpushed work.
- The proxy refuses branch deletes, so whoever collects a landed or abandoned `claude/land-*` branch deletes it with `git push origin --delete` from a host with GitHub write access.
- A handback lands with no lead: the laptop's `north-cloud-reap` timer runs `north cloud reap` every 2 minutes, which lands the handback of every session `north cloud launch` started in the last 7 days. A patch that does not apply, a failed check or a refused enqueue prints `Stop:` with the reason and the lane path and queues nothing; finish the work in that lane or abandon it. A corrupt or empty handback exits 1 (restaff the box).
