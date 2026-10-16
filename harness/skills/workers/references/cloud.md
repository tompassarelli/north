# Cloud workers (Claude-only)

Read this when `north fleet launch` refuses a box for `anthropic-cloud` (or `agents plan --place` prints `target anthropic-cloud`), or when you write a RemoteTrigger routine. Both lanes run Claude in Anthropic's cloud.

The launcher enforces placement: `north fleet launch` of a code-only box on a public repository exits nonzero with the Nexus command below, the safety line and the handback step, unless `--local "REASON"` is given, which the session records and `north fleet list` shows. A box is code-only only when its brief declares a code `Surface:` (rust, ts, lua, shell...), no local `Needs:` (warcraft, native, sign-ins, private, local-state, tui, fleet, live) and a public repository (`Repository:` or the `Item:` repo); an undeclared box stays local.

## Proton credit lane (until 2026-11-05T07:59Z)

- Placement sends a code-only box to `anthropic-cloud` before the credit expires (`[placement] cloud_credit` in orchestration.toml names its costs.toml entry) unless a readout records it spent; send that box here, at about $70/week, then use the routines below. claude:a's weekly % and pace do not gate it, because a cloud session spends the credit first.
- Run it on Nexus, whose `~/.claude` login is Proton (claude:a), never the laptop's `~/.claude-b`, and never change either login, because the credit belongs to the Proton account only.
- Start: `ssh -tt nexus 'mkdir -p /tmp/cloud && cd /tmp/cloud && claude --cloud "<brief>"'`. It prints `Created cloud session: ...` and the `session_...` ID, then exits (`--cloud` refuses `-p`). Record the ID in the brief's thread so the box can be collected.
- Every cloud brief carries this line, because cloud sessions always run in auto mode and send Tom's phone a permission prompt for some Bash calls, which no launch flag suppresses (three probes on 15 Oct): "Run only build, test, git and file commands inside your clone and /tmp. Never kill, pkill or signal a process, never use sudo, and use no network beyond git clone, git fetch and package installs. If a command is blocked or would need approval, do not retry it or work around it; stop and report `Blocked:` with the command." Never answer a cloud prompt for Tom.
- Every tick of a lead with cloud sessions out runs `harness/skills/workers/scripts/cloud-sessions.sh sweep /tmp/cloud/swept` on Nexus, so a prompt never sits on Tom's phone: it saves and archives each cloud-run session waiting on a human, and never touches a Remote Control session (kind `bridge`), because archiving one cut the proxy's link to Tom on 15 Oct. Restaff a swept box with the blocked step named.
- Never teleport a cloud session, because `claude --teleport <id>` stops it (4 of 6 on 15 Oct ended `Interrupted` and lost their work). Read sessions on Nexus with `cloud-sessions.sh list` (id, status `running|idle|requires_action|archived`, kind, update time, title) and `cloud-sessions.sh events <id> OUT.jsonl` (whole transcript, without stopping it). Collect a session once it is `idle`; one still `running` at twice its ETA is restaffed with `Follows:` and a smaller scope.
- The session has no GitHub access on Proton (pushes get 403): it reads public repositories anonymously (north, wisp, smashcraft) and cannot read galileo. It hands back through a Bash call, never a retyped patch, because retyped base64 arrived corrupted for 3 of 5 sessions on 15 Oct. The brief's last step is `git format-patch origin/main..HEAD --stdout | gzip -9 | base64 -w 76 > /tmp/handback.b64`, then one Bash call per 300-line part i of n, because a transcript keeps only the first 30,000 characters of one tool output: `echo BEGIN-HANDBACK part i/n; sed -n <first>,<last>p /tmp/handback.b64; echo END-HANDBACK part i/n` (a patch under 300 lines may use one unnumbered `BEGIN-HANDBACK` ... `END-HANDBACK` call).
- To decode, take the latest handback in the `events` transcript's tool outputs, join each part's lines in part order and pipe them through `base64 -d | gunzip` to an mbox. A missing part, a failed decode or output not starting with `From ` makes it corrupt (restaff the box); empty output means the session committed nothing. Apply it with `git am -3` in a worktree, re-author each commit with `git commit --amend --no-edit --reset-author`, because the patch carries Tom's private email, which GitHub refuses and a public repo must not show, and land it through safe-push.
- Only Tom's Proton claude.ai usage page shows the credit balance, so never open it; record his readout as cumulative `usd_spent_period` with `read_at` in `~/.local/state/agents/costs/claude-a-cloud-credit.json`. `agents plan --credit` prints `cloud credit: unread, expires in N days` while no readout is under 7 days old, and the tick asks Tom weekly.

## RemoteTrigger routines

- Use them only for public Wisp/Smashcraft code tasks without private or local inputs, Warcraft clients or Tom's install, because the routine clones the public repository and nothing else.
- Budget 4 cores, 15 GB, Bun 1.4.2, gcc/clang and 100 runs/hour/account; use the repositories' pinned Bun 1.3.13, and build Lua 5.3.6 with LUA_32BITS or Wisp's pinned command when Lua is required, so results match local checks.
- Keep account extra usage off so plan limits pause work instead of billing.
- Run RemoteTrigger only from the main session and reuse one routine per repository, because routines are deleted only by hand at claude.ai/code/routines.
- Set `session_context.model` on every update (`claude-opus-5-5` for meaningful or complex work, `claude-haiku-5-5` for very simple mechanical work), because an unset model falls back to an unchosen default.
- Update the brief with a fresh v4 UUID, then run it in a separate later call, because a run sent in parallel with its update fires the previous prompt. Keep the returned session ID and read the result with get_run_log or list_runs.
- The brief is self-contained (goal, files, Done when, ETA, `Refs repo#N` commits, no push notifications), fetches and rebases onto origin/main right before its push, because most cloud refusals are conflicts from main moving during the run, and ends with `push to claude/<name>; it lands itself if it passes`.
- Autoland rebases, checks, runs the farm compare and lands sequentially; fix conflicts or new failures on the same branch. Read a refusal's reason in its run's job summary and annotation (`gh run list --workflow autoland.yml --branch claude/<name>`, `gh run view ID`), because Autoland posts no issue comments. Retry with `gh workflow run autoland.yml -R tompassarelli/<repo> -f branch=claude/<name>`. Land workflow changes locally through safe-push. Each repository's docs/ci.md Autoland topic has the landing details.

Routine schema:

```json
{"name": "<repo>-cloud-worker", "run_once_at": "<far future>", "enabled": true,
 "job_config": {"ccr": {
   "environment_id": "env_01EkrXafT5PjQN9jUWzwMhLd",
   "session_context": {"model": "claude-opus-5-5",
     "sources": [{"git_repository": {"url": "https://github.com/tompassarelli/<repo>"}}],
     "allowed_tools": ["..."]},
   "events": [{"data": {"uuid": "<fresh v4>", "session_id": "", "type": "user",
     "parent_tool_use_id": null,
     "message": {"role": "user", "content": "<PROMPT>"}}}]}}}
```

Smashcraft routine: `trig_01RoHaKMjMDSpHAouCU3FkmU`, environment `env_01MLPz6krPVjM4pH3YGTpQz3` (an API-created routine needs one save in the web editor to bind it).
