import { expect, test } from "bun:test";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import {
  BESPOKE_FINGERPRINT_DOMAIN, BESPOKE_FINGERPRINT_VERSION, bespokeContractFingerprint,
} from "../src/bespoke-contract";

const north = resolve(import.meta.dir, "../..");
const cli = resolve(north, "cli/agents-cli.clj");
const gaffer = resolve(north, "../gaffer");
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

function dry(role: string, provider: string, ...extra: string[]): string {
  const result = spawnSync("bb", [cli, "spawn", role, "probe", "--provider", provider, "--dry-run", ...extra], {
    encoding: "utf8", env: { ...process.env, NO_COLOR: "1", GAFFER_HOME: gaffer,
      GAFFER_STAFFING_CATALOG: resolve(gaffer, "staffing/catalog.json") },
  });
  expect(result.status).toBe(0);
  return result.stdout;
}

test("director is the canonical orchestrator role and topology names fail pedagogically", () => {
  for (const provider of ["anthropic", "openai"]) {
    const director = dry("director", provider);
    expect(director).toContain("AGENT_TIER=frontier");
    expect(director).toContain("AGENT_TOPOLOGY=orchestrator");
    expect(director).toContain("AGENT_ROLE=director");
    expect(director).not.toContain("AGENT_MODEL=");
  }
  for (const topology of ["orchestrator", "worker"]) {
    const result = spawnSync("bb", [cli, "spawn", topology, "probe", "--dry-run"], {
      encoding: "utf8", env: { ...process.env, NO_COLOR: "1", GAFFER_HOME: gaffer,
        GAFFER_STAFFING_CATALOG: resolve(gaffer, "staffing/catalog.json") },
    });
    expect(result.status).toBe(1);
    expect(result.stdout).toContain(`${topology} is a topology, not a role`);
  }
});

test("a managed CLI orchestrator can spawn workers but cannot grow another orchestrator tier", () => {
  const run = (...args: string[]) => spawnSync("bb", [cli, "spawn", ...args, "--dry-run"], {
    encoding: "utf8",
    env: {
      ...process.env,
      AGENT_TOPOLOGY: "orchestrator",
      AGENT_ID: "parent-director",
      NO_COLOR: "1",
      GAFFER_HOME: gaffer,
      GAFFER_STAFFING_CATALOG: resolve(gaffer, "staffing/catalog.json"),
    },
  });
  const worker = run("integrator", "bounded worker");
  expect(worker.status).toBe(0);
  expect(worker.stdout).toContain("AGENT_TOPOLOGY=worker");

  const denied = [
    run("director", "role-only nested director"),
    run(
      "director", "overridden nested director", "--tier", "senior",
      "--override-reason", "bounded coordination does not require frontier tier",
    ),
    run(
      "migration-director", "bespoke nested director",
      "--topology", "orchestrator", "--rationale", "one-off coordination shape",
      "--contract", bespokeOrchestratorContract, "--no-promotion-candidate",
    ),
  ];
  for (const result of denied) {
    expect(result.status).toBe(1);
    expect(result.stdout).toContain(
      "coordination depth denied: spawn from an orchestrator may create worker topology only",
    );
  }
});

test("ambiguous researcher role fails with the three explicit research functions", () => {
  const result = spawnSync("bb", [cli, "spawn", "researcher", "probe", "--dry-run"], {
    encoding: "utf8", env: { ...process.env, NO_COLOR: "1", GAFFER_HOME: gaffer,
      GAFFER_STAFFING_CATALOG: resolve(gaffer, "staffing/catalog.json") },
  });
  expect(result.status).toBe(1);
  expect(result.stdout).toContain("researcher is retired because it was ambiguous");
  for (const role of ["scout", "analyst", "research-scientist"]) expect(result.stdout).toContain(role);
});

const delegate = (...args: string[]) => spawnSync("bb", [cli, "delegate", ...args], {
  encoding: "utf8", env: { ...process.env, NO_COLOR: "1", GAFFER_HOME: gaffer,
    GAFFER_STAFFING_CATALOG: resolve(gaffer, "staffing/catalog.json") },
});

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
  expect(result.stdout).toContain("# gaffer dials for role director");
  expect(result.stdout).toContain("AGENT_ROLE=director");
  expect(result.stdout).toContain("AGENT_TOPOLOGY=orchestrator");
  expect(result.stdout).toContain("classified COMPOSITE");
});

test("atomic delegate starts exactly the selected terminal worker and forwards route overrides", () => {
  const result = delegate(
    "apply the bounded fix", "--role", "integrator",
    "--tier", "standard", "--reasoning", "medium",
    "--override-reason", "the implementation boundary is already settled", "--dry-run",
  );
  expect(result.status).toBe(0);
  expect(result.stdout).toContain("# gaffer dials for role integrator");
  expect(result.stdout).toContain("AGENT_ROLE=integrator");
  expect(result.stdout).toContain("AGENT_TOPOLOGY=worker");
  expect(result.stdout).toContain("AGENT_TIER=standard");
  expect(result.stdout).toContain("classified ATOMIC");
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
  expect(result.stdout).toContain("gaffer:bespoke:migration-forensics");
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

test("legacy unclassified delegate no longer silently buys a director", () => {
  const result = spawnSync("bb", [cli, "delegate", "coordinate this", "--dry-run"], {
    encoding: "utf8", env: { ...process.env, NO_COLOR: "1", GAFFER_HOME: gaffer,
      GAFFER_STAFFING_CATALOG: resolve(gaffer, "staffing/catalog.json") },
  });
  expect(result.status).toBe(1);
  expect(result.stdout).not.toContain("# gaffer dials for role director");
});

test("judge is the premium high-leverage verdict role", () => {
  const judge = dry("judge", "openai");
  expect(judge).toContain("AGENT_TIER=frontier");
  expect(judge).toContain("AGENT_REASONING=xhigh");
});

test("bespoke roles require a structured contract and explicit promotion decision", () => {
  const ordinary = dry("migration-forensics", "openai", "--rationale", "one-off probe",
    "--contract", bespokeContract);
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
  expect(nearest).toContain("gaffer:bespoke:migration-cartographer");
  expect(nearest).not.toContain("schema archaeology");
  expect(nearest).not.toContain("timeline and gaps");
});

test("bespoke help is discoverable and invalid bespoke inputs exit nonzero", () => {
  const run = (...args: string[]) => spawnSync("bb", [cli, "spawn", ...args], {
    encoding: "utf8", env: { ...process.env, NO_COLOR: "1", GAFFER_HOME: gaffer,
      GAFFER_STAFFING_CATALOG: resolve(gaffer, "staffing/catalog.json") },
  });
  const help = run();
  expect(help.stdout).toContain("--nearest PRESET");
  expect(help.stdout).toContain("--promotion-candidate");
  expect(help.stdout).toContain("--contract JSON|@file");
  expect(help.stdout).toContain("first-class bespoke compositions");
  for (const result of [
    run("one-off", "probe", "--dry-run"),
    run("one-off", "probe", "--nearest", "missing", "--rationale", "special", "--contract", bespokeContract,
      "--no-promotion-candidate", "--dry-run"),
    run("scout", "probe", "--topology", "verifier", "--dry-run"),
    run("director", "probe", "--topology", "worker", "--override-reason", "contradiction", "--dry-run"),
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
