# Rules and their enforcers

Read this before writing a prose rule, to see whether an enforcer already
holds it, and when you delete prose an enforcer took over. Hook rows come from
`harness/policy-owners.toml` (`guard[]`) and the hook `triggerDescription`s in
south `agents/catalog-config.json`; gate rows come from `bin/safe-push` and
`north language-gate`. Update this table in the same change as the enforcer.

## Edit-time hooks

| Rule | Enforcer | Providers |
| --- | --- | --- |
| No new Python files or Python line growth; shell glue stays at most 80 code lines; write Rust by default | `language-guard` | Claude, Codex |
| No placeholder model identities in Todo attempts or calibration records | `concrete-model-identity-guard` | Codex |
| No broad transcript archive scans; use convo or a bounded transcript target | `corpus-scan-guard` | Claude, Codex |
| System configuration operations go through Firn's source-owned policy | `firn-system-policy` | Claude, Codex |
| No unnamed stash pushes and no stash pop, because every worktree shares the stash | `north-hook` | Claude, Codex |
| Native evidence uses the batch planner; solo exceptions record their reason in the run ledger, and quiet leases belong to timing batches | `north-hook` | Claude, Codex |
| No gh write that names a native threads item or quotes its text | `native-item-guard` | Claude, Codex |
| No gh issue, PR or API text that matches the public deny-list, unless the repository is private | `north-hook` (public-text) | Claude, Codex |
| No bulk Git staging or automatic commit staging; enumerate paths | `north-hook` | Claude, Codex |
| Keep secrets out of output: `sops decrypt` or `sops -d` output never reaches the terminal; extract one value into a pipe or send the output to a file | `north-hook` (sops-decrypt) | Claude, Codex |
| No recursive grep or find tree searches; use rg or fd | `modern-search-guard` | Claude, Codex |
| No recursive searches of virtual filesystems or repository containers | `resource-safe-search-guard` | Codex |
| No session-wide kills, user-manager teardown (also inside an ssh remote command) or unmanaged background child launches | `session-kill-guard` | Claude, Codex |
| No destructive commands, unsafe Git publishing or sensitive outbound transfers | `north-hook` (tripwire) | Claude, Codex |
| Codex prompts, plans and endings avoid the recorded delivery failures | `codex-behavior-guard` | Codex |
| A worker at 350k tokens of context lands what passes and hands off the rest | `worker-handoff` | Claude |
| A worker never runs a foreground command that can block past 60 seconds | `worker-wait-guard` | Claude |
| Worker briefs carry Category, Spec, Scope, Surface and Verify, and tier rules hold | `worker-tier-guard` | Claude, Codex |
| A subagent does not finish while its background shells or Monitor watches run | `subagent-teardown` | Claude |
| No new local worker once capacity leases hold the CPU limit or desktop pressure is above 20% | `spawn-capacity-guard` | Claude |
| Wine, Proton, steam-run or Warcraft .exe launches run inside a machine-capacity native session | `native-launch-guard` | Claude |
| Rust PreBash routes cargo test, TypeScript builds and multi-file Bun tests through machine-capacity; refuses unleased builds, full suites, renders and CPU throttles | `heavy-command-guard` through `north-hook` | Claude, Codex |
| Claude only: no non-Claude model CLI as a reviewer, per Tom's standing order | `reviewer_cli` in `north-hook` | Claude, Codex |
| No agent wl-copy or niri screenshot actions, because they replace Tom's pending paste | `clipboard-guard` | Claude, Codex |
| No window focus, move, fullscreen, close or typed input on Tom's desktop unless the seat is idle 60 seconds, the private desktop is the target, or a sanctioned tool restores focus | `north-hook` | Claude, Codex |
| No hand edits of generated CLAUDE.md, synced Codex files or .nix generated from .gal; edit the source | `generated-policy-guard` | Claude, Codex |
| New structural debt in a repo with structure.toml is reported after the edit (advisory) | `code-health-edit` | Claude, Codex |
| Unlanded lanes older than 24 h are surfaced at session start (advisory) | `unlanded-work` | Claude |

`prompt-grounding` delivers context rather than enforcing a rule; `north-hook`
dispatches provider events to the in-process guards above.

## Landing gates in safe-push

These run for every provider, because every landing goes through `safe-push`.

| Rule | Enforcer |
| --- | --- |
| No force or destructive pushes | `safe-push` argument check |
| Origin carries the default branch only; publish with `--to main` | `enforce_one_branch_main` |
| Repository AGENTS.md and instruction files stay within their limits | `check_instruction_limits` (`agent-instruction-check`) |
| No secrets in pushed commits | gitleaks secret scan |
| Private identity values never reach a public repository | `denylist_scan` |
| Tom-specific facts in a public skill live behind a `south:` pointer | `denylist_scan` (south `privacy/public-denylist.toml` host, account, billing and ID rules; a missing deny-list refuses a public push where south exists) |
| Author and committer emails match the configured identity | `commit_email_scan` |
| Branch names and commit messages never name a native threads item | `native_ref_scan` |
| Core tools stay; other tool removals need `Removes-tool:` | `tool_removal_scan` |
| No added test skip or focus, or removed test, without a `Test-change:` trailer; claimed mutants have recorded patches | `test_integrity_scan` |
| Generated records are not mass-rewritten without their declared inputs changing | `generated_rewrite_scan` |
| A new gating bar, hook or policy line carries a Provenance record | `provenance_scan` (`north provenance check`) |
| A covered harness landing carries a `Reviewed:` trailer whose patch-id matches its final covered diff | `review_scan` (`north review-gate check`) |
| Main's CI budget is respected | `ci_budget_scan` |
| No new Python or script path, Python or script growth, or shell past 80 code lines without a Tom-approved allowlist entry | `run_language_gate` (`north language-gate`) |
| A new dependency names its reason in a `Dependency:` trailer, and new versions wait out a 7-day cooldown unless `Dependency-Urgent:` cites an advisory (the root `nixpkgs` locked to a `NixOS/nixpkgs` `nixos-*` channel is exempt, so nightly security inputs land) | `north dep-gate` |
| No new structural debt in a repo with structure.toml | `code-health gate` |
| The repository's pre-push checks pass on the rebased lane | pre-push hook and landing train |
