# Personal AGENTS.md (global)

Constitution, not manual: durable posture, authority, and routing — applies
to every session, every directory. Detail lives in the linked docs; read a
doc when its trigger fires, not preemptively.

## north — the coordination substrate
<!-- north-section: north · bucket: core -->

Read `north:docs/operating-manual.md` before nontrivial work; where
anything contradicts it, the manual wins (trivial lookups exempt).
**Session state lives on threads, not markdown dumps** — milestones → `tell
<id> progress`, lessons → `learning`, done → `outcome`; the next session
reads `north show <id>`, never a SESSION-DUMP file. SDK dispatch derives
agent posture from thread facts.
Thread format + concurrent write safety: → `~/.agents/docs/north.md`
Spawn/steer/observe/concurrency: → `~/.agents/docs/agent-protocol.md`

## Client time and agent time — two orthogonal clocks
<!-- north-section: client-time · bucket: client -->

**Human/client presence is the billing clock.** Before any edit under
`~/code/client/<owner>/**`, exactly one open North row must identify
`kind client_session`, `clocked_by user`, and that `owner`. `north clock in
<owner>` once when the client block starts; `north clock out` only when the
human context clearly leaves that client. Ticket switches within a client
never restart the clock; ambiguous drift gets one warning; explicit departure
clocks out. Generation waits, builds, and delegated work stay inside the
block while the client remains the focal human context.

**Agent/task duration is telemetry, never billing authority.** Every managed
lane records its own concurrent `kind run` timing against its thread. Run
clocks may overlap, start and stop with the lane, never appear on invoices,
and never satisfy the client-edit guard. Do not serialize workers or churn
the client clock to fit telemetry. Closed legacy `session_of` rows stay in
sent-invoice history (compat projection — never a live session, never edit
authority).

The axes join only for traceability: at intake, derive the Linear ticket from
the branch (`msa-NNN` → `MSA-NNN`) and find-or-`capture` exactly one thread
with `owner msa` + `linear MSA-NNN`. `north-clock-guard` requires that exact
branch/thread identity plus the matching human `client_session` — not that
the session point at the ticket. Coordination and clock commands always
remain available; unrelated repos and proved read-only operations inherit no
deny from a client cwd; ambiguous client mutations fail closed.

Billing is derived, never invented: worklog = `north-timelog`, invoices =
`north-invoice` (uninvoiced → invoice-sent → invoice-paid). Bypass only
deliberately: `north config guards off` or `AGENT_NO_AUTHORING_HOOKS=1`.

## Pre-edit gate — MANDATORY at task intake
<!-- north-section: pre-edit-gate · bucket: orch -->

Run it the moment the work's shape is clear (not when the first Edit looms):
**decompose** into independent subtasks → **graph** true dependencies only →
**dispatch** independent subtasks to agents IN PARALLEL, tier per SUBTASK
(never inherited from the session) → **coordinate** only the cross-cutting
seams (self-contained subtask ⇒ delegate it) → **attach verification where the
outcome lives** (doctrine Law 7: local bar evidence; verifier sibling on
verdict leverage; whole-outcome attestation for emergent aggregates) →
**consume and reconcile** that evidence, with at most one suspicious
load-bearing claim spot-checked on smell. Skip at ONE subtask; fires at 2+
files or 2+ concerns. Coordinate, don't execute; reconcile evidence, don't
trust a bare done-claim.
**Dispatch authority is live configuration, not profile law.** At task intake,
read `north config dispatch`; its current value decides which dispatch surface
creates workers:

- `native` pins the provider-native Agent/Workflow surface. North may still
  coordinate, record, and observe, but MUST NOT independently spawn or delegate
  workers.
- `north` pins the North-managed surface; provider-native Agent/Workflow calls
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
Inline work is limited to answering from context, reading, consuming and
reconciling delegated evidence, the one allowed suspicious spot-check, and
coordination acts.

When the North-managed surface is selected (`north`, or a North choice under
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

## Done-claims carry a bar — probe + observed result
<!-- north-section: done-claims · bucket: core -->

"Done"/"verified"/"fixed" is a JUDGMENT and must cite its evidence: state the
probe run and the result observed ("north validate → exit 0", "firn build +
validate → green"), never the bare adjective.
Graph-side: threads SHOULD carry `done_when` facts (probe +
expected result, one per fact) by commit time; `north dispatch` warns when a
committed thread lacks them and workers define their own bar as a first act;
outcomes on barred threads echo the bars, and needs-review surfaces
unevidenced ones (`bar_evidence` facts hold observed results). Capture stays
zero-ceremony — the bar attaches when work is ACCEPTED, not when a thought
is jotted.

Where evidence attaches, who attests, and the coordinator's one-spot-check
consumption budget are doctrine, not restated here: `north:orchestration/doctrine.md`
Law 7 and the verification doctrine own them.
Consumed evidence carries provenance: name the run that produced a fact
before drawing a verdict from it. A derived metric is a claim made by code —
read the producing line before reasoning from it, and never compare a
same-named metric across systems without reading both producers. When a
hypothesis is load-bearing enough to act on, run the cheapest experiment
that could FALSIFY it — move the input, watch the metric.

Full verification doctrine — claim contracts, paranoia tiers P0–P3, the
one-sentence stop rule, anti-tarpit laws: →
`~/.agents/docs/verification-doctrine.md`
Read when setting a bar/tier at intake, briefing a verifier, or a lane won't
converge on done.

Style: terse by default — no filler, no hedging, full sentences; brevity
comes from content selection, never compression tricks. Per clause: keep the
rule, its trigger, and the compliant move; provenance, rationale, and war
stories live on threads or provenance files, never in always-loaded text.

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
are local and ephemeral — land by fetch + `safe-push --to main`. Landing's
done-bar includes cleanup: remove your worktree and delete your branch
(`wt-reap` sweeps every merged+clean sibling); a landed lane that leaves its
worktree behind is not done. Never publish a feature branch name.

## External code — license first
<!-- north-section: external-code · bucket: write -->

→ `~/.agents/docs/external-code.md`
Before leveraging ANY code you didn't write (`~/code/reference`, forks,
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
Personal policy lives in North's `profiles/tom`; Beagle, Fram, and Firn keep
their integration-specific files in their owner roots and enter the profile by
relative links. `~/.agents` is the composed live projection. Claude Code and
Codex configuration are thin adapters to that projection, never additional
policy sources. Edit a file in its owning repository and commit it there.
Firn owns the NixOS wiring and application step.
**Agents QUEUE rebuilds, never fire them.** `north rebuild request --why
"<reason>"` records one durable ask and returns; `--urgent "<why>"` is counted
and never refused. The reactor's window owner coalesces every open ask into ONE
coordinated rebuild and closes each request against the generation that landed.
Direct `firn rebuild` and `firn-rebuild-coordinated` are DENIED for agent tool
calls (deliberate owner escape: `north config guards off`); `firn update` and
raw nixos-rebuild/nh stay the USER's. A rebuild still builds a COMMIT SNAPSHOT
(`rev=HEAD`), never the working tree — so your one gate is unchanged: commit
YOUR OWN changes before the window fires or they won't be in the build.
Build-only verify: `nix build --no-link`.
Dev environments activate via direnv (`use flake` in `.envrc`) — never bare
`nix develop` / `nix shell`.

## Hot-loop repos never ride the rebuild cycle
<!-- north-section: hot-loop-repos · bucket: write -->

north, fram, and beagle deliver code through their own channels — live
checkout (CLIs), `north-coord-runtime`/`north-runtime` promote (daemons,
reactor, timers), sealed `north-enforcement-promote` (guards, deliberately
slow). A rebuild whose purpose is adopting hot-loop code is a DEFECT: tag
the ask `--why "code-adoption: …"`, capture a thread naming the coupling,
and fix the channel instead. `north doctor` tracks code-adoption rebuild
asks; the target is zero. Rebuilds are for system config only.

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
bootstraps the stack — language `~/code/beagle`, engine `~/code/fram`).
Dogfood by default. Escape hatches, stated in one line when taken: the repo
is already committed to another language; a platform boundary demands one
(nix module, CI config, browser-only); or a one-liner where shell/python is
objectively the smaller move.

**Native Core gradient inside the bias.** System-layer Fram engine/store/coord
primitives and Beagle machinery target the target-neutral **Beagle Native
Core** profile: target-independent typed/effect/region/layout/control/
capability/ABI semantics. Their authoritative lowered program is an immutable
**Native World**. Fram stays entirely Beagle; greenfield work stays
graph-upstream per the rule below. Materializers are disposable projections:
restricted C11 for bootstrap/reference/sanitizers, QBE as the first
direct-native and anti-C-capture check, Wasm/WASI for capability sandboxing, and
LLVM/Cranelift/direct codegen only when measurement justifies them. Coverage
means 30/39 archived core modules lower into a validated Native World, never
"30 modules that print Zig."

The former system-layer **bzig** default is suspended; Zig is not a strategic
native destination. This is an institutional-fit policy, not a technical-defect
claim. Zig commits `0f5dcae`, `cf87612`, `2fcb72d`, and `9f37f7d` are frozen
as compatibility implementation, rollback, and differential oracle at the
closed 13-operation/12-oracle boundary; only narrowly justified
oracle-maintenance fixes may touch them. No Rust rewrite, handwritten-C domain
logic, or C3/Odin/Idris retarget. "Turtle" names only the turtles-all-the-way-down
architectural thesis, never a code identifier, row/log type, or compiler-phase
label. The amendment's provenance lives on North thread
`019fbd6c-7e2b-7e21-aa2a-57b581004f37`.

App-layer code (CLIs, projections, higher-level tooling) may stay Clojure
where the repo is already committed to it. The brownfield rule below is
unchanged: a bounded fix never switches language mid-task; deferred candidates
go to the migration inventory and ride an explicit pick.

**Greenfield vs brownfield stewardship.** A wholly new Beagle program/module
(greenfield) **starts graph-native at inception**. Follow the seed-only
bootstrap in
`~/code/beagle/main/integrations/north/skills/beagle-authoring/SKILL.md`: run
`fram:bin/fram-code-on ~/code/<repo>`, require flip level 3 in a fresh or
restarted trusted-project harness session, then author substantive code only via
the code-as-facts graph-edit verbs. A coordinator or session-wiring failure is a
repair-loop problem, never permission to fall back to text authoring.

A surface whose upstream is already Clojure/text (brownfield) never silently
expands the bounded task into a migration. Surface exactly three choices to the
human — (1) keep the current upstream/language for this bounded task, (2)
migrate to text-upstream Beagle, or (3) migrate directly to graph-upstream
Beagle — and wait for a pick before migrating. Deferred candidates go in a
separate migration inventory, never a side-project expansion. Graph-native
detail: `~/code/fram/main/integrations/north/skills/code-as-facts/SKILL.md`.

## Background shells — always accountable
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
  the dirty state intact to a rescue worktree and restores main to clean.
  Genuine git surgery on a main rides the deliberate bypass
  (`north config guards off`), stated aloud, guards back on after.
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
