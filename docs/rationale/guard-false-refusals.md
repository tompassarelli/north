# Guard false refusals, measured 2026-11-08

Every PreToolUse guard that refused a tool call from 2026-11-01 to 2026-11-08 has a labelled sample in
`crates/north-guard/src/guard/hook_refusal_samples.toml`. `north hook stats` prints each guard's false share and lists every
guard over 10% false with its `narrowed` line, or "not narrowed" (north#706, carrying north#715's E2 method).

## Method

- **Source.** The ledger's `hook_events` refusals (9,049 in the window), joined by tool_use id to the Claude transcripts on
  the laptop and gex45, including `agent-ID.jsonl` subagent transcripts. 8,629 of the 9,049 were found.
- **Unnamed guards.** 2,285 refusals were filed as `other` because their refusal text matched no rule. Six guards were behind
  them, and `hook_rules.rs` now names them: heavy-command.local-write, launch-critical.layout, session-kill.self-match,
  privacy-judge.pending and privacy-judge, issue-cleanup, worker-tier.item and worker-tier.routing, and proxy-role.
- **Sample.** Up to 30 rows per guard: distinct refused calls sorted by text, taking every Nth.
  - launch-critical counts only refusals after its 786ac1b9d4 fix.
  - heavy-command.farm counts only refusals after its df206b8a67 fix.
- **Labels.** A row is false when the call was something the guard's own policy allows or does not aim at. Examples: text
  matched inside a heredoc, a quote or a path; a read-only command; a bounded wait; a correct brief the guard misparsed.
  Labels were judged per guard from its refusal text and source.

## Results

False share by guard (over 10% marked *):

- worker-wait 24/30 * (narrowed here)
- poll-loop 30/30 * (narrowed here)
- privacy-judge.pending 14/30 * (narrowed by 0a478d5770: no pending refusal in the 2 h after it deployed)
- issue-cleanup 24/30 *
- heavy-command.farm 2/3 * (after df206b8a67; the 26 earlier false rows were already fixed) (narrowed here)
- resource-safe-search 20/30 * (narrowed here)
- session-kill.self-match 19/30 * (narrowed here)
- session-kill 13/30 * (narrowed here)
- worker-tier.item 11/30 * (narrowed here)
- corpus-scan 8/30 * (narrowed here)
- modern-search 8/30 * (narrowed here)
- tripwire 8/30 *
- issue-filing 6/30 *
- privacy-judge 6/30 *
- tripwire.push 6/30 *
- proxy-role 6/10 * (narrowed here)
- sops-decrypt 6/18 *
- heavy-command 5/19 * (narrowed here)
- worker-tier.facet-value 5/30 * (narrowed here)
- worker-tier.lands-before 5/7 * (narrowed here)
- launch-critical 4/30 *
- tripwire.delete 4/30 *
- nix-activity.deny 4/7 * (narrowed here)
- clipboard 4/5 *
- public-text 3/15 *
- worker-tier.routing 3/14 * (narrowed here)
- desktop-action 1/6 *
- native-evidence 1/3 * (narrowed here)
- worker-tier.missing-facet 3/30
- heavy-command.local-write 2/30
- launch-critical.layout 0/30
- git-stash 0/30
- issue-labels 0/30
- masked-pipeline 0/14
- worker-tier.payback 0/20
- lead-timer 0/2
- worker-tier.background 0/1
- claude-sleep 0/30 and blind-stage 0/30, from their 2026-11-07 samples

## The largest false class: Monitor calls

poll-loop and worker-wait refused 563 Monitor calls in the window, every one carrying `timeout_ms` in its transcript.
b73718aa85 (2026-11-01) had let a Monitor call through by reading `timeout_ms` from the hook input. The refusals went on
under runtimes that held that commit; the last was at 20:58 UTC on 2026-11-07 on gex45. So Claude Code runs the Bash hooks for a
Monitor call as tool Bash without Monitor's `timeout_ms`, and the input alone cannot tell it from a foreground Bash call.

The fix: when either guard is about to refuse, it looks for a Monitor tool_use with the same id and command in the tail of
the session transcript (`guard/monitor_call.rs`), and allows the call if it finds one. The check runs only on the refusal
path, so allowed calls pay nothing. If the transcript lacks the entry, the guard refuses exactly as before.

poll-loop is active on gex45 and not on the laptop (`~/.local/state/north/agents/current/activation.active`), which is why
laptop sessions never saw it.

## north#715's verdict on main

The post-fix launch-critical sample is 4 of 30 false, one over the keep line of 3 that north#715 set. No true class was
lost. The 4 false rows fall into three classes the fix never addressed:

- a `cd` into a worktree the same command creates;
- a redirect read as a merge source in `git merge --ff-only origin/main 2>&1`;
- literal paths in a heredoc that writes only under /tmp.

Reverting the fix would bring back the 21 of 30 false the earlier sample measured, so it stays. The three new classes are
narrowing work on north#706.

## resource-safe-search and corpus-scan narrowed (north#706)

Replaying the stored sample rows (commands kept to 150 characters) through each guard before and after the change:

- resource-safe-search read 7 false rows refused before and 0 after; corpus-scan read 4 before and 0 after.
- Every true row the old code refused is still refused. The 2 and 8 true rows that were already not refused read the same
  before and after, because their stored text is cut before the searching part.

The false classes and their fixes:

- an operand after `cd DIR` was resolved against the hook's cwd, so `rg ... ..` after a `cd` into a worktree read the
  repository container; both guards now resolve it against DIR, and resource-safe-search leaves a relative operand unjudged
  after a `cd` into a shell variable;
- a redirect target (`2>/dev/null`, `> /dev/null`) was read as a search operand and a /dev/null content search refused; both
  guards drop redirect words and their targets, unless quoted;
- a glob segment such as `land-queue*` counted as `archives` or `accounts` because any glob matched; corpus-scan now matches
  the glob against the name;
- `rg` or `ag` reading a pipe got the default operand `.`; corpus-scan now skips it, as resource-safe-search already did;
- `fd -d3` was not read as a depth; corpus-scan now reads `-dN`.

## Re-sampled after the narrowings (north#706, 2026-11-08)

Each guard's refusals on gex45 from 2026-11-01 were replayed with `north hook dispatch` through main plus the change, and
only the calls still refused are kept as the new sample, so its false share is the guard as it now stands:

- tripwire.push: a push to a remote the same command clones or adds from a local path is not a GitHub push. 3 of the 23
  refusals were scratch repositories of this kind and 1 was a `--dry-run` that only the main-checkout guard now refuses;
  the 19 still refused are all true.
- tripwire: `set -e` makes a failed `cd` end the command, so a delete after `set -e; cd DIR;` is judged in DIR; a clone under
  /tmp (its `.git` a directory, outside $HOME) is the session's own scratch and may go with its `.git`. 4 of the 29 refusals
  were these classes; of the 17 still refused, 1 is false (a scratch directory a lane kept in its `.git/worktrees` entry).
- tripwire.delete: a screenshot run under `~/.local/state/PROJECT/shots/` is regenerable. 2 of the 27 refusals were such
  runs; the 9 still refused are true, and loop-variable targets among them name personal data their literal path would
  refuse too.
- sops-decrypt: all 8 refusals pipe decrypt output through a line redaction to the terminal, which the guard aims at; 0
  false.
- clipboard and desktop-action: no refusal on gex45 in the window; their stored rows replayed through main refuse 0 of the 5
  and 1 false rows and keep every true row.
