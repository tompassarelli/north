// P1+P2 prompt-assembly: capability-gated tiered constitution + cache-first
// (unique-last) ordering. These assert the per-capability-class section matrix,
// byte-identity of the shared tiers for same-class lanes, and that the per-lane
// UNIQUE coordination tail lands after every shared tier.
import { afterEach, expect, test } from "bun:test";
import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { constitutionTiers, harnessCompositionEvidence, harnessOptions } from "../src/harness";
import { applyOrchestrationStaffing } from "../src/orchestration-staffing";
import type { OrchestrationCapability } from "../src/orchestration-capabilities";

const north = resolve(import.meta.dir, "../..");
const B = (s: string) => Buffer.byteLength(s, "utf8");

// Coarse capability classes (catalog-consistent). Read-only classes carry no
// filesystem.write, shell.readonly only, and no coordination.
const CLASS: Record<string, OrchestrationCapability[]> = {
  roEval: ["filesystem.read", "filesystem.search", "shell.readonly"],
  roExplore: ["filesystem.read", "filesystem.search", "shell.readonly", "web"],
  writer: ["filesystem.read", "filesystem.search", "filesystem.write", "shell"],
  orch: ["filesystem.read", "filesystem.search", "shell.readonly", "web", "coordination"],
};

const whole = (caps: OrchestrationCapability[]) => {
  const t = constitutionTiers(caps, north);
  return t.core + t.cap + t.repo;
};

const savedLaws = process.env.AGENT_LAWS;
const savedLawsPath = process.env.AGENT_LAWS_PATH;
afterEach(() => {
  if (savedLaws === undefined) delete process.env.AGENT_LAWS;
  else process.env.AGENT_LAWS = savedLaws;
  if (savedLawsPath === undefined) delete process.env.AGENT_LAWS_PATH;
  else process.env.AGENT_LAWS_PATH = savedLawsPath;
});

// Distinctive section markers.
const M = {
  preamble: "Constitution, not manual",
  blocked: "Blocked ≠ stopped",
  paths: "Paths — full and",
  fleet: "Banned vocabulary",
  apiStub: "subscription entitlements only, never API credits",
  donePara1: "Done-claims carry a bar",
  agentTime: "Agent time is run telemetry",
  preEdit: "Pre-edit gate — MANDATORY",
  routing: "Model + payload routing",
  donePara2: "Evidence attaches where the done-claim lives",
  push: "Push freely",
  external: "External code — license first",
  docsPrivate: "Internal notes → docs/private",
  newCode: "New code — minimize glue",
  serialize: "Never serialize",
  rm: "on variable paths",
  nixos: "Global agent config goes through nixos-config",
  beagle: "Racket / Beagle",
};

test("tagged section metadata overrides legacy heading classification", () => {
  const fixture = mkdtempSync(join(tmpdir(), "north-context-tags-"));
  try {
    const source = join(fixture, "AGENTS.md");
    writeFileSync(source, [
      "# Tagged fixture",
      "",
      "TAGGED_PREAMBLE",
      "",
      "## Push freely — misleading legacy heading",
      "<!-- north-section: tagged-core · bucket: core -->",
      "TAGGED_CORE_SECTION",
      "",
      "## Blocked ≠ misleading legacy heading",
      "<!-- north-section: tagged-write · bucket: write -->",
      "TAGGED_WRITE_SECTION",
      "",
    ].join("\n"));
    process.env.AGENT_LAWS_PATH = source;

    const text = whole(CLASS.roEval);
    expect(text).toContain("TAGGED_CORE_SECTION");
    expect(text).not.toContain("TAGGED_WRITE_SECTION");
  } finally {
    rmSync(fixture, { recursive: true, force: true });
  }
});

test("untagged sections retain legacy substring classification", () => {
  const fixture = mkdtempSync(join(tmpdir(), "north-context-legacy-"));
  try {
    const source = join(fixture, "AGENTS.md");
    writeFileSync(source, [
      "# Untagged fixture",
      "",
      "LEGACY_PREAMBLE",
      "",
      "## Blocked ≠ stopped",
      "",
      "LEGACY_CORE_SECTION",
      "",
      "## Push freely",
      "",
      "LEGACY_WRITE_SECTION",
      "",
    ].join("\n"));
    process.env.AGENT_LAWS_PATH = source;

    const readonly = whole(CLASS.roEval);
    expect(readonly).toContain("LEGACY_CORE_SECTION");
    expect(readonly).not.toContain("LEGACY_WRITE_SECTION");
    expect(whole(CLASS.writer)).toContain("LEGACY_WRITE_SECTION");
  } finally {
    rmSync(fixture, { recursive: true, force: true });
  }
});

test("malformed section tags fall back to legacy headings and unknown headings fail safe", () => {
  const fixture = mkdtempSync(join(tmpdir(), "north-context-malformed-"));
  try {
    const source = join(fixture, "AGENTS.md");
    writeFileSync(source, [
      "# Malformed fixture",
      "",
      "## Push freely",
      "<!-- north-section: malformed-write · bucket: sideways -->",
      "MALFORMED_LEGACY_WRITE",
      "",
      "## Entirely unfamiliar law",
      "<!-- north-section: malformed-core · bucket: sideways -->",
      "MALFORMED_UNKNOWN_CORE",
      "",
    ].join("\n"));
    process.env.AGENT_LAWS_PATH = source;

    const readonly = whole(CLASS.roEval);
    expect(readonly).not.toContain("MALFORMED_LEGACY_WRITE");
    expect(readonly).toContain("MALFORMED_UNKNOWN_CORE");
  } finally {
    rmSync(fixture, { recursive: true, force: true });
  }
});

test("the owned profile has one unique valid tag directly below every section heading", () => {
  const raw = readFileSync(resolve(north, "profiles/tom/AGENTS.md"), "utf8");
  const headings = [...raw.matchAll(/^## .+$/gm)];
  const tags = [...raw.matchAll(
    /^<!-- north-section: ([a-z0-9][a-z0-9-]*) · bucket: (core|write|shell|orch|client|nixos|beagle) -->$/gm,
  )];
  expect(tags.length).toBe(headings.length);
  expect(new Set(tags.map((tag) => tag[1])).size).toBe(tags.length);
  for (const heading of headings) {
    const nextLine = raw.slice((heading.index ?? 0) + heading[0].length + 1).split("\n", 1)[0];
    expect(nextLine).toMatch(
      /^<!-- north-section: [a-z0-9][a-z0-9-]* · bucket: (core|write|shell|orch|client|nixos|beagle) -->$/,
    );
  }
});

test("CORE laws ride with every capability class, byte-identical", () => {
  const cores = Object.values(CLASS).map((caps) => constitutionTiers(caps, north).core);
  for (const core of cores) {
    for (const marker of [M.preamble, M.blocked, M.paths, M.fleet, M.apiStub, M.donePara1, M.agentTime]) {
      expect(core).toContain(marker);
    }
  }
  // Same-class byte identity is the load-bearing cache invariant: CORE is a pure
  // function of the constitution file, independent of role and of the caps that
  // only gate CAP/REPO.
  const uniqueCores = new Set(cores.map(B));
  expect(uniqueCores.size).toBe(1);
});

test("read-only lanes drop pre-edit-gate / routing / push / write / shell / orch laws", () => {
  for (const caps of [CLASS.roEval, CLASS.roExplore]) {
    const text = whole(caps);
    for (const marker of [
      M.preEdit, M.routing, M.donePara2, M.push, M.external,
      M.docsPrivate, M.newCode, M.serialize, M.rm, M.nixos, M.beagle,
    ]) {
      expect(text).not.toContain(marker);
    }
  }
});

test("writer lanes keep the write + shell blocks, not the orchestrator-only laws", () => {
  const text = whole(CLASS.writer);
  for (const marker of [M.push, M.external, M.docsPrivate, M.newCode, M.serialize, M.rm]) {
    expect(text).toContain(marker); // write + shell
  }
  for (const marker of [M.preEdit, M.routing, M.donePara2]) {
    expect(text).not.toContain(marker); // orchestrator-only
  }
  // Generic (non-client/non-nixos) repo: repo-gated laws stay out.
  for (const marker of [M.nixos, M.beagle]) {
    expect(text).not.toContain(marker);
  }
});

test("orchestrator lanes keep the orchestrator block, not the write/shell blocks", () => {
  const text = whole(CLASS.orch);
  for (const marker of [M.preEdit, M.routing, M.donePara2]) {
    expect(text).toContain(marker);
  }
  // director has no filesystem.write and only shell.readonly.
  for (const marker of [M.push, M.newCode, M.serialize, M.rm]) {
    expect(text).not.toContain(marker);
  }
});

test("gating is a deterministic step-function: identical capability sets -> byte-identical tiers", () => {
  for (const caps of Object.values(CLASS)) {
    const a = constitutionTiers([...caps], north);
    const b = constitutionTiers([...caps], north);
    expect(B(a.core)).toBe(B(b.core));
    expect(B(a.cap)).toBe(B(b.cap));
    expect(B(a.repo)).toBe(B(b.repo));
    expect(a.core + a.cap + a.repo).toBe(b.core + b.cap + b.repo);
  }
});

test("constitution byte counts fall well below the ungated whole, monotone by capability breadth", () => {
  const wholeConstitution = B(constitutionTiers(undefined, north).core); // metadata-less = whole
  const sizes = Object.fromEntries(
    Object.entries(CLASS).map(([name, caps]) => [name, B(whole(caps))]),
  );
  // Visible in `bun test` output — quoted in the deliverable's bar evidence.
  console.log(`[tiers] whole=${wholeConstitution}B`, sizes);
  for (const size of Object.values(sizes)) expect(size).toBeLessThan(wholeConstitution);
  expect(sizes.roEval).toBe(sizes.roExplore); // web does not gate any constitution law
  expect(sizes.roEval).toBeLessThan(sizes.writer);
  expect(sizes.writer).toBeLessThan(sizes.orch);
});

test("metadata-less lanes keep the whole constitution unchanged (tiering only activates with caps)", () => {
  const t = constitutionTiers(undefined, north);
  expect(t.cap).toBe("");
  expect(t.repo).toBe("");
  for (const marker of [M.agentTime, M.routing, M.push, M.serialize, M.nixos]) {
    expect(t.core).toContain(marker); // nothing gated away without a capability set
  }
});

test("the per-lane UNIQUE coordination tail lands after every shared tier (P1)", () => {
  const opts = harnessOptions({
    self: "tier-unique-tail",
    provider: "anthropic",
    cwd: north,
    presenceRegistrar: false,
    routingMetadata: applyOrchestrationStaffing({ role: "integrator" }),
  }) as any;
  const sp: string = opts.systemPrompt;
  const coord = sp.indexOf('You are agent "tier-unique-tail"');
  expect(coord).toBeGreaterThan(0);
  // After the shared CORE constitution, the CAP orchestration/law blocks, and the REPO
  // project instructions — i.e. nothing shared follows the unique bytes.
  expect(coord).toBeGreaterThan(sp.indexOf("## Global laws —"));
  expect(coord).toBeGreaterThan(sp.lastIndexOf("## Orchestration"));
  expect(coord).toBeGreaterThan(sp.indexOf("## Project instructions"));
  expect(coord).toBeGreaterThan(sp.indexOf(M.blocked));
  // The DEFAULT head is still first.
  expect(sp.indexOf("north agent")).toBeLessThan(coord);
  const economics = harnessCompositionEvidence(opts)?.promptEconomics;
  expect(economics).toBeDefined();
  expect(economics!.stablePrefixBytes + economics!.uniqueTailBytes).toBe(economics!.totalBytes);
  expect(economics!.totalBytes).toBe(Buffer.byteLength(sp, "utf8"));
  expect(economics!.compositionDigest).toBe(createHash("sha256").update(sp).digest("hex"));
  expect(economics!.tokenMeasurementStatus).toBe("unknown");
  expect(economics!.effectiveContextBudgetTokens).toBeUndefined();
  expect(economics!.contextBudgetStatus).toBe("unknown");
  expect(economics!.contextWindowStatus).toBe("observed");
  expect(economics!.providerContextWindowTokens).toBeGreaterThan(0);
  const encoded = JSON.stringify(economics);
  expect(encoded).not.toContain(sp.slice(0, 80));
  expect(encoded).not.toContain('You are agent "tier-unique-tail"');
});

test("auto-compaction is explicitly pinned in harnessOptions (audit fix 4)", () => {
  const opts = harnessOptions({
    self: "tier-compaction-pin",
    provider: "anthropic",
    cwd: north,
    presenceRegistrar: false,
    routingMetadata: applyOrchestrationStaffing({ role: "integrator" }),
  }) as any;
  expect(opts.settings).toMatchObject({ autoCompactEnabled: true });
});
