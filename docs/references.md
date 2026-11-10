# References

Outside sources that shaped North or its fleet, what each informed and what came of it (north#40). An entry
needs a record: a link, commit, issue or file that cites the source. A source with no record gets no entry, and
an idea whose origin is unrecorded says "origin not recorded".

Each entry keeps a stable anchor id that provenance records can link to (north#40).

Outcomes: adopted, tested with no gain, rejected, or prior only (informs a choice, no test of its own). Current
values live where their consumers read them: thresholds and priors in
[harness/defaults.toml](../harness/defaults.toml), tiers in the
[workers skill](../harness/skills/workers/SKILL.md), prices and benchmark scores in
[harness/model-catalog.toml](../harness/model-catalog.toml).

A worker that adopts or tests an outside source adds its entry here in the same commit
([external-code](../harness/skills/external-code/SKILL.md)).

## Sources

### Agent harnesses and model choice

- <a id="harness-design-study"></a>**An Empirical Study of Harness Design for Coding Agents** (Fan et al., arXiv 2609.20804),
  https://arxiv.org/abs/2609.20804, accessed 2026-10-12.
  Informed: [references/2609.20804-harness-design.md](references/2609.20804-harness-design.md), commit fbcc15bd.
  Outcome: rejected for a recall tool, a trajectory judge and an over-verification hook, because the strong-model
  results showed no gain from them and Codex app-server owns the varied parts.

- <a id="harness-learning"></a>**Harness Learning Enables Generalizable Test-Time Adaptation** (Zhang et al., arXiv 2609.35738),
  https://arxiv.org/abs/2609.35738, accessed 2026-10-12.
  Informed: the held-out confirmation set item on
  [north#38](https://github.com/tompassarelli/north/issues/38#issuecomment-6123155497), the review-stage evidence
  and contribution items on [north#26](https://github.com/tompassarelli/north/issues/26#issuecomment-6123158084),
  and the change-manifest item on
  [north#40](https://github.com/tompassarelli/north/issues/40#issuecomment-6123158494).
  Outcome: prior only. Its proposer and solver are 4B to 8B models on Reasoning Gym and multi-hop QA, and the
  feedback includes gold answers. A hand-written tool loop (0.69) beat the mean trained revision (0.62) on 21
  unseen families (Table 9), and an automatic harness reviser is not adopted.

- <a id="claude-code-advisor"></a>**Claude Code advisor documentation**, https://code.claude.com/docs/en/advisor, accessed 2026-10-12, with the
  "Opus advises, Sonnet builds, Haiku swarms" tip it was read against.
  Informed: north#36, the replay arms in commits f471ef40, 784e1b90 and 8445503c, and north#38 for the arms
  left undecided.
  Outcome: tested with no gain for adding an advisor to the cheapest tier; the replay found no durable
  improvement, and the advisor reads the full transcript on every call with no cache reuse.

- <a id="artificial-analysis"></a>**Artificial Analysis, Claude Sonnet 5.5 vs Claude Opus 5.5 comparison**,
  https://artificialanalysis.ai/models/releases/comparisons/claude-sonnet-5-5-vs-claude-opus-5-5, and
  **Artificial Analysis performance benchmarking methodology**,
  https://artificialanalysis.ai/methodology/performance-benchmarking; cited by Tom, accessed 2026-10-12.
  Informed: north#38's middle-band tier question and its experiment choice
  ([study comment](https://github.com/tompassarelli/north/issues/38#issuecomment-6122255026)).
  Outcome: prior only; a third-party benchmark of the models, not a replay of our tasks, so the north#38 arms
  decide.

- <a id="routellm"></a>**RouteLLM** (Ong et al. 2024, arXiv 2406.18665; lm-sys/RouteLLM, Apache-2.0), https://arxiv.org/abs/2406.18665
  and https://github.com/lm-sys/RouteLLM; accessed: not recorded, cited 2026-10-12.
  Informed: north#35 box 6, commit 37017a39, the
  [learned routing backtest](rationale/learned-routing-and-goal-eta.md#learned-routing-backtest).
  Outcome: tested with no gain for the matrix-factorisation arm, reimplemented with no code copied: no saving
  interval excluded zero. Its BERT recipe was rejected, because no brief text exists at decision time.

- <a id="lightgbm"></a>**LightGBM**, https://github.com/microsoft/LightGBM; accessed: not recorded, cited 2026-10-12.
  Informed: north#35 box 6, [learned routing backtest](rationale/learned-routing-and-goal-eta.md#learned-routing-backtest).
  Outcome: rejected; training and walk-forward accuracy sat close together, so features and data, not model
  capacity, limit the router.

- <a id="arch-router"></a>**Arch-Router-1.5B**, link not recorded; cited 2026-10-12.
  Informed: north#35 box 6, [learned routing backtest](rationale/learned-routing-and-goal-eta.md#learned-routing-backtest).
  Outcome: rejected; it routes on prompt text, and the ledger holds no brief text at decision time.

- <a id="pstack"></a>**pstack** (Lauren Tan; `cursor/plugins` under `pstack/`, MIT), https://github.com/cursor/plugins/tree/main/pstack,
  read at `e5136d8` on 2026-10-15, at `519d36a` on 2026-10-27, at `fd268b8` on 2026-11-04 and at `1e9c3f4` on 2026-11-10. A followed source
  ([watching](../harness/skills/agent-policy/references/watching.md)).
  Informed: [rationale/pstack.md](rationale/pstack.md) for the 2026-10-15 decision, which adopted artifact-bound verdicts, a verification driver (`verify_north`), the
  correction ladder, gates with defaults, standing orders and the flat org; the 2026-10-27 review
  [rationale/watch/2026-10-27-pstack.md](rationale/watch/2026-10-27-pstack.md), north#262, north#263 and north#264; the 2026-11-04 review
  [rationale/watch/2026-11-04-pstack.md](rationale/watch/2026-11-04-pstack.md) and north#524; the 2026-11-10 review of its formal-verification and statechart skills
  [rationale/watch/2026-11-10-pstack-verification.md](rationale/watch/2026-11-10-pstack-verification.md) and north#1042.
  Outcome: adopted as ideas, no code copied; the two 2026-10-15 reviews are archived in south `agents/decisions/harness/`; of the 2026-10-27 mechanisms the dispatch order check and the citation check are live and the sharpen A/B has not run; the 2026-11-04 run-proven findings, able-to-fail properties and no-verifier-for-wording mechanisms are under test; its formal-verification and statechart skills are answered by `north check machines` (2026-11-10), which checks each lifecycle machine's own table over every ordering at its landing and generates its chart, so a review of those skills starts from that record's last section.

- <a id="mattpocock-skills"></a>**mattpocock/skills** (Matt Pocock, MIT), https://github.com/mattpocock/skills, read at
  `2ee14df` on 2026-07-10 and at `4ae2317` on 2026-11-04. A followed source
  ([watching](../harness/skills/agent-policy/references/watching.md)).
  Informed: the 2026-07-10 authoring norms (done-bars, leading words, the information hierarchy) in `684e72bfc`,
  credited in [docs/rationale/skill-provenance.md](rationale/skill-provenance.md); the 2026-11-04 review
  [rationale/watch/2026-11-04-mattpocock-skills.md](rationale/watch/2026-11-04-mattpocock-skills.md) and north#525.
  Outcome: adopted as ideas, no code copied. Done-bars are live in code and the information hierarchy as practice;
  leading words, the no-op test and positive phrasing were cut on 2026-08-02. Of the 2026-11-04 mechanisms the
  sampled retro was tested and kept (8 recurring failure modes with no enforcer in a one-in-five sample), and the
  Spec axis and the no-op test by replay are under test.

- <a id="ponytail"></a>**Ponytail** (Dietrich Gebert, MIT), https://github.com/DietrichGebert/ponytail, read at
  `96608e0` on 2026-11-06. A followed source ([watching](../harness/skills/agent-policy/references/watching.md)).
  Informed: [rationale/watch/2026-11-06-screen.md](rationale/watch/2026-11-06-screen.md) (north#636).
  Outcome: followed for its agentic arm benchmark of a code-writing prompt against no skill and a seven-word
  control; nothing taken yet.

- <a id="impeccable"></a>**Impeccable** (Paul Bakaus, Apache-2.0), https://github.com/pbakaus/impeccable, read at
  `c3bb418` on 2026-11-06. A followed source ([watching](../harness/skills/agent-policy/references/watching.md)).
  Informed: [rationale/watch/2026-11-06-screen.md](rationale/watch/2026-11-06-screen.md) (north#636).
  Outcome: followed for the Mak'gora client UI and the Wisp and Mak'gora pages (deterministic slop rules,
  ranked critique and audit); nothing taken yet.

- <a id="superpowers"></a>**Superpowers** (Jesse Vincent, MIT), https://github.com/obra/superpowers, read at
  `22f0b46` on 2026-11-06. A followed source ([watching](../harness/skills/agent-policy/references/watching.md)).
  Informed: [rationale/watch/2026-11-06-screen.md](rationale/watch/2026-11-06-screen.md) (north#636).
  Outcome: followed for its RED-then-GREEN skill pressure tests and the subagent fix loop; nothing taken yet.

- <a id="addyosmani-agent-skills"></a>**agent-skills** (Addy Osmani, MIT), https://github.com/addyosmani/agent-skills,
  read at `301b02b` on 2026-11-06. A followed source ([watching](../harness/skills/agent-policy/references/watching.md)).
  Informed: [rationale/watch/2026-11-06-screen.md](rationale/watch/2026-11-06-screen.md) (north#636).
  Outcome: followed for its fire and stay-quiet skill evals and rejected-change ledger; nothing taken yet.

- <a id="t3code"></a>**T3 Code** (T3 Tools Inc., Theo Browne and contributors, MIT), https://github.com/pingdotgg/t3code, read at
  `32a0531` on 2026-11-07 and at `5e59ece720` on 2026-11-10. A followed source ([watching](../harness/skills/agent-policy/references/watching.md)).
  Informed: [rationale/watch/2026-11-07-t3code.md](rationale/watch/2026-11-07-t3code.md), north#714, north#715 and north#716; the second read, [rationale/watch/2026-11-08-t3code-gemini.md](rationale/watch/2026-11-08-t3code-gemini.md), checked Gemini's review against main and led to north#862 to north#866; the second cycle, [rationale/watch/2026-11-10-t3code.md](rationale/watch/2026-11-10-t3code.md), led to north#1051.
  Outcome: adopted as ideas, no code copied. Four mechanisms were kept:
  - behavior evals of harness text built from real failures (north#714);
  - stay-quiet controls and a measured false-refusal rate for refusing guards (north#715), after 24 of 30 sampled
    launch-critical denials proved false;
  - tests that read time and host state only through fixtures (north#716);
  - stores opened as an older build wrote them before a schema change lands, and in the runtime canary (north#1051),
    after a graph schema change stopped every host's read-only graph readers for 82 minutes on 2026-11-08.
  The context byte budget, the event outbox, the attention phase, the release path and the worktree checkout cache fell
  below the bar; the outbox was taken later (north#862).

- <a id="claude-code-compaction"></a>**Claude Code 2.1.318 auto-compaction and prompt-cache pricing**: the threshold code in the installed
  `claude` binary (window less a 20,000-token output reserve less a 13,000-token buffer, `CLAUDE_CODE_AUTO_COMPACT_WINDOW`
  over the `autoCompactWindow` setting, `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE` taken against the window less the reserve),
  read 2026-11-06, and https://platform.claude.com/docs/en/about-claude/pricing for cache reads at 0.05x input on
  Opus 5.5 and 0.1x on Sonnet 5.5 and 1-hour cache writes at 2x. Informed:
  [rationale/context-economics.md](rationale/context-economics.md) (north#636).

- <a id="claude-plan-terms"></a>**Anthropic's published Claude plan terms** ("What is the Ultra plan?",
  https://support.claude.com/en/articles/17728952, and "What is the Max plan?",
  https://support.claude.com/en/articles/11049741-what-is-the-max-plan, both read 2026-11-08): Ultra is $200 a month with
  "10 times the Pro plan's usage every week" and is the former Max 20x renamed; Max is $100 a month with five times
  Pro's usage "every five hours and every week". Ultra publishes no five-hour multiple (verified accounts have no
  five-hour limit, unverified ones a temporary limit of unstated size), so the catalog's `ultra-10x` five-hour
  multiplier keeps 20, the figure the same plan carried as Max 20x, and its weekly multiplier is the published 10
  against Max 5x's 5. Outcome: adopted. Informed: `[accounts.claude].plans` in
  [harness/model-catalog.toml](../harness/model-catalog.toml), which placement and spend sizing read, and the plan label
  `north accounts` prints. Claude Code still reports the tier `default_claude_max_20x` for Ultra logins, so the catalog's
  `max-20x` entry carries `renamed_to = "ultra-10x"` and the usage table follows it (north#965).

- <a id="context-thread"></a>**An r/ClaudeCode thread on `autoCompactWindow`, percentage overrides and subagent cache TTLs**, with two
  model reviews of it, read by Tom on 2026-11-06 (no URL kept). An unverified source of hypotheses only: each claim was
  tested against the binary and north's own transcripts before use, and its 225k figure for 300k at 75% did not hold
  (the binary gives 210k). Informed: [rationale/context-economics.md](rationale/context-economics.md) (north#636).

- <a id="claudemd-lines-thread"></a>**An r/ClaudeCode thread asking for the one CLAUDE.md line that changed an agent's behavior most**,
  with two model reviews of it (OpenAI and Gemini), read on 2026-11-06 from a copy (no URL kept). An unverified source
  of ideas only: every claim in it is an anecdote, and each idea was checked against north's tree, ledger and incidents
  before a verdict. Informed: [rationale/watch/2026-11-06-claudemd-lines.md](rationale/watch/2026-11-06-claudemd-lines.md)
  (north#636).

- <a id="stale-instruction-files"></a>**Stale Instruction Files Override the Code in Coding Agents: A One-Sentence Fix** (Xu and Wu,
  arXiv 2610.30571, v1 2026-10-22), https://arxiv.org/abs/2610.30571, abstract read 2026-11-06. Over 40 synthetic
  Python projects and 320 runs, Claude Code noticed the stale instruction file in 79 of 80 runs and followed it anyway;
  one sentence giving the code precedence moved code-following choices from 4.2% to 91.4%. Informed:
  [rationale/watch/2026-11-06-claudemd-lines.md](rationale/watch/2026-11-06-claudemd-lines.md) (north#636).
  Outcome: the report of each conflict adopted in the fleet prompt; the code-wins precedence rejected, because north's
  policy owners and issue plans outrank code under migration.

### Delivery and harness benchmarks

- <a id="google-tap"></a>**Software Engineering at Google, chapter 23: Continuous Integration** (Winters, Manshreck and Wright, O'Reilly 2020),
  https://abseil.io/resources/swe-book/html/ch23.html, accessed 2026-11-06.
  Informed: [rationale/scoreboard.md](rationale/scoreboard.md) (north#657): TAP's average submit wait of about 11 minutes and a 95%+ pass rate after presubmit.
  Outcome: prior only.

- <a id="uber-submitqueue"></a>**Slashing CI costs at Uber** (Uber Engineering, 2025-06-23),
  https://www.uber.com/blog/slashing-ci-costs-at-uber/, accessed 2026-11-06.
  Informed: [rationale/scoreboard.md](rationale/scoreboard.md) (north#657): SubmitQueue P95 waits of 18.64 (Go), 9.91 (iOS) and 17.33 (Android) minutes.
  Outcome: prior only.

- <a id="github-merge-queue"></a>**How GitHub uses merge queue to ship hundreds of changes every day** (GitHub Blog, 2024-03-06),
  https://github.blog/engineering/engineering-principles/how-github-uses-merge-queue-to-ship-hundreds-of-changes-every-day/, accessed 2026-11-06.
  Informed: [rationale/scoreboard.md](rationale/scoreboard.md) (north#657): a 33% cut in average wait and 2,500 pull requests a month.
  Outcome: prior only.

- <a id="google-code-review"></a>**Modern Code Review: A Case Study at Google** (Sadowski et al., ICSE-SEIP 2018),
  https://sback.it/publications/icse2018seip.pdf, accessed 2026-11-06.
  Informed: [rationale/scoreboard.md](rationale/scoreboard.md) (north#657): median review latency under 4 hours over about 9 million changes.
  Outcome: prior only.

- <a id="dora-2024"></a>**2024 DORA performance clusters** (Octopus Deploy's summary of the 2024 Accelerate State of DevOps report),
  https://octopus.com/blog/2024-devops-performance-clusters, accessed 2026-11-06; the report itself is at https://dora.dev/research/2024/dora-report/.
  Informed: [rationale/scoreboard.md](rationale/scoreboard.md) (north#657): elite lead time under 24 hours and change failure rate 5%, high 20%.
  Outcome: prior only.

- <a id="anthropic-work"></a>**How AI is transforming work at Anthropic** (Anthropic, 2025-12-02),
  https://www.anthropic.com/research/how-ai-is-transforming-work-at-anthropic, accessed 2026-11-06.
  Informed: [rationale/scoreboard.md](rationale/scoreboard.md) (north#657): human turns per Claude Code transcript, 6.2 to 4.1 from February to August 2025.
  Outcome: prior only.

- <a id="metr-horizon"></a>**Measuring AI Ability to Complete Long Tasks** (Kwa et al., METR, arXiv 2503.14499),
  https://arxiv.org/abs/2503.14499, accessed 2026-11-06.
  Informed: [rationale/scoreboard.md](rationale/scoreboard.md) (north#657): a 50% time horizon of about 50 minutes for Claude 3.7 Sonnet, doubling about every seven months.
  Outcome: prior only.

### Statistics

- <a id="wilson-interval"></a>**Wilson score interval** (E. B. Wilson, "Probable Inference, the Law of Succession, and Statistical
  Inference", JASA, 1927), https://doi.org/10.1080/01621459.1927.10502953; accessed: not recorded, cited
  2026-10-12.
  Informed: [crates/north-convo/src/policy_replay/stats.rs](../crates/north-convo/src/policy_replay/stats.rs), the sweep's operating point
  and the policy gate (10138bba, 012a7f3f), `recommend`'s no-pass tie-break (22d8c8ea), per-arm reports
  (d32f9266, 309eda0c), north#35 and north#36.
  Outcome: adopted; it keeps a sensible interval at small n and at rates of 0 or 1, where a normal standard
  error collapses to zero.

- <a id="paired-bootstrap"></a>**Paired bootstrap** (B. Efron, "Bootstrap Methods: Another Look at the Jackknife", Annals of Statistics,
  1979), https://doi.org/10.1214/aos/1176344552; accessed: not recorded, cited 2026-10-12.
  Informed: the task-cluster paired difference in
  [stats.rs](../crates/north-convo/src/policy_replay/stats.rs) (309eda0c, north#36), the routing
  backtest's differences (37017a39, [learned routing backtest](rationale/learned-routing-and-goal-eta.md#learned-routing-backtest)), and
  calibration sample-size floors (b6462af5).
  Outcome: adopted; arms run the same tasks, so pairing by task removes task difficulty from the difference.

- <a id="fisher-exact"></a>**Fisher's exact test** (R. A. Fisher, "On the Interpretation of χ² from Contingency Tables, and the
  Calculation of P", JRSS, 1922), https://doi.org/10.2307/2340521; accessed: not recorded, cited 2026-10-12.
  Informed: the one-sided drop and gain test in the policy gate and sweep selection (82fd8604,
  [stats.rs](../crates/north-convo/src/policy_replay/stats.rs)).
  Outcome: adopted; it is exact at the few trials per scenario a replay runs.

### Thresholds with a named outside source

- <a id="fowler-ci"></a>**Martin Fowler, Continuous Integration** (the ten-minute build),
  https://martinfowler.com/articles/continuousIntegration.html; accessed: not recorded, cited 2026-10-12.
  Informed historically: the retired Galileo full-suite wall budget at
  [635486a5](https://github.com/tompassarelli/north/blob/635486a5/harness/lib/test-health-core.ts) and the harness.yml landing
  budget (2c742813).
  Outcome: adopted; a published build-time ceiling, where the repository had no measured one.

- <a id="agent-skills-spec"></a>**Agent Skills specification**, https://agentskills.io/specification; accessed: not recorded, cited 2026-10-12.
  Informed: the playbook SKILL.md body ceiling in
  [crates/north-core/src/agent_config_check/gate.rs](../crates/north-core/src/agent_config_check/gate.rs) (0aa8f9fa, first in agent-config-check.sh).
  Outcome: adopted as the failing ceiling, with the tighter local size kept as a reported target that fails
  nothing.

- <a id="psi-notify"></a>**psi-notify(1) manual page**, https://manpages.debian.org/bookworm/psi-notify/psi-notify.1.en.html; accessed:
  not recorded, cited 2026-10-12.
  Informed: the capacity watchdog's CPU pressure flag (bb9f64bd).
  Outcome: adopted; its default cut flag episodes against the earlier local value.

- <a id="kernel-psi"></a>**Linux kernel documentation, PSI (Pressure Stall Information)**, https://docs.kernel.org/accounting/psi.html; accessed: not recorded, cited 2026-10-31.
  Informed: the heavy-build pressure target and its settle time in [pressure_target.rs](../crates/north-capacity/src/capacity/pressure_target.rs), `admit_psi_*` and
  `admit_settle_seconds` in [harness/defaults.toml](../harness/defaults.toml): "some" is the share of time at least one task waited for a CPU,
  averaged over 10, 60 and 300 s, which is why a controller must wait a window after each start before it reads again.
  Outcome: adopted; the marks come from the laptop's own samples, the kernel text only fixes what the number means.

### Runtime and code

- <a id="codex-app-server"></a>**Codex app-server documentation**, https://learn.chatgpt.com/docs/app-server, and the Codex source at the
  revision `61a44880a85d2fd0d8770908dea5733495e571c8`; accessed: not recorded.
  Informed: [architecture.md](architecture.md), North's process and TUI split and its stdio JSONL transport.
  Outcome: adopted; Codex app-server owns provider communication, so North needs no provider SDK or second
  agent loop.

- <a id="dirge"></a>**DIRGE**, at revision `37785707f3dd3543f76dc5125fd8f331ca1c0813`, GPL-3.0-only; link
  not recorded.
  Informed: [architecture.md](architecture.md)'s adopt-after list: a central deterministic intervention arbiter,
  at most one intervention at a safe boundary, traces of selected and declined actions.
  Outcome: prior only; conceptual prior art, with no implementation copied or adapted because of its licence.

- <a id="untangle-codebase"></a>**caldoai/skills untangle-codebase**,
  https://github.com/caldoai/skills/blob/442c29918339cc8dbf09ae27aaf17b05745465a3/skills/untangle-codebase/SKILL.md,
  MIT (Copyright (c) 2026 Maze, LICENSE at that commit); accessed 2026-11-06, with a rewrite Claude made for Tom the
  same day. Its claims about coupling and agent cost are unverified here.
  Informed: [architectural-drift.md](rationale/architectural-drift.md), the `code-health budget` coupling signals, the
  review brief's method and the change-together section of program-craftsmanship.
  Outcome: ideas adopted in north's own words and code; no text copied, and the co-change script ported to Rust.

- <a id="pi-opencode-releases"></a>**Pi (earendil-works/pi, formerly badlogic/pi-mono) and OpenCode (anomalyco/opencode) release builds**,
  https://github.com/earendil-works/pi/releases and https://github.com/anomalyco/opencode/releases, both MIT (LICENSE
  in each repository; GitHub reports `MIT` for both); accessed 2026-11-09, Pi 1.5.0 (`pi-linux-x64.tar.gz`) and OpenCode
  2.2.6 (`opencode-linux-x64.tar.gz`).
  Informed: [vendor_runtime](../crates/north-ops/src/ops/vendor_runtime/mod.rs), which downloads each program's newest
  stable release asset, checks it against the SHA-256 GitHub records for that asset and installs it under
  `~/.local/lib/pi` and `~/.local/lib/opencode`; north#1023.
  Outcome: adopted as unmodified upstream binaries run as separate programs; nothing is copied into North or
  redistributed, so the MIT notice stays with each upstream archive and repository.

- <a id="threejs-skills"></a>**CloudAI-X/threejs-skills**,
  https://github.com/CloudAI-X/threejs-skills/blob/b1c623076c661fc9b03dac19292e825a5d106823/README.md; accessed:
  not recorded.
  Informed: [harness/skills/threejs/](../harness/skills/threejs/),
  [docs/rationale/skill-provenance.md](rationale/skill-provenance.md).
  Outcome: adopted under its stated MIT terms, with corrections and rationale added.

### Nix

<a id="nix-practice"></a>Each source informed [rationale/nix-current-practice.md](rationale/nix-current-practice.md)
(north#553), accessed 2026-11-04; outcome for all: prior only, a dated audit that sets the targets of the Nix
footguns plan and the nix-development skill's regrounding.

- <a id="nix-endoflife"></a>**endoflife.date, Nix**, https://endoflife.date/api/nix.json: release lines, end-of-life
  dates and latest patches.
- <a id="nix-rl-2-35"></a>**Nix 2.35 release notes**, https://nix.dev/manual/nix/latest/release-notes/rl-2.35.
- <a id="nix-rl-2-34"></a>**Nix 2.34 release notes**, https://nix.dev/manual/nix/latest/release-notes/rl-2.34.
- <a id="nix-rl-2-33"></a>**Nix 2.33 release notes**, https://nix.dev/manual/nix/latest/release-notes/rl-2.33.
- <a id="nix-rl-2-32"></a>**Nix 2.32 release notes**, https://nix.dev/manual/nix/latest/release-notes/rl-2.32.
- <a id="nix-release-notes"></a>**Nix release notes index and support policy**,
  https://nix.dev/manual/nix/latest/release-notes/.
- <a id="nixos-2605"></a>**NixOS 26.05 announcement**, https://nixos.org/blog/announcements/2026/nixos-2605/.
- <a id="nixos-release-notes"></a>**NixOS release notes (26.05, 25.11)**,
  https://nixos.org/manual/nixos/stable/release-notes. Followed at `nixos/doc/manual/release-notes` in
  https://github.com/NixOS/nixpkgs (MIT), monthly. Informed: the 2026-11-10 review
  [rationale/watch/2026-11-10-nixos-release-notes.md](rationale/watch/2026-11-10-nixos-release-notes.md), which made
  watch-review count content commits and let a source come due monthly.
- <a id="nix-experimental-features"></a>**Nix experimental features**,
  https://nix.dev/manual/nix/latest/development/experimental-features.
- <a id="nix-cli-stabilization"></a>**NixOS/nix#7701, CLI stabilization effort**,
  https://github.com/NixOS/nix/issues/7701.
- <a id="rfc-136"></a>**RFC 136, stabilize incrementally**,
  https://github.com/NixOS/rfcs/blob/master/rfcs/0136-stabilize-incrementally.md.
- <a id="lwn-flakes-2026"></a>**LWN, Understanding Nix flakes** (2026-10-21), https://lwn.net/Articles/1094226/.
- <a id="nix-dev"></a>**nix.dev home**, https://nix.dev/.
- <a id="nix-dev-flakes"></a>**nix.dev, Flakes concept**, https://nix.dev/concepts/flakes.
- <a id="nix-dev-direnv"></a>**nix.dev, Automatic environments (direnv)**, https://nix.dev/guides/recipes/direnv.
- <a id="nix-env-manual"></a>**Nix manual, nix-env**, https://nix.dev/manual/nix/latest/command-ref/nix-env.
- <a id="nix-fmt-manual"></a>**Nix manual, nix fmt**, https://nix.dev/manual/nix/latest/command-ref/new-cli/nix3-fmt.
- <a id="nix-flake-manual"></a>**Nix manual, nix flake**,
  https://nix.dev/manual/nix/latest/command-ref/new-cli/nix3-flake.
- <a id="nix-conf-manual"></a>**Nix manual, nix.conf**, https://nix.dev/manual/nix/latest/command-ref/conf-file.
- <a id="nix-direnv-releases"></a>**nix-direnv releases**, https://github.com/nix-community/nix-direnv/releases.
- <a id="install-nix-action-releases"></a>**cachix/install-nix-action releases**,
  https://github.com/cachix/install-nix-action/releases.

<a id="nix-leverage-sources"></a>Each source below informed [rationale/nix-leverage.md](rationale/nix-leverage.md)
(north#968), accessed 2026-11-09.

- <a id="ghuntley-nix"></a>**Geoffrey Huntley, "fix everything with a Nix overlay"** (2026-10-07),
  https://ghuntley.com/nix/. Outcome: rejected as a whole; its force-push idea adopted as a server-side ruleset
  (north#973), its single-expression and overlay claims already in use.
- <a id="ghuntley-nix-demo"></a>**ghuntley/nix-demo**, https://github.com/ghuntley/nix-demo. The post's companion
  repository; no license file, so nothing was copied. Outcome: rejected.
- <a id="docker-sandboxes"></a>**Docker Sandboxes**, https://docs.docker.com/ai/sandboxes/ and
  https://docs.docker.com/ai/sandboxes/architecture/. Outcome: rejected; its credential proxy kept as a proposal.
- <a id="microvm-nix"></a>**microvm.nix**, https://github.com/microvm-nix/microvm.nix. Outcome: rejected.
- <a id="nixos-containers"></a>**NixOS manual, containers**, https://nixos.org/manual/nixos/stable/#ch-containers.
  Outcome: rejected.
- <a id="bun-lifecycle"></a>**bun, lifecycle scripts**, https://bun.sh/docs/install/lifecycle. Outcome: prior only,
  sizing south#93.

<a id="agent-user-sources"></a>Each source below informed [rationale/agent-user.md](rationale/agent-user.md)
(north#977), accessed 2026-11-09.

- <a id="gvisor"></a>**gVisor, runsc** (release 20260406), https://gvisor.dev/docs/. Measured under depbox's workload:
  about 40% slower cold builds and 50% slower tests. Outcome: rejected.
- <a id="github-installation-token"></a>**GitHub REST API, create an installation access token for an app**,
  https://docs.github.com/en/rest/apps/apps#create-an-installation-access-token-for-an-app. A token may be limited
  to a subset of repositories and permissions and lives one hour. Outcome: adopted for the credential broker (south#94).
- <a id="github-rulesets"></a>**GitHub, available rules for rulesets**,
  https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/available-rules-for-rulesets.
  Outcome: adopted (north#973, south#95).
- <a id="cargo-deny"></a>**cargo-deny**, https://github.com/EmbarkStudios/cargo-deny (MIT OR Apache-2.0). Outcome:
  adopted as the landing audit gate (south#96).
- <a id="cargo-vet"></a>**cargo-vet**, https://mozilla.github.io/cargo-vet/. Outcome: measured and not adopted: published
  audits cover 68 of north's 265 locked crates (south#96, docs/rationale/agent-user.md).
- <a id="systemd-exec"></a>**systemd.exec, DynamicUser and credentials**,
  https://www.freedesktop.org/software/systemd/man/latest/systemd.exec.html. Outcome: credentials adopted for the
  broker; a dynamic user per session rejected.

### Scheduling and capacity

Sources behind [docs/rationale/capacity-scheduler.md](rationale/capacity-scheduler.md#prior-art), which states the
claim taken from each; all accessed 2026-11-04. PSI is [kernel-psi](#kernel-psi) above.

- <a id="borg"></a>**Verma et al., Large-scale cluster management at Google with Borg (EuroSys 2015)**,
  https://static.googleusercontent.com/media/research.google.com/en//pubs/archive/43438.pdf.
  Informed: admission on measured use with reclamation for batch, and classes that never rely on reclaimed capacity.
  Outcome: prior only.
- <a id="autopilot"></a>**Rzadca et al., Autopilot: workload autoscaling at Google Scale (EuroSys 2020)**,
  https://research.google/pubs/autopilot-workload-autoscaling-at-google-scale/.
  Informed: MemoryHigh from a learned quantile of use. Outcome: prior only.
- <a id="slurm-backfill"></a>**Slurm scheduling configuration guide (backfill)**, https://slurm.schedmd.com/sched_config.html;
  EASY origin: Lifka, The ANL/IBM SP scheduling system (JSSPP 1995),
  https://link.springer.com/chapter/10.1007/3-540-60153-8_35.
  Informed: the reservation backfill box. Outcome: prior only.
- <a id="tsafrir-backfill"></a>**Tsafrir, Etsion and Feitelson, Backfilling using system-generated predictions rather
  than user runtime estimates (IEEE TPDS 2007)**, abstract at https://manuscript.isc.ac/Inventory/49/827041.htm.
  Informed: predicted ends drive the reservation, overruns correct the estimate. Outcome: prior only.
- <a id="heft"></a>**Topcuoglu, Hariri and Wu, Performance-effective and low-complexity task scheduling for
  heterogeneous computing (IEEE TPDS 2002)**,
  https://experts.arizona.edu/en/publications/performance-effective-and-low-complexity-task-scheduling-for-hete/.
  Informed: placement by earliest predicted finish time. Outcome: prior only.
- <a id="senpai-tmo"></a>**senpai**, https://github.com/facebookincubator/senpai, and **Weiner et al., TMO:
  Transparent Memory Offloading in Datacenters (ASPLOS 2022)**, https://par.nsf.gov/servlets/purl/10332477.
  Informed: memory need learned from pressure, not from a limit-capped peak. Outcome: prior only.
- <a id="cgroup-v2-cpu"></a>**Linux kernel cgroup v2 documentation** and **systemd.resource-control(5)**,
  https://docs.kernel.org/admin-guide/cgroup-v2.html,
  https://www.freedesktop.org/software/systemd/man/latest/systemd.resource-control.html.
  Informed: the elastic-cpu shape (cpu.weight, cpu.idle via `CPUWeight=idle`, memory.high, cgroup.freeze) and its
  sibling-relative idle correction. Outcome: prior only.
- <a id="bazel-remote-cache"></a>**Bazel remote caching** and the **Remote Execution API**,
  https://bazel.build/remote/caching, https://github.com/bazelbuild/remote-apis.
  Informed: land-check verdicts keyed by input tree and tool identity. Outcome: prior only.
- <a id="buck2"></a>**Buck2: why Buck2** and **remote execution**, https://buck2.build/docs/about/why/,
  https://buck2.build/docs/users/remote_execution/.
  Informed: one placement function for local and remote hosts. Outcome: prior only.
- <a id="buck2-dep-files"></a>**Buck2: dep files**, https://buck2.build/docs/rule_authors/dep_files/.
  Informed: a verdict keyed on the inputs a check used, not every input it may read
  ([verdict-reuse.md](rationale/verdict-reuse.md)). Outcome: prior only.
- <a id="buildxl"></a>**BuildXL** (Microsoft Build Accelerator), https://github.com/microsoft/BuildXL.
  Informed: the two-phase lookup of verdict reuse, a weak fingerprint selecting observed path sets and a strong
  fingerprint over their current content, absent paths and listings included
  ([verdict-reuse.md](rationale/verdict-reuse.md)). Outcome: prior only.
- <a id="tup"></a>**tup** and **Shal, Build System Rules and Algorithms (2009)**, https://gittup.org/tup/,
  https://gittup.org/tup/build_system_rules_and_algorithms.pdf.
  Informed: recorded file accesses per command, a probed missing file as an input, FUSE as the fallback
  recorder ([verdict-reuse.md](rationale/verdict-reuse.md)). Outcome: prior only.
- <a id="build2"></a>**build2 build system manual**, https://build2.org/build2/doc/build2-build-system-manual.xhtml.
  Informed: a compile's inputs taken from the compiler's own dependency output (rustc dep-info)
  ([verdict-reuse.md](rationale/verdict-reuse.md)). Outcome: prior only.
- <a id="ekstazi"></a>**Gligoric, Eloussi and Marinov, Practical regression test selection with dynamic file
  dependencies (ISSTA 2015)**, https://doi.org/10.1145/2771783.2771784.
  Informed: per-test file dependencies as a safe and cheap reuse key ([verdict-reuse.md](rationale/verdict-reuse.md)), and
  running every test with no recorded dependencies ([test-selection.md](rationale/test-selection.md)).
  Outcome: prior only.
- <a id="tap-scale"></a>**Memon, Gao, Nguyen, Dhanda, Nickell, Siemborski and Micco, Taming Google-scale continuous
  testing (ICSE-SEIP 2017)**, https://doi.org/10.1109/ICSE-SEIP.2017.16.
  Informed: reverse-dependency selection over the build graph as the sound fallback and the cost ceiling
  ([test-selection.md](rationale/test-selection.md), north#1039). Outcome: prior only.
- <a id="test-impact-analysis"></a>**Microsoft, Test Impact Analysis** (Azure Pipelines),
  https://learn.microsoft.com/en-us/azure/devops/pipelines/test/test-impact-analysis, and **Fowler, The rise of test
  impact analysis**, https://martinfowler.com/articles/rise-test-impact-analysis.html, accessed 2026-11-10.
  Informed: a per-test coverage map as the proof that a test is unaffected, not as the list of tests to run
  ([test-selection.md](rationale/test-selection.md), north#1039). Outcome: prior only.
- <a id="predictive-test-selection"></a>**Machalica, Samylkin, Porth and Chandra, Predictive test selection
  (ICSE-SEIP 2019)**, https://arxiv.org/abs/1810.05286.
  Informed: learned selection rejected as a way to drop tests, since it trades misses for cost
  ([test-selection.md](rationale/test-selection.md), north#1039). Outcome: prior only.
- <a id="starts"></a>**Legunsen, Hariri, Shi, Lu, Zhang and Marinov, An extensive study of static regression test
  selection in modern software evolution (FSE 2016)**, https://doi.org/10.1145/2950290.2950361.
  Informed: static dependency selection rejected for tests that spawn the north binary
  ([test-selection.md](rationale/test-selection.md), north#1039). Outcome: prior only.
- <a id="nextest"></a>**cargo-nextest: how it works** and **target runners**, https://nexte.st/docs/design/how-it-works/,
  https://nexte.st/docs/features/target-runners/.
  Informed: one process per test and the runner hook that records each test's reads
  ([verdict-reuse.md](rationale/verdict-reuse.md)). Outcome: prior only.
- <a id="landlock"></a>**Landlock: unprivileged access control** (Linux kernel documentation),
  https://docs.kernel.org/userspace-api/landlock.html.
  Informed: the read fence of the land sandbox ([verdict-reuse.md](rationale/verdict-reuse.md)). Outcome: prior only.
- <a id="seccomp-unotify"></a>**seccomp_unotify(2)**, https://man7.org/linux/man-pages/man2/seccomp_unotify.2.html.
  Informed: the land sandbox's recorder of every path a test's process tree looks up
  ([verdict-reuse.md](rationale/verdict-reuse.md)). Outcome: prior only.
- <a id="harmonia"></a>**Harmonia** (README, NixOS module services.harmonia.cache), https://github.com/nix-community/harmonia.
  MIT, nix-community, packaged in the nixpkgs firn pins (3.1.0); serves a host's /nix/store as a signed binary
  cache with no upload step. Informed: the build host serves its store and every host substitutes from it.
  Outcome: adopted; see [propagation.md](rationale/propagation.md).
- <a id="sccache-capacity"></a>**sccache** (README and docs/Rust.md), https://github.com/mozilla/sccache.
  Informed: no laptop-shared compiler cache, per-host caches only. Outcome: prior only; see also
  [build-cost.md](rationale/build-cost.md).
- <a id="quasar"></a>**Delimitrou and Kozyrakis, Quasar: resource-efficient and QoS-aware cluster management
  (ASPLOS 2014)**, https://www.csl.cornell.edu/~delimitrou/papers/2014.asplos.quasar.pdf.
  Informed: sizing from measured behaviour with backoff to similar workloads. Outcome: prior only.
- <a id="claude-cloud-sessions"></a>**Claude Code: use Claude Code in the cloud**,
  https://code.claude.com/docs/en/claude-code-on-the-web, and **GitHub Actions billing**,
  https://docs.github.com/en/billing/concepts/product-billing/github-actions.
  Informed: the anthropic-cloud and Actions placement rows (cloud sessions draw plan usage with no VM charge;
  standard hosted runners are free in public repositories). Outcome: adopted as placement facts.

### Knowledge graphs and Datalog

Sources for the world-model plan ([docs/rationale/world-model.md](rationale/world-model.md), north#882), read 2026-11-08.

- <a id="beagle-store"></a>**Beagle Store**, `~/code/beagle/main/store`, MIT or Apache-2.0 (Tom's project), accessed 2026-11-08.
  Informed: north-graph's kernel, the text relations and the `as-of`/`since` views (the query reference in the store repository's docs directory).
  Outcome: adopted for semantics; north ports it to Rust over SQLite and runs none of its code.

- <a id="datafrog"></a>**datafrog**, https://github.com/rust-lang/datafrog, MIT or Apache-2.0, accessed 2026-11-08.
  Informed: in-memory derived relations as sorted vectors joined by leapjoin. Outcome: model for the evaluator's derived
  relations; not a dependency, since it has no quoted propositions or occurrence history.

- <a id="souffle"></a>**Soufflé**, https://souffle-lang.github.io/, UPL-1.0, accessed 2026-11-08.
  Informed: compiling a rule body to one specialized join. Outcome: the base-relation prefix of a rule pushes into one
  SQL join.

- <a id="cozodb"></a>**CozoDB**, https://github.com/cozodb/cozo, MPL-2.0, accessed 2026-11-08.
  Informed: Datalog with full-text search and `Validity` time travel in one embedded store. Outcome: confirms one store
  for text, history and rules; rejected as the engine because its tuples are flat, not recursive quoted propositions.

- <a id="rdf-star"></a>**RDF 1.2 triple terms (RDF-star)**, https://www.w3.org/TR/rdf12-concepts/, accessed 2026-11-08.
  Informed: a quoted triple is a term and is not asserted. Outcome: the same rule as north-graph's `quoted` relation,
  which the reward and box-close readers depend on.

- <a id="provenance-semirings"></a>**Provenance semirings** (Green, Karvounarakis, Tannen, PODS 2007),
  https://doi.org/10.1145/1265530.1265535, accessed 2026-11-08. Informed: why a derived row holds, as a function of the
  base rows that produced it. Outcome: the `corroborated` rule names the mechanical writer behind each verified box.

### Skill bibliographies

Skills that cite many sources keep them beside the skill, so they are not repeated here:

- [harness/skills/agent-policy/references/guides.md](../harness/skills/agent-policy/references/guides.md):
  instruction, skill and hook writing guides.
- [harness/skills/verification/references/testing-sources.md](../harness/skills/verification/references/testing-sources.md): testing
  practice.
- [harness/skills/github/references/actions-sources.md](../harness/skills/github/references/actions-sources.md):
  GitHub Actions and REST limits.
- [harness/skills/warcraft-modding/references/lua-performance-sources.md](../harness/skills/warcraft-modding/references/lua-performance-sources.md):
  Lua and TypeScriptToLua performance.
- [harness/skills/warcraft-modding/references/api-gotchas.md](../harness/skills/warcraft-modding/references/api-gotchas.md),
  [harness/skills/makgora/references/animation-library.md](../harness/skills/makgora/references/animation-library.md),
  and [harness/skills/repo-safety/references/project-layout.md](../harness/skills/repo-safety/references/project-layout.md):
  domain references.
- [harness/model-catalog.toml](../harness/model-catalog.toml): prices and benchmark scores the routing reads.

## Inspirations

<a id="inspiration-state-model"></a>**The historical Galileo state model.** Clause,
Galileo's archived predecessor, informed the earlier rule that a checked
language owns state transitions and the host performs authorized effects.
Galileo at `d1195b99f5cc298eb52b2e9751fe493bebf8a502` supplied North's original
transition reference. North now owns those transitions as typed Rust;
original captured replay bytes preserve that behavioral reference. The
retired source remains in Git history and private reference records.
North-v1 supplied the product contract and invariants, not code. The
intervention arbiter planned on top borrows its shape from DIRGE as prior art
only.

Dated harness fixtures, seeded routing and ownership scenarios, provenance
examples and recorded Rust test input strings retain Galileo names as
historical or synthetic reference data. They do not select the active
repository or require its executable, source checkout or build dependency.

<a id="inspiration-org-shape"></a>**The fleet org shape.** The harness's hooks, lib and agent commands were copied into north from firn (4645127a),
and the fleet tree, a proxy at the root with leads, sub-leads and workers under it, is now
represented by [Fleet session parent links](features/03-fleet-sessions/). The former org schema was retired
when its status bus callers moved to Fleet (d304e9c0). Where the shape and its names came from: origin
not recorded.

<a id="inspiration-replay-loop"></a>**The replay and calibration loop.** Policy replay runs a recorded scenario through an isolated session and
scores it with a detector (8bac5d16); the sweep picks the cheapest variant whose Wilson interval overlaps the
best one's (10138bba), and calibration scales predictions by measured actual over predicted (ac79788c). The
statistics come from the Wilson, Fisher and paired-bootstrap entries above, the routing backtest's second arm
from RouteLLM, and the walk-forward scoring and adopt-only-on-measured-gain rule from north#35 and north#36.
North#40 asks that every rule carry a source so others can repeat the evidence. Where the idea of replaying
recorded policy scenarios came from: origin not recorded.

### Self-improving agents

Sources for the policy climber ([docs/rationale/policy-climber.md](rationale/policy-climber.md), north#636), found 2026-11-06 by
web search. A figure marked (secondary) comes from a summary, not the paper.

- <a id="darwin-godel-machine"></a>**Darwin Gödel Machine** (Sakana AI), https://sakana.ai/dgm/ and
  https://iclr.cc/virtual/2026/poster/10007327, accessed 2026-11-06.
  Informed: the climber's archive of variants with parents sampled across it, and its guards: DGM faked test logs and removed the
  markers that detected it, so the evaluator and the ledger's counting code are outside a candidate's scope.
  Outcome: adopted in the design (archive, parent sampling, scope guard); its cost, about USD 22,000 for one SWE-bench run
  (secondary), is why north tries one candidate per loss per day.

- <a id="gepa"></a>**GEPA: Reflective Prompt Evolution Can Outperform Reinforcement Learning** (arXiv 2507.19457),
  https://arxiv.org/abs/2507.19457, accessed 2026-11-06.
  Informed: the generator that reads failed runs and keeps a Pareto set of candidates that win somewhere.
  Outcome: adopted in the design; the generator box builds it.

- <a id="alphaevolve"></a>**AlphaEvolve** (Google DeepMind; secondary summaries at
  https://venturebeat.com/ai/meet-alphaevolve-the-google-ai-that-writes-its-own-code-and-just-saved-millions-in-computing-costs),
  accessed 2026-11-06.
  Informed: the rule that the loop runs only where the metric is machine-checkable; replay and the ledger are that check.
  Outcome: prior only.

- <a id="adas"></a>**Automated Design of Agentic Systems** (arXiv 2408.08435), https://arxiv.org/abs/2408.08435, accessed 2026-11-06.
  Informed: a meta agent that writes a design, tests it and archives it; the paper does not discuss compute (secondary).
  Outcome: prior only.

- <a id="godel-agent"></a>**Gödel Agent** (arXiv 2410.04444), https://arxiv.org/html/2410.04444v4, accessed 2026-11-06.
  Informed: the caution that an agent rewriting its own improver is the least controlled design; its measured gains were not verified.
  Outcome: prior only; the scope guard exists to prevent it.

- <a id="live-swe-agent"></a>**Live-SWE-agent** (arXiv 2511.13646), https://arxiv.org/html/2511.13646v3, accessed 2026-11-06.
  Informed: a worker proposing a tool or brief fix at run time, recorded in the ledger.
  Outcome: prior only.

- <a id="voyager-reflexion"></a>**Voyager** (arXiv 2305.16291, https://arxiv.org/html/2305.16291) and **Reflexion**
  (secondary, https://blckalpaca.at/en/knowledge-base/ai-agents/agent-architectures-overview/reflexion-pattern-agents), accessed 2026-11-06.
  Informed: inject on a retry only lessons a later success on the same category confirmed; both depend on a reliable test signal.
  Outcome: prior only.

- <a id="swe-agent"></a>**SWE-agent** (arXiv 2405.15793), https://arxiv.org/pdf/2405.15793.pdf, accessed 2026-11-06.
  Informed: attempt loss cut by guards that reject an edit before it is submitted, a lever outside routing.
  Outcome: prior only.

- <a id="bandit-routing"></a>**Contextual-bandit routing** (Greedy LinUCB, AAAI-26,
  https://ojs.aaai.org/index.php/AAAI/article/view/39672/43633; NeuralUCB on RouterBench,
  https://www.alphaxiv.org/abs/2603.30035.md) and **FrugalGPT** (arXiv 2305.05176, https://arxiv.org/abs/2305.05176; primary numbers
  not confirmed), accessed 2026-11-06, beside [RouteLLM](#routellm).
  Informed: adaptive arm shares by Thompson sampling on cost per kept box, a verified outcome as the reward and cost as the penalty.
  Outcome: adopted in the design; the shares box builds it.

- <a id="landlock-crate"></a>**`landlock` crate 0.4.7** (https://github.com/landlock-lsm/rust-landlock, MIT OR Apache-2.0), accessed 2026-11-08.
  Informed: the safe Rust binding the land sandbox uses to restrict a child's file reads under `--enforce`, with no `unsafe` and no `pre_exec`.
  Outcome: adopted as a dependency of north-land-sandbox; nothing copied.
