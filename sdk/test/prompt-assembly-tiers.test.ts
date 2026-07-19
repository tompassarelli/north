// P1+P2 prompt-assembly: capability-gated tiered constitution + cache-first
// (unique-last) ordering. These assert the per-capability-class section matrix,
// byte-identity of the shared tiers for same-class lanes, and that the per-lane
// UNIQUE coordination tail lands after every shared tier.
import { afterEach, expect, test } from "bun:test";
import { resolve } from "node:path";
import { constitutionTiers, harnessOptions } from "../src/harness";
import { applyGafferStaffing } from "../src/gaffer-staffing";
import type { GafferCapability } from "../src/gaffer-capabilities";

const north = resolve(import.meta.dir, "../..");
const B = (s: string) => Buffer.byteLength(s, "utf8");

// Coarse capability classes (catalog-consistent). Read-only classes carry no
// filesystem.write, shell.readonly only, and no coordination.
const CLASS: Record<string, GafferCapability[]> = {
  roEval: ["filesystem.read", "filesystem.search", "shell.readonly"],
  roExplore: ["filesystem.read", "filesystem.search", "shell.readonly", "web"],
  writer: ["filesystem.read", "filesystem.search", "filesystem.write", "shell"],
  orch: ["filesystem.read", "filesystem.search", "shell.readonly", "web", "coordination"],
};

const whole = (caps: GafferCapability[]) => {
  const t = constitutionTiers(caps, north);
  return t.core + t.cap + t.repo;
};

const savedLaws = process.env.AGENT_LAWS;
afterEach(() => {
  if (savedLaws === undefined) delete process.env.AGENT_LAWS;
  else process.env.AGENT_LAWS = savedLaws;
});

// Distinctive section markers.
const M = {
  preamble: "Constitution, not manual",
  blocked: "Blocked ≠ stopped",
  paths: "Paths — full and",
  fleet: "Banned vocabulary",
  apiStub: "subscription entitlements only, never API credits",
  donePara1: "Done-claims carry a bar",
  billing: "clock or it didn't happen",
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

test("CORE laws ride with every capability class, byte-identical", () => {
  const cores = Object.values(CLASS).map((caps) => constitutionTiers(caps, north).core);
  for (const core of cores) {
    for (const marker of [M.preamble, M.blocked, M.paths, M.fleet, M.apiStub, M.donePara1]) {
      expect(core).toContain(marker);
    }
  }
  // Same-class byte identity is the load-bearing cache invariant: CORE is a pure
  // function of the constitution file, independent of role and of the caps that
  // only gate CAP/REPO.
  const uniqueCores = new Set(cores.map(B));
  expect(uniqueCores.size).toBe(1);
});

test("read-only lanes drop billing / pre-edit-gate / routing / push / write / shell / orch laws", () => {
  for (const caps of [CLASS.roEval, CLASS.roExplore]) {
    const text = whole(caps);
    for (const marker of [
      M.billing, M.preEdit, M.routing, M.donePara2, M.push, M.external,
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
  for (const marker of [M.billing, M.nixos, M.beagle]) {
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
  for (const marker of [M.billing, M.routing, M.push, M.serialize, M.nixos]) {
    expect(t.core).toContain(marker); // nothing gated away without a capability set
  }
});

test("the per-lane UNIQUE coordination tail lands after every shared tier (P1)", () => {
  const opts = harnessOptions({
    self: "tier-unique-tail",
    provider: "anthropic",
    cwd: north,
    presenceRegistrar: false,
    routingMetadata: applyGafferStaffing({ role: "integrator" }),
  }) as any;
  const sp: string = opts.systemPrompt;
  const coord = sp.indexOf('You are agent "tier-unique-tail"');
  expect(coord).toBeGreaterThan(0);
  // After the shared CORE constitution, the CAP gaffer/law blocks, and the REPO
  // project instructions — i.e. nothing shared follows the unique bytes.
  expect(coord).toBeGreaterThan(sp.indexOf("## Global laws —"));
  expect(coord).toBeGreaterThan(sp.lastIndexOf("## Gaffer"));
  expect(coord).toBeGreaterThan(sp.indexOf("## Project instructions"));
  expect(coord).toBeGreaterThan(sp.indexOf(M.blocked));
  // The DEFAULT head is still first.
  expect(sp.indexOf("north agent")).toBeLessThan(coord);
});

test("auto-compaction is explicitly pinned in harnessOptions (audit fix 4)", () => {
  const opts = harnessOptions({
    self: "tier-compaction-pin",
    provider: "anthropic",
    cwd: north,
    presenceRegistrar: false,
    routingMetadata: applyGafferStaffing({ role: "integrator" }),
  }) as any;
  expect(opts.settings).toMatchObject({ autoCompactEnabled: true });
});
