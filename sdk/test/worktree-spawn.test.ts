// Spawn-wiring proof for per-lane worktree isolation (design: docs/private/worktree-isolation-report.md).
// The pure salvage-gate + payload contract live in worktree.test.ts; THIS file drives the
// impure spawn() seam end-to-end, hermetically (fake `north` on NORTH_BIN, unused NORTH_PORT,
// injected queryFn that CAPTURES the SDK Options and returns a clean `ran`). Two guarantees:
//   1. Mutation-capable compositions DEFAULT ON when no explicit worktree choice is made.
//   2. Explicit opt-out (worktree:false) refuses mutation before provider execution.
//   3. OPT-IN (worktree:true) => a real worktree provisioned at /tmp/<repo>-lane-<id> on
//      branch lane-<id>; Options.cwd points INTO it; the isolation payload is appended; the
//      worktree/branch facts route through NORTH_BIN; a clean `ran` removes the tree inline.
import { test, expect, beforeAll, afterAll } from "bun:test";
import { presetRequest } from "./routing-fixtures";
import {
  chmodSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync,
} from "node:fs";
import { execFileSync } from "node:child_process";
import { join, resolve } from "node:path";
import { tmpdir } from "node:os";
import { BEAGLE_STORE_RUNTIME_HOME } from "../src/beagle-store";
import type {
  WorktreeAllocationEvent,
  WorktreeAllocationRegistration,
} from "../src/worktree";
import { ProviderRetrySafeError, type RoutedQueryArguments } from "../src/providers";
import { wireTurnQuery } from "./support/wire-query";

let dir: string;      // fake-north sandbox
let log: string;      // fake-north invocation log
let repo: string;     // scratch git repo the opt-in lane worktrees off of
let origCwd: string;

const MANAGED_ENV = [
  "PATH", "NORTH_BIN", "NORTH_PEER_BB", "NORTH_IDENTITY_TEST_REDIRECT", "NORTH_PORT", "NORTH_STREAM_DIR",
  "AGENT_LAWS", "AGENT_PRAXIS",
  "AGENT_ID", "NORTH_AGENT_ID", "AGENT_COORDINATOR", "AGENT_MODEL", "AGENT_ROLE",
  "AGENT_WORKTREE", "AGENT_SETUP_CMD",
  "AGENT_TOPOLOGY", "AGENT_TASK_GRADE", "AGENT_REASONING", "AGENT_POSTURE",
  "AGENT_PROVIDER", "AGENT_TARGET", "AGENT_TIER", "AGENT_IDENTITY_ROLE",
  "AGENT_DOMAIN_REQUIREMENTS", "AGENT_COMPOSITION",
  "NORTH_STALL_MS", "BEAGLE_STORE_HOME", "BEAGLE_STORE_BIN", "BEAGLE_STORE_OUT",
] as const;
const origEnv: Record<string, string | undefined> = {};
for (const k of MANAGED_ENV) origEnv[k] = process.env[k];

const TEST_COORDINATOR = `test-coordinator-${process.pid}`;

// A fake SDK query that CAPTURES the Options it was handed, then yields a single clean
// `result` turn so spawn() finalizes outcome=`ran` (the only worktree-removal case).
function capturingQuery(sink: { options?: RoutedQueryArguments["options"] }) {
  return (args: RoutedQueryArguments) => {
    sink.options = args.options;
    return wireTurnQuery(args, { output: "done" });
  };
}

// Hermetic feed subscription (live-input route) matching spawn-boundary.test.ts.
function readySubscription(stop: () => void = () => {}) {
  return Object.assign(stop, {
    ready: Promise.resolve(),
    caughtUp: Promise.resolve(),
    replay: async () => {},
    drain: async () => {},
    isArmed: () => true,
  });
}

beforeAll(() => {
  origCwd = process.cwd();
  dir = mkdtempSync(join(tmpdir(), "north-wt-spawn-"));
  log = join(dir, "north.log");
  const fake = join(dir, "north");
  writeFileSync(fake, `#!/usr/bin/env bash\nprintf '%s\\n' "$*" >> "${log}"\nexit 0\n`);
  chmodSync(fake, 0o755);
  const fakeBb = join(dir, "bb");
  writeFileSync(fakeBb, `#!/usr/bin/env bash\nprintf 'bb %s\\n' "$*" >> "${log}"\nexit 0\n`);
  chmodSync(fakeBb, 0o755);
  const fakeClaude = join(dir, "claude");
  writeFileSync(fakeClaude, `#!/usr/bin/env bash
if [ "$1" = "--version" ]; then printf '%s\n' '2.1.0-test'; exit 0; fi
if [ "$1" = "auth" ] && [ "$2" = "status" ] && [ "$3" = "--json" ]; then
  printf '%s\n' '{"loggedIn":true,"authMethod":"claude.ai","apiProvider":"firstParty"}'
  exit 0
fi
exit 2
`);
  chmodSync(fakeClaude, 0o755);
  const fakeCodex = join(dir, "codex");
  writeFileSync(fakeCodex, `#!/usr/bin/env bash
if [ "$1" = "--version" ]; then printf '%s\n' 'codex-test'; exit 0; fi
if [ "$1" = "login" ] && [ "$2" = "status" ]; then
  printf '%s\n' 'Logged in using ChatGPT'
  exit 0
fi
exit 2
`);
  chmodSync(fakeCodex, 0o755);

  process.env.PATH = `${dir}:${process.env.PATH}`;
  process.env.NORTH_BIN = fake;
  process.env.NORTH_PEER_BB = fakeBb;
  process.env.NORTH_IDENTITY_TEST_REDIRECT = "1";
  process.env.NORTH_PORT = "59999";
  process.env.NORTH_STREAM_DIR = dir;
  process.env.AGENT_LAWS = "off";
  process.env.AGENT_PRAXIS = "off";
  delete process.env.AGENT_ID;
  delete process.env.NORTH_AGENT_ID;
  delete process.env.AGENT_MODEL;
  delete process.env.AGENT_ROLE;
  delete process.env.AGENT_WORKTREE;
  delete process.env.AGENT_SETUP_CMD;
  // A live managed lane exports its own Orchestration envelope; a hermetic spawn test
  // must not inherit worker topology (spawn would be denied) or any routing pin.
  delete process.env.AGENT_TOPOLOGY;
  delete process.env.AGENT_TASK_GRADE;
  delete process.env.AGENT_REASONING;
  delete process.env.AGENT_POSTURE;
  delete process.env.AGENT_PROVIDER;
  delete process.env.AGENT_TARGET;
  delete process.env.AGENT_TIER;
  delete process.env.AGENT_IDENTITY_ROLE;
  delete process.env.AGENT_DOMAIN_REQUIREMENTS;
  delete process.env.AGENT_COMPOSITION;
  process.env.AGENT_COORDINATOR = TEST_COORDINATOR;
  delete process.env.BEAGLE_STORE_HOME;
  delete process.env.BEAGLE_STORE_BIN;
  delete process.env.BEAGLE_STORE_OUT;

  // Scratch git repo the opt-in lane cuts its worktree from. basename must be unique so the
  // /tmp/<basename>-lane-<id> path can't collide with a real repo's worktree.
  repo = join(dir, `wtspawnrepo-${process.pid}`);
  execFileSync("git", ["init", "-q", "-b", "main", repo]);
  execFileSync("git", ["-C", repo, "config", "user.email", "t@t.t"]);
  execFileSync("git", ["-C", repo, "config", "user.name", "t"]);
  writeFileSync(join(repo, "a.txt"), "hello\n");
  execFileSync("git", ["-C", repo, "add", "a.txt"]);
  execFileSync("git", ["-C", repo, "commit", "-qm", "init"]);
});

afterAll(() => {
  try { process.chdir(origCwd); } catch {}
  for (const k of MANAGED_ENV) {
    if (origEnv[k] === undefined) delete process.env[k];
    else process.env[k] = origEnv[k];
  }
  try { rmSync(dir, { recursive: true, force: true }); } catch {}
});

test("explicit worktree:false refuses managed mutation before canonical mutation", async () => {
  const { spawn } = await import("./support/spawn");
  const sink: { options?: RoutedQueryArguments["options"] } = {};
  const agentId = "wt-off-1";
  let providerQueries = 0;
  let thrown: unknown;
  try { await spawn({
    prompt: "trivial default lane", agentId, worktree: false,
    routingMetadata: presetRequest("integrator"),
    queryFn: (args: RoutedQueryArguments) => {
      providerQueries++;
      return capturingQuery(sink)(args);
    },
    feedSubscriber: () => readySubscription(),
  }); } catch (error) { thrown = error; }

  expect(String(thrown)).toContain("managed mutation cannot opt out of a registered worktree allocation");
  expect(String(thrown)).toContain("remove worktree:false to use the default managed worktree lane");
  expect(String(thrown)).toContain("drop mutation capabilities for a read-only lane");
  expect(String(thrown)).toContain("canonical checkout mutation denied");
  expect(providerQueries).toBe(0);
  expect(sink.options).toBeUndefined();
  // No worktree/branch fact was ever written for this lane.
  const logged = existsSync(log) ? readFileSync(log, "utf8") : "";
  expect(logged).not.toContain(`tell agent:${agentId} worktree`);
  expect(logged).not.toContain(`tell agent:${agentId} branch`);
  // No worktree directory materialized anywhere under /tmp for this id.
  expect(existsSync(`/tmp/${agentId}`)).toBe(false);
});

test("mutation-capable spawn composes worktree=true without AGENT_WORKTREE", async () => {
  const { spawn } = await import("./support/spawn");
  delete process.env.AGENT_WORKTREE;
  process.chdir(repo);
  const sink: { options?: RoutedQueryArguments["options"] } = {};
  const agentId = "wt-default-mutation-1";
  const expectedPath = `/tmp/${require("node:path").basename(repo)}-lane-${agentId}`;
  const registrations: WorktreeAllocationRegistration[] = [];
  try {
    await spawn({
      prompt: "default mutation lane",
      agentId,
      routingMetadata: presetRequest("integrator"),
      queryFn: capturingQuery(sink),
      feedSubscriber: () => readySubscription(),
      worktreeAllocationWriter: {
        register: (registration: WorktreeAllocationRegistration) => registrations.push(registration),
        event: () => {},
      },
    });
  } finally {
    process.chdir(origCwd);
    if (existsSync(expectedPath)) rmSync(expectedPath, { recursive: true, force: true });
  }

  expect(registrations).toHaveLength(1);
  expect(registrations[0].worktree).toBe(expectedPath);
  expect(sink.options!.cwd).toBe(expectedPath);
  expect(sink.options!.systemPrompt).toContain("Worktree isolation");
});

test("OPT-IN (worktree:true) => real worktree, cwd inside it, payload appended, facts written, clean ran removes it", async () => {
  const { spawn } = await import("./support/spawn");
  process.chdir(repo); // spawn reads repoRoot = process.cwd()
  const sink: { options?: RoutedQueryArguments["options"] } = {};
  const agentId = "wt-on-1";
  const expectedPath = `/tmp/${require("node:path").basename(repo)}-lane-${agentId}`;
  const registrations: WorktreeAllocationRegistration[] = [];
  const events: WorktreeAllocationEvent[] = [];

  const result = await spawn({
    prompt: "trivial worktree lane", agentId, worktree: true,
    routingMetadata: presetRequest("integrator"),
    queryFn: capturingQuery(sink),
    feedSubscriber: () => readySubscription(),
    worktreeAllocationWriter: {
      register: (registration: WorktreeAllocationRegistration) => registrations.push(registration),
      event: (_subject: string, event: WorktreeAllocationEvent) => events.push(event),
    },
  });
  process.chdir(origCwd);

  expect(typeof result).toBe("string");
  // Options.cwd points INTO the provisioned worktree.
  expect(sink.options!.cwd).toBe(expectedPath);
  // The isolation + landing + verify payload is appended to the lane's system prompt.
  expect(sink.options!.systemPrompt).toContain("Worktree isolation");
  expect(sink.options!.systemPrompt).toContain("ISOLATED");
  expect(sink.options!.systemPrompt).toContain("--ff-only");
  // Reports pointed at the MAIN tree's docs/private (absolute), not the worktree's.
  expect(sink.options!.systemPrompt).toContain(`${repo}/docs/private`);
  // worktree + branch facts routed through NORTH_BIN (not a bare-`north` escape).
  const logged = readFileSync(log, "utf8");
  expect(logged).toContain(`tell agent:${agentId} worktree ${expectedPath}`);
  expect(logged).toContain(`tell agent:${agentId} branch lane-${agentId}`);
  expect(registrations).toHaveLength(1);
  expect(registrations[0]).toMatchObject({
    worktree: expectedPath,
    durableRef: `refs/heads/lane-${agentId}`,
    repositoryLayout: "standalone",
    agent: `@agent:${agentId}`,
    thread: "@thread:ad-hoc",
  });
  expect(events.map(({ type }) => type)).toEqual([
    "provisioned", "authority-profiled", "quarantined",
  ]);
  expect(events[1].providerAuthorityProfile).toMatchObject({
    phase: "resolved",
    authMode: expect.stringMatching(/^(ambient|isolated)$/),
  });
  expect(existsSync(expectedPath)).toBe(true);
  // The lane branch lives in the CLONE's own ref space (self-contained), not the canonical repo.
  const branches = execFileSync("git", ["-C", expectedPath, "branch", "--list", `lane-${agentId}`], { encoding: "utf8" });
  expect(branches).toContain(`lane-${agentId}`);
  rmSync(expectedPath, { recursive: true, force: true });
});

test("explicit worktree provisioning failure aborts before provider, admission, identity, or run side effects", async () => {
  const { spawn } = await import("./support/spawn");
  const agentId = "wt-provision-fail-1";
  const branch = `lane-${agentId}`;
  const expectedPath = `/tmp/${require("node:path").basename(repo)}-${branch}`;
  const beforeLog = existsSync(log) ? readFileSync(log, "utf8") : "";
  const sharedBytes = readFileSync(join(repo, "a.txt"), "utf8");
  const sink: { options?: RoutedQueryArguments["options"] } = {};
  let providerQueries = 0;
  let envelopeAdmissions = 0;

  // Plant a real `git worktree add -b` failure: the exact derived branch
  // already exists. The branch is harmless and points at the scratch repo HEAD.
  execFileSync("git", ["-C", repo, "branch", branch, "HEAD"]);
  process.chdir(repo);
  let thrown: unknown;
  try {
    await spawn({
      prompt: "must never reach provider execution",
      agentId,
      worktree: true,
      routingMetadata: presetRequest("integrator"),
      queryFn: (args: RoutedQueryArguments) => {
        providerQueries++;
        return capturingQuery(sink)(args);
      },
      feedSubscriber: () => readySubscription(),
      admitResourceEnvelope: async () => {
        envelopeAdmissions++;
        throw new Error("envelope admission must be unreachable");
      },
    });
  } catch (error) {
    thrown = error;
  } finally {
    process.chdir(origCwd);
  }

  expect(String(thrown)).toContain("explicit worktree provisioning failed");
  expect(String(thrown)).toContain("spawn aborted before provider execution");
  expect(providerQueries).toBe(0);
  expect(envelopeAdmissions).toBe(0);
  expect(sink.options).toBeUndefined();
  expect(existsSync(expectedPath)).toBe(false);
  expect(readFileSync(join(repo, "a.txt"), "utf8")).toBe(sharedBytes);
  expect(execFileSync("git", ["-C", repo, "status", "--porcelain"], { encoding: "utf8" })).toBe("");
  // Physical registration is now the one intentional pre-Git side effect. The
  // collision leaves an append-only planned -> quarantined history plus an
  // explicit orphan-recovery fact, while provider/admission/run remain unreachable.
  const afterLog = existsSync(log) ? readFileSync(log, "utf8") : "";
  const delta = afterLog.slice(beforeLog.length).trim().split("\n");
  expect(delta).toHaveLength(3);
  const allocationWriter = resolve(import.meta.dir, "../../cli/worktree-allocation-internal.clj");
  const allocationPrefix = `bb -cp ${join(BEAGLE_STORE_RUNTIME_HOME, "out")} ${allocationWriter} 59999`;
  expect(delta[0]).toStartWith(`${allocationPrefix} register `);
  expect(delta[1]).toContain('"type":"quarantined"');
  expect(delta[1]).toContain('"code":"durable_ref_collision"');
  expect(delta[1]).toContain('"resourceState":"quarantined"');
  expect(delta[1]).toStartWith(`${allocationPrefix} event `);
  expect(delta[2]).toBe(
    `tell agent:${agentId} worktree_orphaned ${expectedPath} | worktree provisioning failed after physical identity appeared — inspect; never auto-delete`,
  );
  expect(delta.join("\n")).not.toContain("must never reach provider execution");

  execFileSync("git", ["-C", repo, "branch", "-d", "--", branch]);
});

test("typed provider preflight refusal preserves a queryable quarantine with exact recovery", async () => {
  const { spawn } = await import("./support/spawn");
  const agentId = "wt-provider-preflight-fail-1";
  const expectedPath = `/tmp/${require("node:path").basename(repo)}-lane-${agentId}`;
  const events: WorktreeAllocationEvent[] = [];
  process.chdir(repo);
  try {
    const result = await spawn({
      prompt: "typed retry-safe provider preflight refusal",
      agentId,
      worktree: true,
      routingMetadata: presetRequest("integrator"),
      queryFn: () => (async function* () {
        throw new ProviderRetrySafeError("injected_provider_admission_refusal");
      })(),
      feedSubscriber: () => readySubscription(),
      worktreeAllocationWriter: {
        register: () => {},
        event: (_subject: string, event: WorktreeAllocationEvent) => events.push(event),
      },
    });
    expect(result).toBe("");
  } finally {
    process.chdir(origCwd);
  }

  expect(events.map(({ type }) => type)).toEqual([
    "provisioned", "authority-profiled", "quarantined",
  ]);
  expect(events.at(-1)).toMatchObject({
    type: "quarantined",
    resourceState: "quarantined",
    error: { code: "provider_preflight_refused", phase: "provider_admission" },
    recovery: {
      action: "inspect-and-salvage",
      resource: expectedPath,
      durableRef: `refs/heads/lane-${agentId}`,
    },
  });
  expect(existsSync(expectedPath)).toBe(true);

  rmSync(expectedPath, { recursive: true, force: true });
});

test("watchdog termination preserves live-child and quarantined-worktree recovery receipts", async () => {
  const { spawn } = await import("./support/spawn");
  const agentId = "wt-watchdog-recovery-1";
  const expectedPath = `/tmp/${require("node:path").basename(repo)}-lane-${agentId}`;
  const events: WorktreeAllocationEvent[] = [];
  writeFileSync(log, "");
  process.env.NORTH_STALL_MS = "10";
  process.chdir(repo);
  try {
    const result = await spawn({
      prompt: "preserve watchdog recovery receipts",
      agentId,
      worktree: true,
      routingMetadata: presetRequest("integrator"),
      queryFn: () => ({
		executionTransport: "managed-app-server",
        interrupt: async () => {},
        close: async () => { throw new Error("interrupted provider stream closing"); },
        [Symbol.asyncIterator]() {
          return { next: () => new Promise(() => {}) };
        },
      }),
      childSettlementReader: () => ({
        kind: "live",
        children: ["@agent:child-watchdog-1"],
        live: ["@agent:child-watchdog-1"],
      }),
      feedSubscriber: () => readySubscription(),
      worktreeAllocationWriter: {
        register: () => {},
        event: (_subject: string, event: WorktreeAllocationEvent) => events.push(event),
      },
    });
    expect(result).toBe("");
  } finally {
    delete process.env.NORTH_STALL_MS;
    process.chdir(origCwd);
  }

  expect(events.map(({ type }) => type).slice(-3)).toEqual([
    "provisioned", "authority-profiled", "quarantined",
  ]);
  expect(events.at(-1)).toMatchObject({
    type: "quarantined",
    resourceState: "quarantined",
    error: { code: "salvage_required", phase: "finalize" },
    recovery: {
      action: "inspect-and-salvage",
      resource: expectedPath,
      durableRef: `refs/heads/lane-${agentId}`,
    },
  });
  const logged = readFileSync(log, "utf8");
  expect(logged).toContain(`tell agent:${agentId} process_outcome watchdog_aborted`);
  expect(logged).toContain(`tell agent:${agentId} early_exit_children`);
  expect(logged).toContain("child-watchdog-1");
  expect(logged).toContain(`tell agent:${agentId} worktree_orphaned ${expectedPath}`);
  expect(existsSync(expectedPath)).toBe(true);
  rmSync(expectedPath, { recursive: true, force: true });
});
