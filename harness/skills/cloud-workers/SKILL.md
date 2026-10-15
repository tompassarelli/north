---
name: cloud-workers
agents: [claude]
description: >-
  Run every code-only worker task in Anthropic's cloud first (Proton credit
  lane, then RemoteTrigger routines); local workers take only Warcraft
  clients, private game assets, the LAN pool or unpushed local state.
grounded: 2026-10-13
written: 2026-10-13
metadata:
  kind: playbook
---

# Cloud workers

## Proton credit lane (until 2026-11-05T07:59Z)

- `agents plan --place BRIEF` prints `target anthropic-cloud` for a code-only box before the credit's expiry (`[placement] cloud_credit` in orchestration.toml names its costs.toml entry) unless a readout records it spent; send that box here, at about $70/week, then use the RemoteTrigger path below. `agents plan --credit` prints `cloud credit: unread, expires in N days` while no readout is under 7 days old; the tick asks Tom for his readout weekly. claude:a's weekly % and pace do not gate it: a cloud session spends the credit first.
- Run it on Nexus, whose `~/.claude` login is Proton (claude:a); not the laptop's `~/.claude-b`, and never change either login.
- Start: `ssh -tt nexus 'mkdir -p /tmp/cloud && cd /tmp/cloud && claude --account a --cloud "<brief>"'` (the wrapper refuses an unnamed account without headroom) prints `Created cloud session: ...` and the `session_...` ID, then exits; `--cloud` refuses `-p`. Record the ID in the brief's thread.
- A cloud session always runs in auto mode, and auto mode sends Tom's phone a permission prompt for some Bash calls (S1c on 15 Oct prompted while killing its own harness processes). Launch flags cannot change it: three 15 Oct probes ran a write under `--permission-mode dontAsk` and under `--settings '{"permissions":{"defaultMode":"dontAsk"}}'`. So every brief carries this line: "Run only build, test, git and file commands inside your clone and /tmp. Never kill, pkill or signal a process, never use sudo, and use no network beyond git clone, git fetch and package installs. If a command is blocked or would need approval, do not retry it or work around it; stop and report `Blocked:` with the command." Never answer a cloud prompt for Tom. Every tick of a lead with cloud sessions out runs `harness/skills/cloud-workers/scripts/cloud-sessions.sh sweep /tmp/cloud/swept` on Nexus: it saves the transcript of each cloud-run session waiting on a human and archives it, so a prompt never sits on Tom's phone; it never touches a Remote Control session (kind `bridge`), because archiving one cut the proxy's link to Tom on 15 Oct. Restaff a swept box with the blocked step named.
- Never teleport a cloud session: `claude --teleport <id>` stops it (4 of 6 on 15 Oct ended `Interrupted` mid-work and lost their work). Read sessions through the API on Nexus instead, with `cloud-sessions.sh` from this skill's scripts: `list` prints each session's id, status (`running`, `idle` once its turn ended, `requires_action` while it waits on a human, `archived`), kind (`anthropic_cloud`, or `bridge` for Remote Control), update time and title; `events <id> OUT.jsonl` saves its whole transcript, tool outputs included, without stopping it. Collect a session once `list` shows it `idle`; one still `running` at twice its ETA gets restaffed with `Follows:` and a smaller scope.
- The session has no GitHub access on Proton (pushes get 403), so it reads public repositories anonymously (north, wisp, smashcraft), cannot read galileo and cannot push. Brief it to hand back through a Bash tool call and never retype a patch: retyped base64 arrived corrupted for 3 of 5 sessions on 15 Oct. The brief's last step is `git format-patch origin/main..HEAD --stdout | gzip -9 | base64 -w 76 > /tmp/handback.b64`, then, because a transcript keeps only the first 30,000 characters of one tool output, one Bash call per 300-line part i of n: `echo BEGIN-HANDBACK part i/n; sed -n <first>,<last>p /tmp/handback.b64; echo END-HANDBACK part i/n` (a patch under 300 lines may use one `BEGIN-HANDBACK` ... `END-HANDBACK` call). To decode, take the latest handback in the tool outputs of the `events` transcript, join the lines between each `BEGIN-HANDBACK part i/n` and its `END-HANDBACK part i/n` in part order (or take the lines of a single unnumbered `BEGIN-HANDBACK` ... `END-HANDBACK` block), and pipe them through `base64 -d | gunzip` to an mbox; the latest handback is the result, so a missing part, a failed decode or output that does not start with `From ` makes it corrupt (restaff the box), and empty output means the session committed nothing. Apply it with `git am -3` in a worktree, re-author each commit with `git commit --amend --no-edit --reset-author` (the patch carries Tom's private email, which GitHub refuses and a public repo must not show), and land it through safe-push.
- No CLI, `/usage` or API shows the credit balance; only Tom's Proton claude.ai usage page does, so never open it. Record his readout as cumulative `usd_spent_period` with `read_at` in `~/.local/state/agents/costs/claude-a-cloud-credit.json`.

## Routines

- Use only public Wisp/Smashcraft code tasks without private/local inputs, Warcraft clients or Tom's install.
- Budget 4 cores, 15 GB, Bun 1.4.2, gcc/clang and 100 runs/hour/account; use pinned Bun 1.3.13 for these repositories.
- Build Lua 5.3.6 with LUA_32BITS or use Wisp's pinned command when Lua is required.
- Keep account extra usage off so plan limits pause work instead of billing.
- Run RemoteTrigger only from the main session and reuse one routine/repo.
- Set `session_context.model` on every routine update: `claude-opus-5-5` for meaningful or complex work, `claude-haiku-5-5` for very simple mechanical work; never leave it unset.
- Update the routine brief with a fresh v4 UUID, run it and retain its returned session ID.
- Read get_run_log or list_runs for its result.
- Put goal/files/Done when/ETA in a self-contained brief with `Refs repo#N` commits and no push notifications.
- End each prompt with `push to claude/<name>; it lands itself if it passes`.
- Let Autoland rebase/check/farm/compare and land sequentially; fix conflicts or new failures on the same branch.
- Read a refusal's reason in its run's job summary and annotation (`gh run list --workflow autoland.yml --branch claude/<name>`, `gh run view ID`); Autoland and the sheriff post no issue comments.
- Tell each brief to fetch and rebase onto origin/main right before its push; most cloud refusals are conflicts from main moving during the run.
- Retry a branch through `gh workflow run autoland.yml -R tompassarelli/<repo> -f branch=claude/<name>`.
- Land workflow changes locally through safe-push.

Use [RemoteTrigger schema](references/remote-trigger.md) when creating a routine; delete routines only at claude.ai/code/routines.
Use each repository's indexed docs/ci.md Autoland topic for landing details.
