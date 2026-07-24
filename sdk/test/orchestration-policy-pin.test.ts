import { afterEach, expect, test } from "bun:test";
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  readPolicyDigestPin, verifyPolicyDigestPin, type PolicyDigestPin,
} from "../src/orchestration-policy-pin";

// §3.2 digest pin (thread 019f8f5c, Phase 2). The pin is the fail-closed
// consumer of policy_sha256: admission proceeds only when the graph's stored
// digest, the live rule projection, and the canonical validator's baked table
// all agree, so a routing floor moves only by a policy version bump — never by
// a bare graph write. These assertions are hermetic (no coordinator): the
// verification logic is exercised directly, and the projection wiring is driven
// by a fake `bb`.

const A = "a".repeat(64);
const B = "b".repeat(64);

function pin(over: Partial<PolicyDigestPin>): PolicyDigestPin {
  return {
    policyVersion: "minimum-sufficient-v1",
    catalogVersion: 1,
    storedSha256: A,
    projectionSha256: A,
    validatorSha256: A,
    ...over,
  };
}

test("all-equal digests admit and return the pinned digest", () => {
  expect(verifyPolicyDigestPin(pin({}))).toBe(A);
});

test("a perturbed rule projection is refused as a bare graph write", () => {
  expect(() => verifyPolicyDigestPin(pin({ projectionSha256: B })))
    .toThrow(/bare graph write altered a routing floor; admission refused/);
});

test("a stale or forged stored digest is refused as a missing version bump", () => {
  expect(() => verifyPolicyDigestPin(pin({ storedSha256: B, projectionSha256: B })))
    .toThrow(/without a policy version bump; admission refused/);
});

// -- projection wiring via a fake bb ---------------------------------------
const priorBb = process.env.NORTH_PEER_BB;
const scratch: string[] = [];

afterEach(() => {
  if (priorBb === undefined) delete process.env.NORTH_PEER_BB;
  else process.env.NORTH_PEER_BB = priorBb;
  for (const dir of scratch.splice(0)) rmSync(dir, { recursive: true, force: true });
});

function fakeBb(body: string): void {
  const dir = mkdtempSync(join(tmpdir(), "north-policy-pin-"));
  scratch.push(dir);
  const bb = join(dir, "bb");
  writeFileSync(bb, `#!/usr/bin/env bash\n${body}\n`);
  chmodSync(bb, 0o755);
  process.env.NORTH_PEER_BB = bb;
}

test("readPolicyDigestPin parses the projector JSON and verifies the normal path", () => {
  fakeBb(`printf '%s' '{"policyVersion":"minimum-sufficient-v1","catalogVersion":1,`
    + `"storedSha256":"${A}","projectionSha256":"${A}","validatorSha256":"${A}"}'`);
  const p = readPolicyDigestPin();
  expect(p.storedSha256).toBe(A);
  expect(verifyPolicyDigestPin(p)).toBe(A);
});

test("readPolicyDigestPin surfaces a perturbed projection for refusal", () => {
  fakeBb(`printf '%s' '{"policyVersion":"minimum-sufficient-v1","catalogVersion":1,`
    + `"storedSha256":"${A}","projectionSha256":"${B}","validatorSha256":"${A}"}'`);
  expect(() => verifyPolicyDigestPin(readPolicyDigestPin()))
    .toThrow(/bare graph write/);
});

test("readPolicyDigestPin fails closed when the projector cannot run", () => {
  fakeBb("exit 7");
  expect(() => readPolicyDigestPin()).toThrow(/policy pin projection failed/);
});

test("a non-digest field is rejected rather than admitted", () => {
  fakeBb(`printf '%s' '{"storedSha256":"nope","projectionSha256":"${A}","validatorSha256":"${A}"}'`);
  expect(() => readPolicyDigestPin()).toThrow(/is not a sha256 digest/);
});
