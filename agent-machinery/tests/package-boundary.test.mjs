import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { validateContract } from "../index.mjs";

test("generated explicit lifecycle budgets authorize exactly the named actions", () => {
  const fixture = JSON.parse(readFileSync(new URL("../contracts/project-exposure-profile.fixtures.json", import.meta.url), "utf8"));
  const mechanisms = ["compatibility", "rollback", "generalized-assurance", "provenance-immutability", "release-ceremony", "operational-hardening"];
  for (let mask = 0; mask < 1 << mechanisms.length; mask++) {
    for (const [index, mechanism] of mechanisms.entries()) {
      const profile = structuredClone(fixture.valid.find(({ profile }) => profile.engineeringContext === "volatile-owner-controlled-research").profile);
      profile.facts.explicitLifecycleActions = mechanisms.filter((_, bit) => mask & (1 << bit));
      profile.lifecycleBudget = [{ mechanism, evidence: "explicit-operator-instruction" }];
      if (mask & (1 << index)) {
        assert.equal(validateContract("project-exposure-v1", profile), profile);
      } else {
        assert.throws(
          () => validateContract("project-exposure-v1", profile),
          (error) => error instanceof Error && error.message.includes(`${mechanism} cites absent explicit action ${mechanism}`),
        );
      }
    }
  }
});
