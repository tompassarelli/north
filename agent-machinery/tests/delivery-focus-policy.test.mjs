import test from "node:test";
import assert from "node:assert/strict";
import { validateProjectExposureProfile } from "../scripts/project-exposure-profile.mjs";

test("public state permits but does not require lifecycle ceremony", () => {
  const profile = validateProjectExposureProfile({
    version: "project-exposure-v1",
    scope: "public but break-tolerant research artifact",
    facts: {
      consumer: "named-break-tolerant",
      state: "production-or-public",
      effect: "reversible",
      correctness: "exact-bounded-claim",
      boundaries: [],
      stage: "externally-depended-upon",
      explicitLifecycleActions: [],
    },
    engineeringContext: "externally-depended-upon",
    lifecycleBudget: [],
  });

  assert.deepEqual(profile.lifecycleBudget, []);
});
