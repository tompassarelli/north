import { expect, test } from "bun:test";
import {
  chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  BESPOKE_FINGERPRINT_DOMAIN, BESPOKE_FINGERPRINT_VERSION, bespokeContractFingerprint,
} from "../src/bespoke-contract";
import { admitPinnedProvider } from "../src/execution-admission";

const north = resolve(import.meta.dir, "../..");
const cli = resolve(north, "cli/agents-cli.clj");
const orchestration = resolve(north, "orchestration");
const CLI_PROCESS_TEST_TIMEOUT_MS = 45_000;
const bespokeContract = JSON.stringify({
  responsibility: "reconstruct migration provenance", deliverable: "evidence-linked timeline",
  capabilities: ["filesystem.read", "filesystem.search", "shell.readonly"],
  mayDecide: ["read-only traces"], mustEscalate: ["destructive recovery"],
  doneWhen: ["every transition is sourced"], report: "timeline and gaps",
});
const bespokeOrchestratorContract = JSON.stringify({
  responsibility: "coordinate a bounded migration",
  deliverable: "integrated migration result",
  capabilities: ["coordination", "filesystem.read", "filesystem.search", "shell.readonly"],
  mayDecide: ["worker decomposition"], mustEscalate: ["scope expansion"],
  doneWhen: ["all worker results are reconciled"], report: "integrated verdict",
});

function pinEvidence(...pins: Array<{ kind: "provider" | "account" | "model"; value: string }>): string {
  const issuedAt = new Date();
  return JSON.stringify({
    policyVersion: "north-routing-pin-v1",
    issuedAt: issuedAt.toISOString(),
    expiresAt: new Date(issuedAt.getTime() + 60 * 60 * 1000).toISOString(),
    reasonCode: "explicit-human-request",
    detail: "agents CLI routing fixture",
    pins,
  });
}

function providerPin(provider: string): string[] {
  return ["--pin-evidence", pinEvidence({ kind: "provider", value: provider })];
}

function dry(role: string, provider: string, ...extra: string[]): string {
  const result = spawnSync("bb", [
    // --ad-hoc: these assert ROUTING, not attribution. Spawn now refuses a run
    // that names neither a thread nor --ad-hoc, so routing fixtures declare the
    // unattributed intent explicitly rather than relying on a silent default.
    cli, "spawn", role, "probe", "--provider", provider, ...providerPin(provider),
    "--ad-hoc", "--dry-run", ...extra,
  ], {
    encoding: "utf8", env: { ...process.env, NO_COLOR: "1", NORTH_ORCHESTRATION_HOME: orchestration,
      ORCHESTRATION_STAFFING_CATALOG: resolve(orchestration, "staffing/catalog.json") },
  });
  expect(result.status).toBe(0);
  return result.stdout;
}

test("director is the canonical orchestrator role and topology names fail pedagogically", () => {
  const director = dry("director", "anthropic");
  expect(director).toContain("AGENT_TIER=frontier");
  expect(director).toContain("AGENT_TOPOLOGY=orchestrator");
  expect(director).toContain("AGENT_ROLE=director");
  expect(director).not.toContain("AGENT_MODEL=");
  for (const topology of ["orchestrator", "worker"]) {
    const result = spawnSync("bb", [cli, "spawn", topology, "probe", "--ad-hoc", "--dry-run"], {
      encoding: "utf8", env: { ...process.env, NO_COLOR: "1", NORTH_ORCHESTRATION_HOME: orchestration,
        ORCHESTRATION_STAFFING_CATALOG: resolve(orchestration, "staffing/catalog.json") },
    });
    expect(result.status).toBe(1);
    expect(result.stdout).toContain(`${topology} is a topology, not a role`);
  }
}, CLI_PROCESS_TEST_TIMEOUT_MS);

test("CLI dry preview uses the exact topology policy selected for execution", () => {
  expect(dry("integrator", "openai")).toContain(
    "policy=north:struggle-observer:v1 topology=worker error-streak=3 loop-repeat=3 loop-window=20 no-progress-turns=6",
  );
  expect(dry("director", "anthropic")).toContain(
    "policy=north:struggle-observer:v1 topology=orchestrator error-streak=3 loop-repeat=3 loop-window=20 no-progress-turns=12",
  );

  const overridden = spawnSync("bb", [
    cli, "spawn", "director", "probe", "--provider", "anthropic",
    ...providerPin("anthropic"), "--ad-hoc", "--dry-run",
  ], {
    encoding: "utf8",
    env: {
      ...process.env,
      NO_COLOR: "1",
      NORTH_ORCHESTRATION_HOME: orchestration,
      ORCHESTRATION_STAFFING_CATALOG: resolve(orchestration, "staffing/catalog.json"),
      STRUGGLE_ERROR_STREAK: "5",
      STRUGGLE_LOOP_REPEAT: "4",
      STRUGGLE_LOOP_WINDOW: "30",
      STRUGGLE_STALL_TURNS: "9",
      STRUGGLE_STALL_TURNS_ORCHESTRATOR: "18",
    },
  });
  expect(overridden.status).toBe(0);
  expect(overridden.stdout).toContain(
    "topology=orchestrator error-streak=5 loop-repeat=4 loop-window=30 no-progress-turns=18",
  );
  // The full JSON witness is deliberately not printed in the shell command.
  expect(overridden.stdout).not.toContain("NORTH_STRUGGLE_POLICY_EXPECTED");

  const invalid = spawnSync("bb", [
    cli, "spawn", "integrator", "probe", "--provider", "openai",
    ...providerPin("openai"), "--ad-hoc", "--dry-run",
  ], {
    encoding: "utf8",
    env: {
      ...process.env,
      NO_COLOR: "1",
      NORTH_ORCHESTRATION_HOME: orchestration,
      ORCHESTRATION_STAFFING_CATALOG: resolve(orchestration, "staffing/catalog.json"),
      STRUGGLE_STALL_TURNS: "0",
    },
  });
  expect(invalid.status).toBe(1);
  expect(invalid.stderr).toContain("positive integer between 1 and 1000");
  expect(invalid.stdout).not.toContain("[dry-run]");
}, CLI_PROCESS_TEST_TIMEOUT_MS);

test("a managed CLI orchestrator without an exact parent reservation fails safe before recursive spawn", () => {
  const run = (...args: string[]) => spawnSync("bb", [cli, "spawn", ...args, "--ad-hoc", "--dry-run"], {
    encoding: "utf8",
    env: {
      ...process.env,
      AGENT_TOPOLOGY: "orchestrator",
      AGENT_ID: "parent-director",
      NO_COLOR: "1",
      NORTH_ORCHESTRATION_HOME: orchestration,
      ORCHESTRATION_STAFFING_CATALOG: resolve(orchestration, "staffing/catalog.json"),
    },
  });
  const refused = [
    run("integrator", "bounded worker"),
    run("director", "role-only nested director"),
    run(
      "director", "overridden nested director", "--tier", "senior",
      "--reasoning", "high",
      "--override-reason", "bounded coordination does not require frontier tier",
    ),
    run(
      "migration-director", "bespoke nested director",
      "--task-grade", "staff", "--tier", "senior",
      "--reasoning", "high", "--posture", "deliver",
      "--topology", "orchestrator", "--rationale", "one-off coordination shape",
      "--contract", bespokeOrchestratorContract, "--no-promotion-candidate",
    ),
  ];
  for (const result of refused) {
    expect(result.status).toBe(1);
    expect(result.stdout).toContain("recursive orchestrator spawn requires its exact parent run/thread reservation");
    expect(result.stdout).not.toContain("coordination depth denied");
  }
}, CLI_PROCESS_TEST_TIMEOUT_MS);

test("ambiguous researcher role fails with the three explicit research functions", () => {
  const result = spawnSync("bb", [cli, "spawn", "researcher", "probe", "--ad-hoc", "--dry-run"], {
    encoding: "utf8", env: { ...process.env, NO_COLOR: "1", NORTH_ORCHESTRATION_HOME: orchestration,
      ORCHESTRATION_STAFFING_CATALOG: resolve(orchestration, "staffing/catalog.json") },
  });
  expect(result.status).toBe(1);
  expect(result.stdout).toContain("researcher is retired because it was ambiguous");
  for (const role of ["scout", "analyst", "scientist"]) expect(result.stdout).toContain(role);
});

const delegate = (...args: string[]) => spawnSync("bb", [cli, "delegate", ...args], {
  encoding: "utf8", env: { ...process.env, NO_COLOR: "1", NORTH_ORCHESTRATION_HOME: orchestration,
    ORCHESTRATION_STAFFING_CATALOG: resolve(orchestration, "staffing/catalog.json") },
});

test("composite preview and execution share pinned-provider admission before side effects", () => {
  const directory = mkdtempSync(join(tmpdir(), "north-provider-admission-parity-"));
  const home = join(directory, "home");
  const calls = join(directory, "side-effects.log");
  const fakeNorth = join(directory, "north-fake");
  mkdirSync(home);
  const planted = (name: string) => {
    const file = join(directory, name);
    writeFileSync(file, `#!/usr/bin/env bash\nprintf '%s\\n' ${JSON.stringify(name)} >> ${JSON.stringify(calls)}\nexit 97\n`);
    chmodSync(file, 0o755);
    return file;
  };
  planted("claude");
  planted("codex");
  writeFileSync(
    fakeNorth,
    `#!/usr/bin/env bash\nprintf 'north %s\\n' "$*" >> ${JSON.stringify(calls)}\nexit 97\n`,
  );
  chmodSync(fakeNorth, 0o755);

  const capabilities = [
    "filesystem.read", "filesystem.search", "shell.readonly", "web", "coordination",
  ] as const;
  let productionReason = "";
  try {
    admitPinnedProvider("openai", capabilities);
  } catch (error) {
    productionReason = error instanceof Error ? error.message : String(error);
  }
  expect(productionReason).toBe("");

  const env = {
    ...process.env,
    HOME: home,
    PATH: `${directory}:${process.env.PATH ?? ""}`,
    NORTH_HOME: north,
    NORTH_BIN: fakeNorth,
    NORTH_POLICY_BUN: process.execPath,
    NORTH_RUN_ID: "",
    NORTH_THREAD_ID: "",
    NORTH_RUN_CAPABILITY: "",
    NO_COLOR: "1",
    NORTH_ORCHESTRATION_HOME: orchestration,
    ORCHESTRATION_STAFFING_CATALOG: resolve(orchestration, "staffing/catalog.json"),
  };
  try {
    const requests = [
      ["delegate", "coordinate this", "--composite"],
      ["spawn", "director", "coordinate this", "--ad-hoc"],
    ];
    for (const request of requests) {
      const result = spawnSync("bb", [
        cli, ...request,
        "--provider", "openai", "--target", "codex-work",
        "--pin-evidence", pinEvidence(
          { kind: "provider", value: "openai" },
          { kind: "account", value: "codex-work" },
        ),
        "--ad-hoc", "--dry-run",
      ], { encoding: "utf8", env });
      expect(result.status, result.stderr).toBe(0);
      expect(result.stdout).toContain("[dry-run]");
      expect(result.stdout).toContain("semantic handle");
      expect(result.stdout).not.toContain("blocked_preflight");
    }

    expect(existsSync(calls)).toBe(false);
    expect(existsSync(join(home, ".local/state/north/accounts"))).toBe(false);
    expect(existsSync(join(home, ".local/state/north/agents"))).toBe(false);

    const anthropic = spawnSync("bb", [
      cli, "delegate", "coordinate this", "--composite",
      "--provider", "anthropic", "--target", "claude-work",
      "--pin-evidence", pinEvidence(
        { kind: "provider", value: "anthropic" },
        { kind: "account", value: "claude-work" },
      ),
      "--dry-run",
    ], { encoding: "utf8", env });
    expect(anthropic.status).toBe(0);
    expect(anthropic.stdout).toContain("# orchestration dials for role director");
    expect(anthropic.stdout).toContain("AGENT_ROLE=director");
    expect(anthropic.stdout).toContain("AGENT_TOPOLOGY=orchestrator");
    expect(anthropic.stdout).toContain("NORTH_DELEGATE_THREAD_ID=capture-on-execution");
    expect(anthropic.stdout).toContain("[dry-run] not executed. semantic handle would be");
    expect(existsSync(calls)).toBe(false);
    expect(existsSync(join(home, ".local/state/north/accounts"))).toBe(false);
    expect(existsSync(join(home, ".local/state/north/agents"))).toBe(false);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}, CLI_PROCESS_TEST_TIMEOUT_MS);

test("delegate requires one explicit dependency-shape classification", () => {
  for (const result of [
    delegate("classify me", "--dry-run"),
    delegate("contradiction", "--role", "integrator", "--composite", "--dry-run"),
    delegate("orchestrator disguised as atomic", "--role", "director", "--dry-run"),
  ]) {
    expect(result.status).toBe(1);
  }
  expect(delegate("classify me", "--dry-run").stdout).toContain("--role for atomic work or --composite");
});

test("composite delegate alone hydrates the canonical director preset", () => {
  const result = delegate("coordinate this", "--composite", "--dry-run");
  expect(result.status).toBe(0);
  expect(result.stdout).toContain("# orchestration dials for role director");
  expect(result.stdout).toContain("AGENT_ROLE=director");
  expect(result.stdout).toContain("AGENT_TOPOLOGY=orchestrator");
  expect(result.stdout).toContain("COMPOSITE INTAKE");
  expect(result.stdout).toContain("NORTH_DELEGATE_THREAD_ID=capture-on-execution");
  expect(result.stdout).toContain("child thread linked `part_of @capture-on-execution`");
});

test("atomic delegate starts exactly the selected terminal worker and forwards route overrides", () => {
  const result = delegate(
    "apply the bounded fix", "--role", "integrator",
    "--tier", "standard", "--reasoning", "medium",
    "--override-reason", "the implementation boundary is already settled", "--dry-run",
  );
  expect(result.status).toBe(0);
  expect(result.stdout).toContain("# orchestration dials for role integrator");
  expect(result.stdout).toContain("AGENT_ROLE=integrator");
  expect(result.stdout).toContain("AGENT_TOPOLOGY=worker");
  expect(result.stdout).toContain("AGENT_TIER=standard");
  expect(result.stdout).toContain("ATOMIC INTAKE");
  expect(result.stdout).toContain("return one evidence-backed result");
  expect(result.stdout).not.toContain("You are the DIRECTOR");
});

test("atomic delegate forwards first-class bespoke composition options", () => {
  const result = delegate(
    "reconstruct the migration", "--role", "migration-forensics",
    "--nearest", "analyst", "--rationale", "one-off provenance reconstruction",
    "--contract", bespokeContract, "--no-promotion-candidate", "--dry-run",
  );
  expect(result.status).toBe(0);
  expect(result.stdout).toContain("AGENT_ROLE=migration-forensics");
  expect(result.stdout).toContain("orchestration:bespoke:migration-forensics");
  expect(result.stdout).toContain("AGENT_COMPOSITION=REDACTED_BESPOKE_CONTRACT");
  expect(result.stdout).not.toContain('"nearestPreset":"analyst"');
});

test("delegate context remains an orthogonal handoff payload", () => {
  const directory = mkdtempSync(join(tmpdir(), "north-delegate-context-"));
  const context = join(directory, "brief.md");
  writeFileSync(context, "settled fact: use the canonical parser");
  const result = delegate("finish parser", "--role", "implementer", "--context", context, "--dry-run");
  rmSync(directory, { recursive: true, force: true });
  expect(result.status).toBe(0);
  expect(result.stdout).toContain("CONTEXT BRIEF:");
  expect(result.stdout).toContain("settled fact: use the canonical parser");
});

test("delegate prompt keeps North deltas and does not duplicate canonical Orchestration law", () => {
  const result = delegate("coordinate this", "--composite", "--dry-run");
  expect(result.status).toBe(0);
  for (const duplicate of [
    "STOP-RULE",
    "A director never executes",
    "Decide worker tiers independently",
    "You are read-only by contract",
    "verification is a sibling lane",
  ]) {
    expect(result.stdout).not.toContain(duplicate);
  }
  for (const northDelta of [
    "aggregate reduction/checkpoint thread",
    "North listener/continuation",
    "reconcile every child",
    "north evidence record",
  ]) {
    expect(result.stdout).toContain(northDelta);
  }
});

function writeThreadFake(
  directory: string,
  receipt: Record<string, unknown>,
  title: string,
  runFacts?: Array<{ predicate: string; value: string }>,
  recursiveParent?: string,
): { command: string; calls: string; showRows: Record<string, string[][]> } {
  const command = join(directory, "north-fake");
  const calls = join(directory, "calls.log");
  const threadRows = [
    ["title", title],
    ["kind", "thread"],
    ["committed", "2026-07-19"],
    ...(recursiveParent ? [["part_of", `@${recursiveParent}`]] : []),
  ];
  const showRows: Record<string, string[][]> = {
    [`@${String(receipt.id)}`]: threadRows,
  };
  if (recursiveParent) showRows[`@${recursiveParent}`] = threadRows;
  if (runFacts) {
    showRows["@run-parent"] = runFacts.map(({ predicate, value }) => [predicate, value]);
  }
  writeFileSync(command, `#!/usr/bin/env bash
printf '%s\\n' "$*" >> ${JSON.stringify(calls)}
if [ "$1" = "capture" ]; then
  printf '%s\\n' ${JSON.stringify(JSON.stringify(receipt))}
  exit 0
fi
if [ "$1" = "tell" ]; then
  exit 0
fi
exit 1
`);
  chmodSync(command, 0o755);
  return { command, calls, showRows };
}

function reservationFacts(
  thread: string,
  agent: string,
  capability: string,
): Array<{ predicate: string; value: string }> {
  const projection: Record<string, string> = {
    run_capability_sha256: createHash("sha256").update(capability).digest("hex"),
    run_reservation_agent: `@agent:${agent}`,
    run_reservation_contract_origin: "worker-defined",
    run_reservation_done_when: "[]",
    run_reservation_thread: `@${thread}`,
    run_reservation_version: "north:run-reservation:v1",
    run_reserved_at: "2026-07-19T00:00:00.000Z",
  };
  const canonical = Object.entries(projection)
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([predicate, value]) => `${predicate}\0${value}\n`)
    .join("");
  return [
    ...Object.entries(projection).map(([predicate, value]) => ({ predicate, value })),
    {
      predicate: "run_reservation_manifest_sha256",
      value: createHash("sha256").update(canonical).digest("hex"),
    },
  ];
}

function resolveDelegateThread(
  request: { task: string; explicit?: string },
  env: Record<string, string>,
  fake: ReturnType<typeof writeThreadFake>,
) {
  const expression = [
    `(System/setProperty "north.agents.lib" "1")`,
    `(load-file ${JSON.stringify(cli)})`,
    `(let [fixture-rows (json/parse-string ${JSON.stringify(JSON.stringify(fake.showRows))})]`,
    `(with-redefs [north.coord/show-rows`,
    `(fn [_port subject]`,
    `(spit ${JSON.stringify(fake.calls)} (str "show " subject "\\n") :append true)`,
    `(get fixture-rows subject []))]`,
    `(println (json/generate-string (resolve-delegate-thread! {:task ${JSON.stringify(request.task)} :explicit-thread ${request.explicit ? JSON.stringify(request.explicit) : "nil"}} false)))))`,
  ].join(" ");
  return spawnSync("bb", ["-e", expression], {
    encoding: "utf8",
    cwd: north,
    env: {
      ...process.env,
      NORTH_AGENTS_LIB: "1",
      NO_COLOR: "1",
      ...env,
    },
  });
}

test("delegate captures exactly one thread when no explicit or managed binding exists", () => {
  const directory = mkdtempSync(join(tmpdir(), "north-delegate-capture-"));
  try {
    const task = "proof-bound task";
    const receipt = {
      id: "captured-thread",
      thread: "@captured-thread",
      title: task,
      path: "/tmp/captured-thread.md",
      expected: 7,
      committed: 7,
      complete: true,
      reason: "captured",
    };
    const fake = writeThreadFake(directory, receipt, task);
    const result = resolveDelegateThread({ task }, {
      NORTH_BIN: fake.command,
      NORTH_RUN_ID: "",
      NORTH_THREAD_ID: "",
      NORTH_RUN_CAPABILITY: "",
    }, fake);
    expect(result.status).toBe(0);
    expect(JSON.parse(result.stdout.trim())).toMatchObject({
      id: "captured-thread",
      source: "captured",
    });
    const calls = readFileSync(fake.calls, "utf8").trim().split("\n");
    expect(calls.filter((line) => line.startsWith("capture "))).toEqual([`capture ${task}`]);
    expect(calls.filter((line) => line === "show @captured-thread")).toHaveLength(1);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("delegate derives a bounded single-line capture title without truncating the worker task", () => {
  const directory = mkdtempSync(join(tmpdir(), "north-delegate-title-"));
  try {
    const meaningfulLine = `  Design\t  ${"界".repeat(80)} trailing detail  `;
    const task = `\r\n${meaningfulLine}\u2028Second line must remain in the worker prompt.`;
    const collapsed = meaningfulLine.trim().replace(/\s+/g, " ");
    let title = "";
    for (const point of collapsed) {
      if (Buffer.byteLength(title + point, "utf8") > 160) break;
      title += point;
    }
    const receipt = {
      id: "bounded-title-thread",
      thread: "@bounded-title-thread",
      title,
      path: "/tmp/bounded-title-thread.md",
      expected: 7,
      committed: 7,
      complete: true,
      reason: "captured",
    };
    const fake = writeThreadFake(directory, receipt, title);
    const result = resolveDelegateThread({ task }, {
      NORTH_BIN: fake.command,
      NORTH_RUN_ID: "",
      NORTH_THREAD_ID: "",
      NORTH_RUN_CAPABILITY: "",
    }, fake);
    expect(result.status).toBe(0);
    expect(Buffer.byteLength(title, "utf8")).toBeLessThanOrEqual(160);
    expect(title).not.toContain("\n");
    expect(title).not.toContain("\u2028");
    expect(readFileSync(fake.calls, "utf8")).toContain(`capture ${title}\n`);

    const briefExpression = [
      `(System/setProperty "north.agents.lib" "1")`,
      `(load-file ${JSON.stringify(cli)})`,
      `(print (delegate-brief {:task ${JSON.stringify(task)} :mode :atomic}`,
      `{:id "bounded-title-thread" :committed? true :done-when []}))`,
    ].join(" ");
    const brief = spawnSync("bb", ["-e", briefExpression], {
      encoding: "utf8",
      cwd: north,
      env: { ...process.env, NORTH_AGENTS_LIB: "1", NO_COLOR: "1" },
    });
    expect(brief.status).toBe(0);
    expect(brief.stdout).toContain(task);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("delegate explicit binding reuses its thread while a managed parent receives a fresh linked child", () => {
  const directory = mkdtempSync(join(tmpdir(), "north-delegate-reuse-"));
  try {
    const childReceipt = {
      id: "fresh-child-thread",
      thread: "@fresh-child-thread",
      title: "child task",
      path: "/tmp/fresh-child-thread.md",
      expected: 7,
      committed: 7,
      complete: true,
      reason: "captured",
    };
    const fake = writeThreadFake(
      directory,
      childReceipt,
      "child task",
      reservationFacts("existing-thread", "parent-agent", "capability"),
      "existing-thread",
    );
    const explicit = resolveDelegateThread({ task: "child task", explicit: "@existing-thread" }, {
      NORTH_BIN: fake.command,
      NORTH_RUN_ID: "",
      NORTH_THREAD_ID: "",
      NORTH_RUN_CAPABILITY: "",
    }, fake);
    expect(explicit.status).toBe(0);
    expect(JSON.parse(explicit.stdout.trim())).toMatchObject({
      id: "existing-thread",
      source: "explicit",
    });
    const dryCliExpression = [
      `(System/setProperty "north.agents.lib" "1")`,
      `(load-file ${JSON.stringify(cli)})`,
      `(let [fixture-rows (json/parse-string ${JSON.stringify(JSON.stringify(fake.showRows))})]`,
      `(with-redefs [north.coord/show-rows`,
      `(fn [_port subject]`,
      `(spit ${JSON.stringify(fake.calls)} (str "show " subject "\\n") :append true)`,
      `(get fixture-rows subject []))]`,
      `(cmd-delegate (json/parse-string`,
      `${JSON.stringify(JSON.stringify([
        "child task", "--role", "integrator",
        "--thread", "@existing-thread", "--dry-run",
      ]))}))))`,
    ].join(" ");
    const dryCli = spawnSync("bb", ["-e", dryCliExpression], {
      encoding: "utf8",
      env: {
        ...process.env,
        NORTH_BIN: fake.command,
        NO_COLOR: "1",
        NORTH_ORCHESTRATION_HOME: orchestration,
        ORCHESTRATION_STAFFING_CATALOG: resolve(orchestration, "staffing/catalog.json"),
      },
    });
    expect(dryCli.status).toBe(0);
    expect(dryCli.stdout).toContain("NORTH_DELEGATE_THREAD_ID=existing-thread");
    expect(dryCli.stdout).toContain("prebound this accepted, currently barless thread to @existing-thread");
    const inherited = resolveDelegateThread({ task: "child task" }, {
      NORTH_BIN: fake.command,
      NORTH_RUN_ID: "run-parent",
      NORTH_THREAD_ID: "existing-thread",
      NORTH_RUN_CAPABILITY: "capability",
      AGENT_ID: "parent-agent",
    }, fake);
    expect(inherited.status).toBe(0);
    expect(JSON.parse(inherited.stdout.trim())).toMatchObject({
      id: "fresh-child-thread",
      source: "recursive-child",
      parent: "existing-thread",
    });
    const calls = readFileSync(fake.calls, "utf8");
    expect(calls).toContain("show @run-parent");
    expect(calls).toContain("show @fresh-child-thread");
    expect(calls).toContain("capture child task");
    expect(calls).toContain(
      "tell fresh-child-thread part_of existing-thread",
    );
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("a partial structured capture fails closed before any provider process starts", () => {
  const directory = mkdtempSync(join(tmpdir(), "north-delegate-partial-"));
  try {
    const task = "must not spawn";
    const fake = writeThreadFake(directory, {
      id: "partial-thread",
      thread: "@partial-thread",
      title: task,
      path: "/tmp/partial-thread.md",
      expected: 7,
      committed: 3,
      complete: false,
      reason: "partial-cleaned",
    }, task);
    const bunLog = join(directory, "bun.log");
    const fakeBun = join(directory, "bun");
    writeFileSync(fakeBun, `#!/usr/bin/env bash\nprintf '%s\\n' "$*" >> ${JSON.stringify(bunLog)}\nexit 0\n`);
    chmodSync(fakeBun, 0o755);
    const result = spawnSync("bb", [
      cli, "delegate", task, "--role", "integrator",
    ], {
      encoding: "utf8",
      env: {
        ...process.env,
        PATH: `${directory}:${process.env.PATH}`,
        NORTH_POLICY_BUN: process.execPath,
        NORTH_BIN: fake.command,
        NORTH_RUN_ID: "",
        NORTH_THREAD_ID: "",
        NORTH_RUN_CAPABILITY: "",
        NO_COLOR: "1",
        NORTH_ORCHESTRATION_HOME: orchestration,
        ORCHESTRATION_STAFFING_CATALOG: resolve(orchestration, "staffing/catalog.json"),
      },
    });
    expect(result.status).toBe(1);
    expect(result.stdout).toContain("capture was partial");
    expect(() => readFileSync(bunLog, "utf8")).toThrow();
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("legacy unclassified delegate no longer silently buys a director", () => {
  const result = spawnSync("bb", [cli, "delegate", "coordinate this", "--dry-run"], {
    encoding: "utf8", env: { ...process.env, NO_COLOR: "1", NORTH_ORCHESTRATION_HOME: orchestration,
      ORCHESTRATION_STAFFING_CATALOG: resolve(orchestration, "staffing/catalog.json") },
  });
  expect(result.status).toBe(1);
  expect(result.stdout).not.toContain("# orchestration dials for role director");
});

test("judge is the premium high-leverage verdict role", () => {
  const judge = dry("judge", "openai");
  expect(judge).toContain("AGENT_TIER=frontier");
  expect(judge).toContain("AGENT_REASONING=xhigh");
});

test("bespoke roles require a structured contract and explicit promotion decision", () => {
  const ordinary = dry("migration-forensics", "openai", "--rationale", "one-off probe",
    "--contract", bespokeContract, "--task-grade", "senior", "--tier", "senior",
    "--reasoning", "high", "--posture", "explore", "--topology", "worker");
  expect(ordinary).toContain("AGENT_COMPOSITION=REDACTED_BESPOKE_CONTRACT");
  expect(ordinary).toContain(`version=${BESPOKE_FINGERPRINT_VERSION}`);
  expect(ordinary).toContain(`domain=${BESPOKE_FINGERPRINT_DOMAIN}`);
  expect(ordinary).toContain(`sha256=${bespokeContractFingerprint(JSON.parse(bespokeContract))}`);
  expect(ordinary).not.toContain("reconstruct migration provenance");
  expect(ordinary).not.toContain("one-off probe");
  expect(dry("migration-forensics", "openai", "--nearest", "analyst", "--rationale", "one-off probe",
    "--contract", bespokeContract, "--promotion-candidate"))
    .toContain("reason=recorded");
  const nearest = dry("migration-cartographer", "openai", "--nearest", "analyst", "--rationale", "schema archaeology",
    "--contract", bespokeContract, "--no-promotion-candidate");
  expect(nearest).toContain('AGENT_ROLE=migration-cartographer');
  expect(nearest).toContain("AGENT_COMPOSITION=REDACTED_BESPOKE_CONTRACT");
  expect(nearest).toContain("orchestration:bespoke:migration-cartographer");
  expect(nearest).not.toContain("schema archaeology");
  expect(nearest).not.toContain("timeline and gaps");
}, CLI_PROCESS_TEST_TIMEOUT_MS);

test("CLI effective-authority closure rejects open shell capability sets", () => {
  const contract = (capabilities: string[]) => JSON.stringify({
    responsibility: "probe composition ingress",
    deliverable: "a closure verdict",
    capabilities,
    mayDecide: ["which fixture to run"],
    mustEscalate: ["any runtime side effect"],
    doneWhen: ["ingress returns a precise diagnostic"],
    report: "the validation result",
  });
  for (const [capabilities, diagnostic] of [
    [
      ["filesystem.search", "filesystem.write", "shell"],
      "shell requires filesystem.read capability",
    ],
    [
      ["filesystem.read", "filesystem.search", "shell"],
      "shell requires filesystem.write capability",
    ],
    [
      ["filesystem.search", "shell.readonly"],
      "shell.readonly requires filesystem.read capability",
    ],
    [
      ["filesystem.read", "shell.readonly"],
      "shell.readonly requires filesystem.search capability",
    ],
  ] as const) {
    const result = spawnSync("bb", [
      cli, "spawn", "authority-probe", "probe closure",
      "--rationale", "no preset fits this regression",
      "--contract", contract([...capabilities]),
      "--task-grade", "senior", "--topology", "worker", "--tier", "senior",
      "--reasoning", "high", "--posture", "preserve",
      "--no-promotion-candidate", "--ad-hoc", "--dry-run",
    ], {
      encoding: "utf8",
      env: {
        ...process.env,
        NO_COLOR: "1",
        NORTH_ORCHESTRATION_HOME: orchestration,
        ORCHESTRATION_STAFFING_CATALOG: resolve(orchestration, "staffing/catalog.json"),
      },
    });
    expect(result.status).toBe(1);
    expect(result.stdout).toContain(diagnostic);
  }
});

test("a managed spawn must declare its thread attribution or explicit ad-hoc intent", () => {
  const spawn = (...args: string[]) => spawnSync("bb", [cli, "spawn", ...args], {
    encoding: "utf8", env: { ...process.env, NO_COLOR: "1", NORTH_ORCHESTRATION_HOME: orchestration,
      ORCHESTRATION_STAFFING_CATALOG: resolve(orchestration, "staffing/catalog.json") },
  });

  // Naming neither is REFUSED. An unattributed run's wall time and tokens can
  // never roll up to a workstream, and 244 of 303 historical runs were lost to
  // exactly this silent default.
  const bare = spawn("implementer", "probe", "--dry-run");
  expect(bare.status).toBe(2);
  expect(`${bare.stdout}${bare.stderr}`).toContain("requires --thread");

  // Naming BOTH is refused too — the intent is ambiguous, not additive.
  const both = spawn("implementer", "probe", "--thread", "019f0000-0000-7000-8000-000000000000",
    "--ad-hoc", "--dry-run");
  expect(both.status).toBe(2);

  // Bare `spawn` with no role/prompt is the usage path and must still help.
  const help = spawn();
  expect(help.status).toBe(0);
  expect(help.stdout).toContain("--nearest PRESET");
});

test("bespoke help is discoverable and invalid bespoke inputs exit nonzero", () => {
  const run = (...args: string[]) => spawnSync("bb", [cli, "spawn", ...args], {
    encoding: "utf8", env: { ...process.env, NO_COLOR: "1", NORTH_ORCHESTRATION_HOME: orchestration,
      ORCHESTRATION_STAFFING_CATALOG: resolve(orchestration, "staffing/catalog.json") },
  });
  const help = run();
  expect(help.stdout).toContain("--nearest PRESET");
  expect(help.stdout).toContain("--promotion-candidate");
  expect(help.stdout).toContain("--contract JSON|@file");
  expect(help.stdout).toContain("first-class bespoke compositions");
  for (const result of [
    run("one-off", "probe", "--ad-hoc", "--dry-run"),
    run("one-off", "probe", "--nearest", "missing", "--rationale", "special", "--contract", bespokeContract,
      "--no-promotion-candidate", "--ad-hoc", "--dry-run"),
    run("scout", "probe", "--topology", "verifier", "--ad-hoc", "--dry-run"),
    run("director", "probe", "--topology", "worker", "--override-reason", "contradiction", "--ad-hoc", "--dry-run"),
  ]) expect(result.status).toBe(1);
});

test("agent roster facts fold coordination and telemetry logs together", () => {
  const directory = mkdtempSync(join(tmpdir(), "north-agent-split-"));
  try {
    const coordination = join(directory, "coordination.log");
    const telemetry = join(directory, "telemetry.log");
    writeFileSync(coordination, '{:tx 1 :op "assert" :l "@agent:coord" :p "display_name" :r "coord-name"}\n');
    writeFileSync(telemetry, '{:tx 2 :op "assert" :l "@agent:telemetry" :p "display_name" :r "telemetry-name"}\n');
    const expression = `(load-file ${JSON.stringify(cli)}) (println (cheshire.core/generate-string (agent-facts)))`;
    const result = spawnSync("bb", ["-e", expression], {
      encoding: "utf8", cwd: north,
      env: { ...process.env, NORTH_AGENTS_LIB: "1", FRAM_LOG: coordination,
        FRAM_TELEMETRY_LOG: telemetry, FRAM_PORT: "59998", NO_COLOR: "1" },
    });
    expect(result.status).toBe(0);
    const facts = JSON.parse(result.stdout.trim());
    expect(facts.coord.display_name).toBe("coord-name");
    expect(facts.telemetry.display_name).toBe("telemetry-name");
  } finally { rmSync(directory, { recursive: true, force: true }); }
});
