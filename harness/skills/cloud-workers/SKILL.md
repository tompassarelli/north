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
- Collect only after the ETA, because teleporting interrupts a running turn (resume it with a `claude -p` follow-up): `ssh nexus 'cd /tmp/cloud && tmux new -d -s <id> claude --teleport <id>'`, wait about 20 seconds, `tmux send-keys -t <id> '/export /tmp/cloud/<id>.txt' Enter` (the pane keeps about 50 lines), copy the file, then `tmux send-keys -t <id> /exit Enter`; `claude -p "<msg>" --cloud <id>` only sends a follow-up and prints no reply.
- The session has no GitHub access on Proton, so it reads public repositories anonymously (north, wisp, smashcraft), cannot read galileo and cannot push: brief it to end with `git format-patch origin/main..HEAD --stdout | gzip -9 | base64 -w 76` between BEGIN-PATCH and END-PATCH lines (the export wraps a plain diff at 80 columns), and the lead decodes the last block and applies it with `git am -3` in a worktree, re-authors each commit with `git commit --amend --no-edit --reset-author` (the patch carries Tom's private email, which GitHub refuses and a public repo must not show), and lands it through safe-push.
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
