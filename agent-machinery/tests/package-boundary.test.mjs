import test from "node:test";
import assert from "node:assert/strict";
import {
  defaultProjectExposureProfile,
  resolveProjectExposureProfile,
  validateProjectExposureProfile,
} from "../index.mjs";

test("omitted project exposure resolves to research without a recorded profile artifact", () => {
  const unclassified = resolveProjectExposureProfile(undefined);
  assert.equal(unclassified.engineeringContext, "volatile-owner-controlled-research");
  assert.equal(unclassified.facts.correctness, "exact-bounded-claim");
  assert.deepEqual(unclassified.facts.explicitLifecycleActions, []);
  assert.deepEqual(unclassified.lifecycleBudget, []);
});

test("episodic prior failure: explicit release-only direction cannot authorize adjacent lifecycle actions", () => {
  const profile = defaultProjectExposureProfile("episodic prior failure");
  profile.facts.explicitLifecycleActions = ["release-ceremony"];
  for (const mechanism of [
    "compatibility",
    "rollback",
    "generalized-assurance",
    "provenance-immutability",
    "operational-hardening",
  ]) {
    profile.lifecycleBudget = [{ mechanism, evidence: "explicit-operator-instruction" }];
    assert.throws(
      () => validateProjectExposureProfile(profile),
      Error,
    );
  }
});
