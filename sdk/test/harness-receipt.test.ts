import { afterEach, describe, expect, test } from "bun:test";
import { resolve } from "node:path";
import {
  harnessCompositionEvidence, harnessOptions,
} from "../src/harness";
import { sha256Bytes } from "../src/composition-receipt";

const root = resolve(import.meta.dir, "../..");
const previousLearningPolicy = process.env.NORTH_LEARNING_POLICY;

if (process.env.NORTH_TEST_SANDBOX_HOME === "1" && !process.env.AGENT_HOOKS_DIR) {
  throw new Error("NORTH-TEST-HOOKS-001: sandbox-home receipt tests require AGENT_HOOKS_DIR");
}

afterEach(() => {
  if (previousLearningPolicy === undefined) delete process.env.NORTH_LEARNING_POLICY;
  else process.env.NORTH_LEARNING_POLICY = previousLearningPolicy;
});

function options(explicitClosure: boolean) {
  process.env.NORTH_LEARNING_POLICY = resolve(import.meta.dir, "learning-regime.test.ts");
  return harnessOptions({
    self: explicitClosure ? "receipt-exact" : "receipt-unknown",
    cwd: root,
    systemPrompt: "receipt integration fixture",
    presenceRegistrar: false,
    presenceRenewer: false,
    ...(explicitClosure ? {
      availableSkills: [{
        id: "skill-catalog", sha256: sha256Bytes("catalog-v1"), coverage: "exact" as const,
      }],
      activatedResources: [{
        id: "skill-activated", sha256: sha256Bytes("activated-v1"), coverage: "exact" as const,
      }],
    } : {}),
  });
}

describe("harness construction receipts", () => {
  test("wire prompt and ordered construction are content-addressed without raw prompt storage", () => {
    const built = options(true);
    const evidence = harnessCompositionEvidence(built)!;
    expect(evidence.promptReceipt?.coverage).toBe("exact");
    expect(evidence.promptReceipt?.wireBytesSha256)
      .toBe(sha256Bytes(built.systemPrompt as string));
    expect(evidence.promptReceipt?.modules.length).toBeGreaterThan(1);
    expect(JSON.stringify(evidence.promptReceipt)).not.toContain("receipt integration fixture");
  });

  test("available catalog and activated closure are separate exact identities", () => {
    const evidence = harnessCompositionEvidence(options(true))!;
    expect(evidence.environmentReceipt?.coverage).toBe("exact");
    expect(evidence.environmentReceipt?.availableSkillCatalogSha256)
      .not.toBe(evidence.environmentReceipt?.activatedResourceClosureSha256);
  });

  test("unobserved activation remains unknown instead of an evaluation-ready empty set", () => {
    const evidence = harnessCompositionEvidence(options(false))!;
    expect(evidence.environmentReceipt?.coverage).toBe("unknown");
    expect(evidence.environmentReceipt?.coverageReason)
      .toBe("activated-resource-observation-unavailable");
  });
});
