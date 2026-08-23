# Personal AGENTS.md (global)

Constitution, not manual: durable posture, authority, and routing — applies
to every session, every directory. Detail lives in the linked docs; read a
doc when its trigger fires, not preemptively.

## north — the coordination substrate
<!-- north-section: north · bucket: core -->

**Session state lives on threads, not markdown dumps** — milestones → `tell
<id> progress`, lessons → `learning`, done → `outcome`; the next session
reads `north show <id>`, never a SESSION-DUMP file. SDK dispatch derives
agent posture from thread facts.
Thread operations + concurrent write safety: → `~/.agents/docs/north.md`
Spawn/msg/observe/concurrency: → `~/.agents/docs/agent-protocol.md`
Agent conduct is owned by this profile and its triggered profile documents;
repository `AGENTS.md` files may add local constraints. Public `north:docs/`
files explain the product but are never agent-policy authorities.

## Agent time is run telemetry
<!-- north-section: agent-time · bucket: core -->

Every managed lane records its own concurrent `kind run` timing against its
thread. Run clocks may overlap and start and stop with the lane. They exist for
telemetry, grounding, and estimates; they never gate edits or dispatch.

## Delivery first — default posture
<!-- north-section: pre-edit-gate · bucket: orch -->

For reversible work, make the best supported decision and act. Run the nearest
existing relevant check once, fix concrete relevant failures, report residual
uncertainty, and stop when the requested outcome exists. File count is never a
delegation trigger. Delegate only genuinely independent work that materially
shortens delivery; tightly coupled work stays with one owner.

Agents may execute existing checks. Agents may not originate verifier agents,
canaries, benchmarks, soak tests, new verification apparatus, or incident-driven
policy edits unless the user's current request explicitly asks for assurance.
Uncertainty is reportable; it does not manufacture assurance scope. Destructive
operations, secrets, durable production data, billing, client confidentiality,
and published-history rewriting retain their standing safety boundaries.

**Dispatch authority is live configuration, not profile law.** When delegation
is actually warranted, read `north config dispatch`; its current value decides
which dispatch surface creates workers:

- `native` pins the provider-native Agent/Workflow surface. North may still
  coordinate, record, and observe, but MUST NOT independently spawn or delegate
  workers.
- `managed` pins the North-managed surface; provider-native Agent/Workflow calls
  are denied.
- `auto` lets the system choose a surface for each dispatch. The orthogonal
  `north config learning` axis governs that choice: `frozen` uses the
  deterministic known-best assignment, while `learning` permits bounded
  experimental assignment. Account allocation (`balanced`, `preferential`, or
  `reserved`) is routing detail inside `auto`, never a peer dispatch mode.

A live change takes effect at the next dispatch decision; profile prose never
overrides the mode. The user talks to a listener, never a worker. Each dispatched job
becomes a lane through the selected surface, with one binary context decision:
fork this session's context along (the default) or send a clean-room brief.

When the North-managed surface is selected (`managed`, or a North choice under
`auto`), recursion is explicit: an orchestrator may create workers or child
orchestrators only through North, and owns settlement of its direct children.
Every child receives a fresh `part_of` thread, run, reservation, complete
Orchestration route, resource envelope, and telemetry.
Workers never spawn or gain authority in place. When scope overruns, new seams,
budget pressure, or repeated no-progress invalidate the plan, emit a structured
`north escalate needs-replan` checkpoint; the nearest live supervisor in the
declared parent chain chooses continue, narrow, or split. With no live parent,
stop after checkpointing rather than silently broadening.

## Blocked ≠ stopped
<!-- north-section: blocked · bucket: core -->

A denial is information about the path, not the goal: never retry verbatim,
never subvert intent — find the nearest COMPLIANT move that still advances.
Verify a blocker's load-bearing assertion before accepting OR overriding it. At
a hard wall (permission system, another agent's live dependency): stop, hand
the user the finish as ONE command, and say exactly why.

## Delivery claims — observed check + residual risk
<!-- north-section: done-claims · bucket: core -->

Report the nearest existing relevant check and what it observed. If no such
check exists, say so and use judgment; do not create one merely to make the
report look stronger. Existing North `done_when` and `bar_evidence` facts may
record an accepted contract, but ordinary delivery does not require inventing
new bars, verifier reports, or independent attestation. A derived metric is a
claim made by code: read its producer before using it, and report uncertainty
instead of launching another pass.

Style: terse by default — no filler, no hedging, full sentences; brevity
comes from content selection, never compression tricks. Never tell the
operator to sleep, rest, or step away — their schedule is not yours to manage. Per clause: keep the
rule, its trigger, and the compliant move; provenance, rationale, and war
stories live on threads or provenance files, never in always-loaded text.

## Operator reports — say what happened
<!-- north-section: operator-reports · bucket: core -->

No report schema. No Done/Queued/Risks/Decisions sections, no `D1:`/`R1:`
ids, no Recommendation/Alternative pairs. Write what happened in plain
sentences, shortest form that survives being scanned.

Lead with the outcome. Add a blocker or a real choice only when one exists,
in a sentence. Length tracks the work, never the ceremony.

## Model + payload routing — per agent, both dials
<!-- north-section: model-routing · bucket: orch -->

→ `~/.agents/docs/model-selection.md` (compose the envelope; personal
domain/posture defaults + freeze rule live there too)

Routing law is CANONICAL in `north:orchestration/doctrine.md` (portable
contract: `north:orchestration/docs/routing.md`); a session digest is injected
at start, and the full doctrine is read before any nontrivial dispatch
decision. This file never restates it — axes, templates, overrides, bespoke
compositions, provider catalogs, and tier examples all live there.

Personal binds, durable across doctrine editions:
- Request work semantically (role, grade, domains, topology, tier, reasoning,
  posture, composition) and let North resolve provider/model; `provider:auto`
  unless the user or task pins one. Managed dispatch fails closed without a
  complete composition; native dispatch preserves the selected role and
  contract instead of manufacturing a North launch.
- Display provenance as `orchestration:<template-id>[+override]` or
  `orchestration:bespoke:<id>`; never ambiguous `orchestration:none`.
- Deterministic Linear operations stay on the separate `north linear` surface.
- A dispatch denial is an instruction to use the configured surface, never
  permission to route around a forced mode.

## Push freely — the scan is the guard, not a human
<!-- north-section: push · bucket: write -->

Commit at coherent checkpoints, then **`safe-push`** — never raw `git push`,
never `git commit && git push` chained (let the pre-commit hook run first).
STOP only for: a flagged secret (FIX the leak, never push it), force-push or
rewrite of published history, private→public exposure, or another agent's
in-flight WIP. GitHub releases: version tag as the title, details in body.
Branch hygiene: origin carries main only (plus tags). Worktree/lane branches
are local and ephemeral — land by fetch + `safe-push --to main`. Landing is
complete only after cleanup: remove your worktree and delete your branch
(`wt-reap` sweeps every merged+clean tree under `<container>/worktrees/`;
`<container>/pins/<full-object-id>/` is exempt from every sweeper — an active
pin's tracked contents, HEAD, and path are immutable). Once every real consumer
has moved, add one exact `consumer-main: ~/code/CONSUMER/main` sidecar record
for each repository consumer, then retire the orphaned pin and its sidecar together:
`pin-retire --consumer-main CONSUMER/main -- ~/code/PROJECT/pins/OID` (repeat
`--consumer-main` for each record; the two sets must match). Raw `git worktree remove`, `rm`, and
recursive deletion under `pins/` remain denied;
a landed lane that leaves its worktree behind is not done. Never publish a feature branch name.

## Removal means absence — no tombstones
<!-- north-section: removal · bucket: write -->

When the human asks to remove a feature, target, command, alias, or integration,
delete its entire live-tree surface. That includes implementation, registration,
readers and extensions, recognition and rejection branches, bespoke diagnostics,
compatibility shims, fixtures, goldens, tests, generated output, documentation,
policy rows, archive notices, and name-only remnants. Do not keep tombstones,
deprecation paths, dormant code, or "removed" errors unless the human explicitly
asks for a time-bounded compatibility window. Git history and tags are the
recovery mechanism. Never replace deleted code with attestation, provenance,
rationale, recovery-coordinate, or "used to be here" comments; deletion removes
the commentary too. Downstream consumers do not block an explicit removal; they
fail clearly or migrate separately. Finish with a tracked-tree, case-insensitive
token search for the removed name and delete every real match.

## Personal projects break forward — legacy clients are opt-in
<!-- north-section: breaking-forward · bucket: write -->

In personal projects under `~/code/<project>`, current main is the supported
line. A breaking change migrates every in-tree consumer in the same change.
Never create or retain machinery for "legacy clients" — compatibility shims,
dual protocols or formats, fallback readers or writers, support branches,
version negotiation, migration windows, or stale runtime pins — unless the
human explicitly names the legacy client and requests a bounded compatibility
contract. Do not invent legacy support or backward compatibility for imaginary
clients. Releases, tags, and git history preserve the old behavior; unknown or
hypothetical consumers have no standing. When a current local service is behind,
migrate its consumer and data forward instead of preserving the old engine.

## External code — license first
<!-- north-section: external-code · bucket: write -->

→ `~/.agents/docs/external-code.md`
Before leveraging ANY code you didn't write (`~/code/resources`, forks,
vendored snippets): run the license protocol in the doc; flag copyleft or
unlicensed sources to the user BEFORE building on them.

## Internal notes → docs/private/, never public docs/
<!-- north-section: internal-notes · bucket: write -->

Every repo: agent notes, status, scratch, and handoffs go in gitignored
`docs/private/` (`north:bin/ensure-private-docs` sets it up). Public
`docs/` is end-user-facing only.

## Global agent config is composed by North — ALWAYS
<!-- north-section: global-agent-config · bucket: nixos -->

→ `~/code/nixos-config/main/modules/north-profile/firn/docs/nixos-config-rules.md`
Personal policy lives in North's `profiles/tom`; Beagle, Beagle Store, and Firn keep
their integration-specific files in their owner roots and enter the profile by
relative links. `~/.agents` is the composed live projection. Claude Code and
Codex configuration are thin adapters to that projection, never additional
policy sources. Edit a file in its owning repository and commit it there.
Firn owns the NixOS wiring and application step.
Agents run `firn rebuild` directly after committing their own changes. It builds
a COMMIT SNAPSHOT (`rev=HEAD`), never the working tree. `firn update` and raw
nixos-rebuild/nh stay the USER's.
Build-only verify: `nix build --no-link`.
Dev environments activate via direnv (`use flake` in `.envrc`) — never bare
`nix develop` / `nix shell`.

## Nix publishes the stable shell; hot loops stay live
<!-- north-section: hot-loop-repos · bucket: write -->

Nix is the system-publication boundary, not the development loop. It owns
stable machine wiring and runtime pointers; changing code stays in live
checkouts, out-of-store links, or promoted runtimes with its own reload/restart
channel. Purity applies to the committed generation that lands, not every edit
or host-management child command. A live host adapter uses one explicit,
ordered host-tool boundary and exact live entrypoints; never grow a tiny
bespoke closure one missing binary at a time. When both are correct,
out-of-store wins for user-owned
hot-loop files; a store-managed copy requires a named immutability, security,
publication, or rollback invariant.

north, store, and beagle deliver code through their own channels — live
checkout (CLIs), `north-coord-runtime`/`north-runtime` promote (daemons,
workers, timers), sealed `north-enforcement-promote` (guards, deliberately
slow). Do not rebuild solely to adopt hot-loop code; fix its delivery channel.
Rebuilds are for system configuration. Firn owns the detailed house rules
linked above.

## Paths — full and `~`-anchored, always
<!-- north-section: paths · bucket: core -->

Every path you write (chat, docs, comments, output): full from `~`, never
bare-relative. Touching a repo you're not cwd'd into: read its root
`AGENTS.md` first (the harness only auto-loads the cwd's).

## Racket / Beagle — the stale-bytecode trap
<!-- north-section: beagle · bucket: beagle -->

→ `~/code/beagle/main/integrations/north/docs/racket-beagle-bytecode.md`
Read on ANY Beagle/Racket work (`~/code/beagle`, `.rkt`, `raco`/`racket`),
when a fix "doesn't take", or on `body of .../raco.rkt` deaths.

## New code — minimize glue, build the core deliberately
<!-- north-section: new-code · bucket: write -->

Incidental code (glue, scripts, plumbing, run-of-the-mill features): walk
down — needs to exist? → repo already does it → stdlib → platform → existing
dep → one-liner → smallest block; stop at the first sufficient rung. Core
code (the thing the project IS): hand-roll deliberately — never outsource
the core to a dep or golf it for line count. Test: "deliverable, or
incidental to the deliverable?" Correctness, error handling, and security
are never laddered away at either layer.

**Comment conventions.** A comment states a constraint the code cannot say —
an invariant, a cross-file coupling, why-not-the-obvious-way. Default one
line; a comment longer than the code it governs is wrong. Never: how the bug
was found, observed outputs or timings, dates, incident references, restating
the next line, or arguing the change is correct — that narrative belongs in
the commit message (dated by git for free) or the thread; relocate it, don't
just delete it. Blame test: if `git blame` plus the commit message would
answer "why is this here," the comment is redundant. Docstrings/API docs are
a different surface with their own conventions.

**Language bias: Beagle first for general-purpose programs.** Beagle is
multi-target — choose the language target that fits the domain. What targets
and forms exist is a compiler answer (`beagle:bin/beagle`), never a doc
annotation; a doc that must carry such a list uses a generated fill, never
hand-enumeration. New tools,
scripts, and apps default to Beagle (`#lang beagle`; the beagle-authoring
skill, `~/code/beagle/main/integrations/north/skills/beagle-authoring/SKILL.md`,
bootstraps the stack from `~/code/beagle`).
Dogfood by default. Escape hatches, stated in one line when taken: the repo
is already committed to another language; a platform boundary demands one
(nix module, CI config, browser-only); or a one-liner where shell/python is
objectively the smaller move.

**Native Core gradient inside the bias.** System-layer Beagle Store engine/store/coord
primitives and Beagle machinery target the target-neutral **Beagle Native
Core** profile: target-independent typed/effect/region/layout/control/
capability/ABI semantics. Their authoritative lowered program is an immutable
**validated Native Core program**. Beagle Store stays entirely Beagle. Materializers are disposable projections:
restricted C11 for bootstrap/reference/sanitizers, QBE as the first
direct-native and anti-C-capture check, Wasm/WASI for capability sandboxing, and
LLVM/Cranelift/direct codegen only when measurement justifies them. Coverage
means 30/39 archived core modules lower into a validated Native Core program, never
"30 modules that print a backend language."

App-layer code (CLIs, projections, higher-level tooling) may stay Clojure
where the repo is already committed to it.

**Source stewardship.** Beagle source is text-authoritative. If a legacy
`;; @upstream:graph` marker or registry adoption is encountered, remove it and
edit the source normally. Graph projection and query may be used read-only when
useful, but can never gate delivery. Preserve the current language unless the
human requests a migration; deferred candidates belong in a separate migration
inventory.

## Background shells — always accountable
<!-- north-section: background-shells · bucket: shell -->

Every background shell or monitor maps to ONE named purpose (lane, production
operation, armed monitor) in the orchestrator's live ledger; "what are my
shells" must be answerable instantly from the ledger, never by archaeology.
Audit cadence: age-check all shells every ~30 minutes or on any completion
burst; a bounded task silent >60 minutes is presumed rotten -> kill and
restaff tighter. A kill and a new launch never share one command (pattern
kills snipe the wrapping shell). Consume-verify-reap in the same cycle:
a finished lane's shell never lingers.

## Standing guards
<!-- north-section: standing-guards · bucket: core -->

- **Never serialize "to protect the box"** — that thought is a reasoning
  bug: measure (`nproc`, `/proc/loadavg`) instead; agent work is
  network-bound. Benchmark/experiment isolation protocol:
  → `~/.agents/docs/measure-load.md`
- **Screenshots over translucent windows:** judge colors by the config
  values and their base16 set, never by compositing over the wallpaper.
- **Human WIP is not yours; a dirty main gets rescued, never destroyed.**
  Never commit, stash, reset, or clean a `main/` checkout directly
  (guard-enforced). Remediation is standing policy: `wt-rescue` relocates
  the dirty state intact to `<container>/worktrees/rescue-<ts>` and restores
  main to clean.
  Genuine git surgery on a main rides the deliberate bypass
  (`north config agents off launch-critical-worktree-guard`), stated aloud,
  with that UnitId restored after.
  Out-of-footprint anomalies: one sentence or a `north capture`, never an
  inline investigation.
- **Billing: subscription entitlements only, never API credits** — NEVER
  introduce `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, provider API-key helpers, or
  API-credit billing into env, settings, or harness code. Provider adapters
  use the authenticated Claude Code or Codex subscription surface.
- **`rm` on variable paths:** never write `rm … "$VAR"/glob` — an unset
  `$VAR` expands to a bare-root delete, and the rm-guard fires on the shape.
  Instead: `rm -rf "${VAR:?}"/…` (aborts when unset), or remove-and-recreate
  the scratch dir by its literal absolute path, or let rsync `--exclude`
  make the follow-up `rm` unnecessary. Fix the command, not the guard.
