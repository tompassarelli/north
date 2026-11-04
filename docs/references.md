# References

Outside sources that shaped North or Socrates, what each informed and what came of it (north#40). An entry
needs a record: a link, commit, issue or file that cites the source. A source with no record gets no entry, and
an idea whose origin is unrecorded says "origin not recorded".

Each entry keeps a stable anchor id that provenance records can link to (north#40).

Outcomes: adopted, tested with no gain, rejected, or prior only (informs a choice, no test of its own). Current
values live where their consumers read them: thresholds and priors in
[harness/defaults.toml](../harness/defaults.toml), tiers in the
[workers skill](../harness/skills/workers/SKILL.md), prices and benchmark scores in
[harness/model-catalog.toml](../harness/model-catalog.toml).

A worker that adopts or tests an outside source adds its entry here in the same commit
([external-code](../agent-machinery/skills/external-code/SKILL.md)).

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
  read at `e5136d8` on 2026-10-15, at `519d36a` on 2026-10-27 and at `fd268b8` on 2026-11-04. A followed source
  ([watching](../harness/skills/agent-policy/references/watching.md)).
  Informed: [rationale/pstack.md](rationale/pstack.md) for the 2026-10-15 decision, which adopted artifact-bound verdicts, a verification driver (`verify_north`), the
  correction ladder, gates with defaults, standing orders and the flat org; the 2026-10-27 review
  [rationale/watch/2026-10-27-pstack.md](rationale/watch/2026-10-27-pstack.md), north#262, north#263 and north#264; the 2026-11-04 review
  [rationale/watch/2026-11-04-pstack.md](rationale/watch/2026-11-04-pstack.md) and north#524.
  Outcome: adopted as ideas, no code copied; the two 2026-10-15 reviews are archived in south `agents/decisions/harness/`; of the 2026-10-27 mechanisms the dispatch order check and the citation check are live and the sharpen A/B has not run; the 2026-11-04 run-proven findings, able-to-fail properties and no-verifier-for-wording mechanisms are under test.

- <a id="mattpocock-skills"></a>**mattpocock/skills** (Matt Pocock, MIT), https://github.com/mattpocock/skills, read at
  `2ee14df` on 2026-07-10 and at `4ae2317` on 2026-11-04. A followed source
  ([watching](../harness/skills/agent-policy/references/watching.md)).
  Informed: the 2026-07-10 authoring norms (done-bars, leading words, the information hierarchy) in `684e72bfc`,
  credited in [agent-machinery/PROVENANCE.md](../agent-machinery/PROVENANCE.md); the 2026-11-04 review
  [rationale/watch/2026-11-04-mattpocock-skills.md](rationale/watch/2026-11-04-mattpocock-skills.md) and north#525.
  Outcome: adopted as ideas, no code copied. Done-bars are live in code and the information hierarchy as practice;
  leading words, the no-op test and positive phrasing were cut on 2026-08-02. Of the 2026-11-04 mechanisms the
  sampled retro was tested and kept (8 recurring failure modes with no enforcer in a one-in-five sample), and the
  Spec axis and the no-op test by replay are under test.

### Statistics

- <a id="wilson-interval"></a>**Wilson score interval** (E. B. Wilson, "Probable Inference, the Law of Succession, and Statistical
  Inference", JASA, 1927), https://doi.org/10.1080/01621459.1927.10502953; accessed: not recorded, cited
  2026-10-12.
  Informed: [crates/north-core/src/ops/policy_replay/stats.rs](../crates/north-core/src/ops/policy_replay/stats.rs), the sweep's operating point
  and the policy gate (10138bba, 012a7f3f), `recommend`'s no-pass tie-break (22d8c8ea), per-arm reports
  (d32f9266, 309eda0c), north#35 and north#36.
  Outcome: adopted; it keeps a sensible interval at small n and at rates of 0 or 1, where a normal standard
  error collapses to zero.

- <a id="paired-bootstrap"></a>**Paired bootstrap** (B. Efron, "Bootstrap Methods: Another Look at the Jackknife", Annals of Statistics,
  1979), https://doi.org/10.1214/aos/1176344552; accessed: not recorded, cited 2026-10-12.
  Informed: the task-cluster paired difference in
  [stats.rs](../crates/north-core/src/ops/policy_replay/stats.rs) and advisor-report (309eda0c, north#36), the routing
  backtest's differences (37017a39, [learned routing backtest](rationale/learned-routing-and-goal-eta.md#learned-routing-backtest)), and
  calibration sample-size floors (b6462af5).
  Outcome: adopted; arms run the same tasks, so pairing by task removes task difficulty from the difference.

- <a id="fisher-exact"></a>**Fisher's exact test** (R. A. Fisher, "On the Interpretation of χ² from Contingency Tables, and the
  Calculation of P", JRSS, 1922), https://doi.org/10.2307/2340521; accessed: not recorded, cited 2026-10-12.
  Informed: the one-sided drop and gain test in the policy gate and sweep selection (82fd8604,
  [stats.rs](../crates/north-core/src/ops/policy_replay/stats.rs)).
  Outcome: adopted; it is exact at the few trials per scenario a replay runs.

- <a id="noninferiority-margin"></a>**Non-inferiority margin**, common practice in non-inferiority trials; no document on record.
  Informed: the north#36 "matches" rule, `noninferiority_margin` in
  [harness/defaults.toml](../harness/defaults.toml).
  Outcome: adopted as a named standard; at the replay's sample sizes "beats" was out of reach, so a challenger
  is judged by whether it matches the incumbent for less.

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
  Informed: the heavy-build pressure target and its settle time in [pressure_target.rs](../crates/north-core/src/capacity/pressure_target.rs), `admit_psi_*` and
  `admit_settle_seconds` in [harness/defaults.toml](../harness/defaults.toml): "some" is the share of time at least one task waited for a CPU,
  averaged over 10, 60 and 300 s, which is why a controller must wait a window after each start before it reads again.
  Outcome: adopted; the marks come from the laptop's own samples, the kernel text only fixes what the number means.

### Runtime and code

- <a id="codex-app-server"></a>**Codex app-server documentation**, https://learn.chatgpt.com/docs/app-server, and the Codex source at the
  revision named in [architecture.md](architecture.md#prior-art-boundary); accessed: not recorded.
  Informed: [architecture.md](architecture.md), North's process and TUI split and its stdio JSONL transport.
  Outcome: adopted; Codex app-server owns provider communication, so North needs no provider SDK or second
  agent loop.

- <a id="dirge"></a>**DIRGE**, at the revision named in [architecture.md](architecture.md#prior-art-boundary), GPL-3.0-only; link
  not recorded.
  Informed: [architecture.md](architecture.md)'s adopt-after list: a central deterministic intervention arbiter,
  at most one intervention at a safe boundary, traces of selected and declined actions.
  Outcome: prior only; conceptual prior art, with no implementation copied or adapted because of its licence.

- <a id="threejs-skills"></a>**CloudAI-X/threejs-skills**,
  https://github.com/CloudAI-X/threejs-skills/blob/b1c623076c661fc9b03dac19292e825a5d106823/README.md; accessed:
  not recorded.
  Informed: [agent-machinery/skills/threejs/](../agent-machinery/skills/threejs/),
  [agent-machinery/PROVENANCE.md](../agent-machinery/PROVENANCE.md).
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
  https://nixos.org/manual/nixos/stable/release-notes.
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

### Skill bibliographies

Skills that cite many sources keep them beside the skill, so they are not repeated here:

- [harness/skills/agent-policy/references/guides.md](../harness/skills/agent-policy/references/guides.md):
  instruction, skill and hook writing guides.
- [agent-machinery/skills/verification/references/testing-sources.md](../agent-machinery/skills/verification/references/testing-sources.md): testing
  practice.
- [harness/skills/github/references/actions-sources.md](../harness/skills/github/references/actions-sources.md):
  GitHub Actions and REST limits.
- [harness/skills/warcraft-modding/references/lua-performance-sources.md](../harness/skills/warcraft-modding/references/lua-performance-sources.md):
  Lua and TypeScriptToLua performance.
- [harness/skills/warcraft-modding/references/api-gotchas.md](../harness/skills/warcraft-modding/references/api-gotchas.md),
  [harness/skills/makgora/references/animation-library.md](../harness/skills/makgora/references/animation-library.md),
  [agent-machinery/skills/babashka-development/references/](../agent-machinery/skills/babashka-development/references/)
  and [harness/skills/repo-safety/references/project-layout.md](../harness/skills/repo-safety/references/project-layout.md):
  domain references.
- [harness/model-watch-sources.toml](../harness/model-watch-sources.toml) and
  [harness/model-catalog.toml](../harness/model-catalog.toml): release feeds, prices and benchmark scores the
  routing reads.

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

<a id="inspiration-org-shape"></a>**The Socrates org shape.** The harness's hooks, lib and agent commands were copied into north from firn (4645127a),
and the workforce tree, a proxy at the root with leads, sub-leads and workers under it, is now
represented by [Fleet session parent links](features.md#fleet-sessions). The former org schema was retired
when its status bus callers moved to Fleet (d304e9c0). Where the shape and its names came from: origin
not recorded.

<a id="inspiration-replay-loop"></a>**The replay and calibration loop.** Policy replay runs a recorded scenario through an isolated session and
scores it with a detector (8bac5d16); the sweep picks the cheapest variant whose Wilson interval overlaps the
best one's (10138bba), and calibration scales predictions by measured actual over predicted (ac79788c). The
statistics come from the Wilson, Fisher and paired-bootstrap entries above, the routing backtest's second arm
from RouteLLM, and the walk-forward scoring and adopt-only-on-measured-gain rule from north#35 and north#36.
North#40 asks that every rule carry a source so others can repeat the evidence. Where the idea of replaying
recorded policy scenarios came from: origin not recorded.
