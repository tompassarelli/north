// Suite-wide hermeticity boundary (loaded via bunfig.toml [test] preload,
// before any test module is imported).
//
import { writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
//
// A managed dev lane runs `bun run test` with its OWN routing/topology/caveman
// env exported into the shell (AGENT_TOPOLOGY=worker, AGENT_ID, AGENT_MODEL,
// NORTH_ROUTING_PIN_EVIDENCE=RECORDED (a sentinel, not JSON — dispatch.ts:1038
// parses it), NORTH_CAVEMAN_HOME/_REV, ...). Those ambient values leak into
// every suite process exactly like the live-coordinator NORTH_PORT leak below:
// a managed lane's `bun run test` sees failures (18 completion-outcome +
// agents-cli-routing) that do not exist under a clean shell, because the
// lane's own identity/routing/caveman env got adopted instead of each test's
// explicit hermetic fixture (thread 019f8efb, verified 2026-07-23 against
// main 8391e37: clean-env 39/39 green vs 18 fail in lane env, byte-identical
// baseline diff otherwise). Scrub the whole AGENT_*/NORTH_ROUTING_*/
// NORTH_CAVEMAN_* surface (AGENT_WORKTREE included — it already matches
// AGENT_*) BEFORE any test file's own module-load snapshot, so `bun run test`
// is env-independent and no `env -u ...` prefix is ever required again.
for (const key of Object.keys(process.env)) {
  if (key.startsWith("AGENT_") || key.startsWith("NORTH_ROUTING_") || key.startsWith("NORTH_CAVEMAN_"))
    delete process.env[key];
}

// Same leak, different shape: a managed dev lane also exports a REAL babashka
// path (NORTH_PEER_BB/NORTH_BB/NORTH_MCP_BB — src/watchdog.ts:30, src/spend-guard.ts:180,
// src/harness.ts:68) so its own peer-command/spend-cli plumbing shells out to the
// real coordinator. Every SDK test that exercises those code paths supplies its
// own fake `bb` explicitly; none of them expect (or scrub) this ambient override
// themselves, so under a live managed lane's `bun run test` it silently replaces
// the intended fake with the real binary — observed 2026-07-24: watchdog.test.ts
// and spend-guard.test.ts fail under lane env (`bb` expected, real nix-store
// babashka path received; a real spend-cli reservation refusal instead of the
// fixture's), green under a clean shell. Delete it up front for the same reason
// as the AGENT_*/NORTH_ROUTING_* scrub above.
for (const key of ["NORTH_PEER_BB", "NORTH_BB", "NORTH_MCP_BB"]) delete process.env[key];
//
// When the suite runs inside a live managed north lane, the ambient
// NORTH_PORT points at the REAL coordinator on the session's port. Admission
// (requireCoordinator) and presence probes read NORTH_PORT, so a test that
// builds harnessOptions without first pinning its own fake coordinator would
// silently target the live coordinator. Because process.env is shared across
// test files and every coordinator-bearing suite snapshots+restores NORTH_PORT
// at module load, that live value leaks and races between files — surfacing as
// nondeterministic `north_coordinator_preflight_invalid_response` failures.
//
// Pin NORTH_PORT to a closed sentinel BEFORE any module snapshot, so every
// save/restore cycle carries the dead port, never the live coordinator. Suites
// that need a coordinator still stand up their own server and set NORTH_PORT to
// it in beforeAll/beforeEach; suites that assert a dead coordinator get exactly
// that. This is the "live coordinator = env hermeticity" contract in practice.
//
// A closed high port (nothing listens here) — connect() yields ECONNREFUSED,
// which admission treats as an unavailable coordinator, the honest default for
// a hermetic unit test.
const HERMETIC_DEAD_PORT = "59319";

if (process.env.NORTH_TEST_ALLOW_AMBIENT_COORDINATOR !== "1") {
  process.env.NORTH_PORT = HERMETIC_DEAD_PORT;
}

// Global-authority hermeticity. The laws bootstrap now resolves an exact
// AGENT_LAWS_PATH or ~/.agents/AGENTS.md — never a provider config home. So a
// bare suite run (no ambient AGENT_LAWS_PATH, no ~/.agents on the box) would
// fail every AGENT_LAWS=on assembly, and the tiered assembler (prompt-assembly)
// needs a real, section-structured constitution to gate. Pin the override to a
// self-contained SYNTHETIC constitution written to this process's temp dir when
// unset: it carries the exact section headings the tier gates key on, with no
// personal prose and no provider-home dependency. A per-pid path keeps parallel
// isolate runs from racing. Tests that exercise default resolution or the
// unavailable path delete or override AGENT_LAWS_PATH explicitly.
//
// NOTE: the repo's own root AGENTS.md is deliberately NOT used here — it is the
// North project doc, so it (a) lacks the constitution's gated sections and
// (b) is itself composed into the root-to-cwd project-instruction block, which
// would double-inject the same text and trip the exact-once bootstrap guard.
const SYNTHETIC_GLOBAL_AGENTS = `# Synthetic global constitution — SDK test fixture

Constitution, not manual: this is a hermetic, self-contained stand-in for the
provider-neutral global AGENTS.md. It exists only so laws assembly in the SDK
suite never reaches a provider config home, and carries the section structure
the tiered assembler gates on — no personal content.

## Blocked ≠ stopped

A denial is information about the path, not the goal. Find the nearest
compliant move that still advances; never retry a blocked action verbatim.

## Paths — full and \`~\`-anchored, always

Every path written is full from \`~\`, never bare-relative, so the reader never
has to intuit a working directory before acting on it.

## Done-claims carry a bar — probe + observed result

A done-claim cites the probe run and the observed result it produced, never the
bare adjective; each worker reports its own evidence against its own bars.

Evidence attaches where the done-claim lives: the coordinator that owns a
reduction reconciles and attests the aggregate, echoing every worker bar with
the exact observed result it saw, so the whole chain stays independently
checkable end to end rather than resting on any single lane's say-so.

## Standing guards

- Banned vocabulary: dead pre-rename naming must never leak back into output;
  prefer the current terms. Ordinary English usage of the words is fine.
- Never serialize work to protect the box — measure load instead; agent work is
  network-bound, so isolation is a measured decision, never a reflex.
- \`rm\` on variable paths — make it self-evidently safe so the guard never has to
  prompt: brace-guard every interpolated path segment or delete a literal dir.

## Pre-edit gate — MANDATORY

Before any first side effect a lane satisfies its pre-edit gate: it names the
exact authoritative artifact it read, confirms the coordination declaration is
in place, and states the probe that will demonstrate the change. The gate is a
coordinator-side discipline — orchestrating lanes carry it because they own the
reduction that a bare worker never sees, and skipping it silently converts an
unverified edit into an unearned done-claim that the aggregate cannot defend.

## Model + payload routing

Model selection and payload routing resolve from the sealed routing policy: the
semantic tier maps to a concrete model only through the admitted route, and the
orchestrating lane revalidates that seal before it publishes work to a peer.
This block rides only with coordination authority because a terminal worker
neither selects models nor routes payloads for anyone else; it simply executes
the exact route it was handed and reports the observed result upward.

## Push freely — the scan is the guard

Commit at coherent checkpoints, then push through the scan; stop only for a
flagged secret or a rewrite of already-published history.

## External code — license first

Before leveraging any code you did not write, run the license protocol and flag
copyleft or unlicensed sources before building on them.

## Internal notes → docs/private, never public docs

Agent notes, status, scratch, and handoffs go in the gitignored private docs
tree; public docs stay end-user-facing only.

## New code — minimize glue

Ladder down for incidental glue and stop at the first sufficient rung; hand-roll
the core deliberately. Correctness and security are never laddered away.

## Client time and agent time — two orthogonal clocks

Human/client presence is the billing clock. Agent/task duration remains
telemetry, not billing authority, and never satisfies the client-edit guard.

## Global agent config goes through nixos-config

Global agent configuration is owned by the dotfiles repo and never edited from
inside a provider config home.

## Racket / Beagle first for general-purpose programs

New general-purpose tools default to the graph-native language stack; every
escape hatch is stated in one line when it is taken.
`;

if (!process.env.AGENT_LAWS_PATH) {
  const fixture = join(tmpdir(), `north-sdk-global-agents-${process.pid}.md`);
  writeFileSync(fixture, SYNTHETIC_GLOBAL_AGENTS);
  process.env.AGENT_LAWS_PATH = fixture;
}

// Same class of leak as NORTH_PORT/AGENT_LAWS_PATH above, for the authoring-guard
// killswitch: authoringGuardsOff() (src/authoring-guards.ts) falls back to reading
// $NORTH_HARNESS_STATE / $AUTHORING_KILLSWITCH_STATE / ~/.local/state/north/harness.conf
// when neither override is set. On a real dev box that file tracks the operator's own
// live `north config` state (e.g. "guards=off" while doing unrelated authoring-guard
// work) — an ambient value with nothing to do with the suite. Any harness test that
// exercises a PreToolUse guard chain (gaffer-operational-semantics topology test
// included) would then silently no-op every guard and pass/fail on host mood instead
// of code (observed 2026-07-24: host harness.conf had guards=off, which alone flips
// "topology controls prompt and tools with positive-only orchestration authority"
// from 14/14 to 13/14 with NO src/ diff at all — confirmed by toggling the real file
// to guards=on and rerunning green). Pin the killswitch state path to a file that
// never exists, so the fallback always resolves to guards-on (the safe hermetic
// default) unless a test explicitly opts into its own fixture.
if (!process.env.NORTH_HARNESS_STATE && !process.env.AUTHORING_KILLSWITCH_STATE) {
  process.env.NORTH_HARNESS_STATE = join(
    tmpdir(), `north-sdk-no-harness-state-${process.pid}.conf`,
  );
}
