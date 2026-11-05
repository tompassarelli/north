# Hooks against the runtime

Audited 2026-11-04 against Claude Code 2.1.317, for the 18 hook entries installed in the `claude-d`
profile (`jq '.hooks' ~/.local/state/north/profiles/claude-d/settings.json`).

## Rule

Every hook names the runtime gap it fills, and is deleted when the runtime closes that gap. The
subagent teardown stop hook broke this on 2026-11-03: it told workers to stop their own builds and tests
with TaskStop, although Claude Code already wakes a subagent when its background work completes, and it
stalled two of thirteen workers. A hook that fights the runtime costs more than the gap it fills.

## How the entries dispatch

- `north-hook.sh` sources the authoring kill-switch and then runs `north hook`. `north hook` ignores the
  matcher and picks its guards from the event and the tool name (`HookKind::from_input` and `guards()` in
  `crates/north-core/src/hook.rs`). Every `north-hook.sh` entry runs the same command line.
- Claude Code runs an identical command once per event even when several matching entries list it. The
  transcripts show it: each of 83 Edit calls, 9 PostToolUse Edit calls and 27 PostToolUse Agent calls
  has exactly one `north-hook.sh` record, although two entries match each. The hooks reference says the
  same handler in more than one settings file runs once (https://code.claude.com/docs/en/hooks). An
  extra entry with the same command is therefore dead configuration.
- Since 2.1.314, hooks that match Bash also run on the Monitor tool's shell commands, and can block or ask
  but cannot approve or rewrite them ("Fixed hooks matching Bash not running on shell commands started by
  the Monitor tool"). The 96 Monitor denials below come through the Bash entries.
- Bindings live in `harness/policy-owners.toml`; the wrapper scripts are in `harness/hooks/`.

## Method

- Use: hook records (`"type":"hook_*"` attachments) in every Claude transcript under
  `~/.local/state/north/profiles/claude-{a,b,c,d}` with a timestamp in the last 7 days, 11,400 records.
  Denials: `PreToolUse:<tool> hook error:` tool results in transcripts touched in the last 7 days
  (2,679 Bash, 96 Monitor, 511 Agent, 48 Edit, 26 Write). Spawn denials:
  `~/.local/state/agents/spawn-gate-denials.jsonl`. Hook failures: `~/.local/state/agents/hooks/errors.tsv`
  (one row in the window).
- Latency: `durationMs` from the transcript records (p50/p95), else ten direct runs of the installed
  command with a trivial payload, averaged.
- Injected characters: mean length of the `additionalContext` or system message per injection.
- Runtime: the Claude Code changelog (`claude-code` in `harness/model-watch-sources.toml`), cited by
  version.

## Entries

| # | Event / matcher | Runs | Gap it fills | Does 2.1.317 close it? | Use, last 7 days | Cost per call | Verdict and change |
|---|---|---|---|---|---|---|---|
| 1 | PermissionRequest / `Bash` | `north-hook.sh` -> `tripwire::check` | Answers a Bash permission prompt by policy so an unattended session never waits on one. | No. Auto mode judges safety, not house rules such as safe-push. | 17 permission decisions. | 17 ms measured. | **keep**. None. |
| 2 | PostToolUse / (all) | `north-hook.sh` -> `worker_handoff` (time and context lease, spawn recording on Agent), `issue_filing::link`, `code_health_edit` | Gives a worker a wall-clock and context lease, links filed issues as blockers, and reports code-health drift after an edit. | No. Claude Code has no per-subagent lease or code-health feedback. | 22 lease nudges (217 and 522 chars), 9 issue links (~70 chars), 50 code-health notes (387 chars), 37 "spawn recording boundary miss" notes (91 chars). | Bash p50 43 ms; Edit p50 247 ms, p95 914 ms; Agent p50 8,067 ms, p95 8,154 ms. | **narrow**. Take spawn recording off the synchronous PostToolUse Agent path and stop injecting boundary-miss notes into the model. |
| 3 | PostToolUse / `Agent` | `north-hook.sh`, same command as #2 | None of its own: #2 already matches Agent. | Yes. Identical commands run once; 27 of 27 Agent calls show one record. | 0 runs of its own. | 0 (deduplicated). | **delete**. Remove the entry. |
| 4 | PostToolUse / `Edit\|Write\|MultiEdit` | `north-hook.sh`, same command as #2 | None of its own: #2 already matches these tools. | Yes, deduplicated as above. | 0 runs of its own. | 0 (deduplicated). | **delete**. Remove the entry. |
| 5 | PostToolUse / `RemoteTrigger` | `north-hook.sh`, same command as #2 | None of its own: #2 already matches RemoteTrigger. | Yes, deduplicated as above. | 0 runs of its own. | 0 (deduplicated). | **delete**. Remove the entry. |
| 6 | PreToolUse / `Write\|Edit\|MultiEdit\|NotebookEdit\|Bash` | `language-guard.sh` -> `north guard language` | Refuses new code in a disallowed language at write time, before the language gate at landing. | No. The runtime has no language policy. | 19 denials. | 15 ms measured. | **keep**. None. |
| 7 | PreToolUse / `Edit\|Write\|MultiEdit` | `firn-system-policy` (firn-tools, native) | Gives an edit in the NixOS configuration its Firn rules. | No. | 7 injections (1,247 chars), 0 denials. | p50 12 ms. | **keep**. None. |
| 8 | PreToolUse / `Edit\|Write\|MultiEdit` | `north-hook.sh`, same command as #13 | None of its own: #13 is a superset matcher. | Yes, deduplicated: 83 of 83 Edit calls show one record. | 0 runs of its own. | 0 (deduplicated). | **delete**. Remove this `north-hook.sh` item; keep #7 in the group. |
| 9 | PreToolUse / `Bash` | `firn-system-policy` | Refuses shell commands that change the system outside the Firn flow. | No. | 0 records, 0 denials found. | 6 ms measured. | **keep**. None; it guards a rare, costly mistake. |
| 10 | PreToolUse / `Bash` (and Monitor commands) | `north-hook.sh` -> 24 guards: tripwire, corpus and resource-safe search, modern search, launch-critical, desktop action, blind stage, reviewer CLI, git stash, native evidence, issue labels and filing, sops, public text, frozen topics, poll loop, heavy command, session kill, clipboard, worker wait, native launch and item, generated policy, model identity | Enforces house rules for shell commands that the permission system cannot express: primary checkout writes, blind staging, raw `git push`, detached jobs, foreground waits, capacity admission, issue labels. | Partly. The runtime blocks long foreground `sleep` itself (at least 787 native "Blocked: sleep N" denials), which overlaps worker-wait's bare `sleep N` shape. The rest stays open. | 2,775 denials (top: blind staging 325, interpreter in primary checkout 220, detached job 151+98+26+22, raw git push 107, container-rooted search 98+30+16, farm-only suite 83, worker filing 76, foreground waits 316 across Bash and Monitor). Injections: 983 heavy-command routes (56 chars), 952 run-bounded wraps (163 chars), 552 search rewrites (297 chars). | p50 54 ms, p95 145 ms; 1 timeout at 10 s. | **narrow**. Drop worker-wait's bare `sleep N` shape, which the runtime already blocks. |
| 11 | PreToolUse / `Agent` | `north hook` pre-agent (was `spawn-capacity-guard.sh` -> **Python** `lib/spawn_capacity.py`, retired) | Refuses a local worker when CPU leases, desktop pressure, priority headroom or account pace say no. | No. The runtime caps concurrent subagents at 20 (2.1.217) but knows nothing of the machine or account pace. | 369 timeouts at 10 s against 68 completed runs; 7 denials; 68 "warning only" notes, each injected twice (126 chars as context and again as a system message). | Completed p50 847 ms, p95 7,214 ms; 85% of calls hit the 10 s timeout and allow by default. | **narrow**. Port the decider into `north hook` pre-agent in Rust with a deadline below the timeout, and emit the warning once. |
| 12 | PreToolUse / `Agent` | `north-hook.sh` -> `worker_tier` | Refuses a worker brief that lacks its facets or tier, or a Claude worker spawned in the foreground. | Partly. Main-session spawns default to background (2.1.232), but a subagent's own Agent calls in `-p` and the SDK run in the foreground (2.1.302), and brief facets are house rules. | 511 denials (missing facets 175, bad facet values 86 or more, foreground worker 51); 25 injections (189 chars). | p50 31 ms. | **keep**. None. |
| 13 | PreToolUse / `Edit\|Write\|MultiEdit\|NotebookEdit` | `north-hook.sh` -> `launch_critical`, `handoff_record`, `generated_policy`, `model_identity`, `file_skill` | Refuses edits in a primary checkout or generated files, and loads the skill that owns the edited file. | No. Measured 2026-11-05 on 2.1.318: a skill with `paths:` frontmatter only joins the skill listing once a matching file is touched (a `skill_listing` attachment); its body never loads unless the model calls Skill, and the skill is hidden from the listing until then. The other guards stay open too. | 74 denials (primary checkout), 136 file-skill injections (2,200 chars each, about 300k chars a week), 30 handoff-record notes (326 chars). | p50 24 ms. | **keep**. `file_skill` stays the loader on Claude and Codex; `agents sync` refuses `paths:` frontmatter (box 6). |
| 14 | SessionStart / (all) | `north-hook.sh` -> `unlanded_work` | Tells a new session about unlanded worktrees and branches it may own. | No. | About 500 injections (229 to 268 chars) naming 341 to 384 unlanded items across all repos; the count climbed all week, so nothing acts on it. | Startup p50 419 ms, p95 3,485 ms. | **narrow**. Report only the session's own repository and lanes it can land, not the global count. |
| 15 | Stop / (all) | `north-hook.sh` -> `permission_offer` | Blocks a proxy or interactive report whose closing paragraph hands Tom reversible work as a question ("want me to", "should I"), once per stop, unless it already holds a recommendation and an alternative. Fleet workers report to their lead and pass. | No. Nothing in the runtime judges a report's ending. | New 2026-11-05 after a proxy report asked to staff a box it had recommended itself. | Reads only the transcript's last 256 KiB. | **keep**. |
| 16 | SubagentStop / (all) | `north-hook.sh` -> `subagent_teardown` | Stops a worker from finishing while a background task it started still runs. | Mostly. A background subagent wakes when its last background Bash task completes (2.1.243), and background Bash is cleaned up when the agent exits (2.1.73); the SubagentStop flood bug is fixed (2.1.311). A watcher with no exit is the remaining gap. | 148 refusals ("You still have background tasks running", 165 to 216 chars). | Not measured. | **narrow**. Refuse only for watchers with no exit condition, never for builds, tests or landings. Owned by the teardown-wait lane. |
| 17 | UserPromptSubmit / (all) | `prompt-grounding.sh` -> `north grounding hook` | Injects matching skills, policy, decisions, usage and history into Tom's interactive prompt. | Partly. The runtime lists skill descriptions natively; decisions, usage and history stay open. | 2,369 injections at 5,122 chars each, about 12.1M chars a week; 45 timeouts at 10 s. | Up to 10 s on timeout. | **narrow**. Cap the block at 1,500 characters, dropping the skill listing the runtime already gives. |
| 18 | UserPromptSubmit / (all) | `north-hook.sh` -> no guards (`UserPromptSubmit => &[]`) | None: no guard is registered. | n/a. | 0 records. | 11 ms measured, every prompt. | **delete**. Remove the Claude UserPromptSubmit `north-hook.sh` item; keep #17. |

Counts: 7 keep, 6 narrow, 5 delete.

## Python in the hook path

- `spawn-capacity-guard` (#11) ran `lib/spawn_capacity.py` on every Agent call until box 5 ported it to Rust. It was also the worst
  latency in the table.
- The shared kill-switch (`lib/north-agent-activation.sh`) falls back to an inline Python reader of
  `activation.json` when `activation.active` is missing. Every wrapper sources it: `north-hook.sh`,
  `language-guard.sh` and `prompt-grounding.sh`. While `activation.active`
  exists the check is plain shell (3 ms). The Rust guards already check activation themselves
  (`guard::activation::authoring_guards_off`), so the shell prelude duplicates that check.
- Every entry sets `NORTH_AGENT_PYTHON=/etc/codex/hooks/runtime/python3`; only the two paths above read it.

## Boxes

Each box changes the Claude projection in `harness/policy-owners.toml` (and, where named, code) and lands
through `safe-push`. Codex bindings stay as they are unless the box says otherwise.

1. **Delete the duplicate north-hook entries (#3, #4, #5, #8).** Done when: `jq` over the projected
   `settings.json` shows exactly one `north-hook.sh` item under PostToolUse (matcher empty) and one under
   PreToolUse for edits (`Edit|Write|MultiEdit|NotebookEdit`), and a PostToolUse Agent and an Edit call
   in a fresh session each still show one `north-hook.sh` record.
2. **Delete the empty UserPromptSubmit north-hook entry (#18).** Stop (#15) now runs `permission_offer`
   (2026-11-05), and Codex Stop stays with `codex-conduct`. Done when: the projected `settings.json` has no
   `north-hook.sh` under UserPromptSubmit and `prompt-grounding.sh` is still under UserPromptSubmit.
3. **Take spawn recording off the PostToolUse Agent path (#2).** Done when: PostToolUse Agent
   `north-hook.sh` p95 is under 200 ms over 20 spawns in transcripts, the spawn ledger still records each
   spawn, and no "spawn recording boundary miss" text reaches `additionalContext`.
4. **Drop worker-wait's bare `sleep N` shape (#10).** Done when: a foreground `sleep 30` gets the
   runtime's own block and no `worker_wait` denial, and the `worker_wait` tests for the other shapes
   (safe-push, `timeout N`, until loops, test suites) still pass. Done 2026-11-05: `guard::worker_wait_tests::a_bare_foreground_sleep_is_left_to_the_runtime_block`.
5. **Port spawn-capacity to Rust in `north hook` pre-agent (#11).** Done when:
   `lib/spawn_capacity.py` and `spawn-capacity-guard.sh` are gone (named in `harness/retired-hooks.txt`),
   the pre-agent p95 over 20 spawns is under 1 s with no `hook_cancelled` record, the existing
   spawn-capacity decision cases pass against the Rust decider, and a warning appears once per spawn.
6. **Keep file-skill loading in the hook and refuse `paths:` frontmatter (#13).** A probe skill with
   `paths:` in a fresh Claude session (2026-11-05) gained only a listing line after a matching read, and
   its body never reached the transcript; on `rust-development`, which is listed from the start, `paths:`
   would hide it until a `.rs` file is touched. `paths:` also cannot express `file-skills.toml`'s
   `package` condition. So skills keep loading through `file_skill` from `file-skills.toml`. Done when:
   `tree_policy_refuses_a_skill_with_paths_frontmatter_because_file_skill_loads_language_skills` and the
   `guard::file_skill` tests pass.
7. **Scope the SessionStart unlanded-work notice (#14).** Done when: a session started in
   `~/code/north/main` names only north lanes, the notice is under 400 chars, and SessionStart p95 is under
   1 s over 20 starts. Done 2026-11-05: the notice counts only the cwd container's stale lanes and names up to two that
   reference the session's item (`guard::unlanded_work_tests::a_north_session_hears_only_about_stale_north_lanes_and_those_naming_its_item`).
8. **Narrow the subagent teardown refusal (#16).** Owned by the teardown-wait lane; Done when that lane
   lands and a worker with a running background build ends its turn and is resumed by the runtime
   without a SubagentStop refusal. Follow-up done 2026-11-05: a background Bash refuses only as an endless watcher
   (`tail -f`, `journalctl -f`, `watch`, `cargo watch`, `inotifywait -m`, `while true` with no break) and Monitors
   keep their expiry rule (`guard::subagent_teardown_tests::builds_tests_and_landings_may_outlive_the_turn_and_only_endless_watchers_refuse`).
9. **Cap prompt grounding (#17).** Done when: the Grounding block is at most 1,500 chars on 20 sampled
   prompts, it no longer repeats the native skill listing, and no `prompt-grounding.sh` `hook_cancelled`
   record appears over a day.
