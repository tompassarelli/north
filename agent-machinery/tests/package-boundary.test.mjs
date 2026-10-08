import test from "node:test";
import assert from "node:assert/strict";
import {
  assetPath,
  defaultProjectExposureProfile,
  loadExportCatalog,
  resolveProjectExposureProfile,
  validateProjectExposureProfile,
  validateWorkOwnershipTransition,
} from "../index.mjs";
import { FORBIDDEN_TEXT, portableSourceText, validatePackage } from "../scripts/validate.mjs";

test("runtime-owner brands remain forbidden while provider catalog brands are allowed", () => {
  const runtimeOwner = ["Nor", "th"].join("");
  const provider = ["Open", "AI"].join("");
  assert.equal(FORBIDDEN_TEXT.test(runtimeOwner), true, runtimeOwner);
  assert.equal(FORBIDDEN_TEXT.test(provider), false, provider);
});

test("typed source permits only its exact language header", () => {
  const brand = ["Bea", "gle"].join("");
  const header = `#lang ${brand.toLowerCase()}/js\n`;
  assert.equal(FORBIDDEN_TEXT.test(portableSourceText("source.bjs", header)), false);
  assert.equal(
    FORBIDDEN_TEXT.test(portableSourceText("source.bjs", `${header}const leak = "${brand}";\n`)),
    true,
  );
});

test("package manifest permits only exact typed-authoring commands", () => {
  const tool = ["Bea", "gle"].join("");
  const build = `${tool.toLowerCase()} build scripts/work-ownership.bjs scripts/work-ownership.js`;
  assert.equal(FORBIDDEN_TEXT.test(portableSourceText("package.json", build)), false);
  assert.equal(
    FORBIDDEN_TEXT.test(portableSourceText("package.json", `${build}\n"leak":"${tool}"`)),
    true,
  );
});

test("export manifest is a closed source-authority package", () => {
  const result = validatePackage();
  assert.equal(result.units, 23);
});

test("public index resolves declared assets and validators", () => {
  const catalog = loadExportCatalog();
  assert.equal(catalog.package.license, "MIT OR Apache-2.0");
  assert.match(assetPath("doctrine.md"), /doctrine\.md$/);
});

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
      new RegExp(`project exposure lifecycle ${mechanism} cites absent explicit action ${mechanism}`),
    );
  }
});

test("public ownership validator keeps an unacknowledged transfer with its owner", () => {
  const state = {
    goal: "deliver-candidate",
    owner: { kind: "agent-run", id: "run-1" },
    accountableParent: { kind: "listener-agent", id: "listener-1" },
    pendingOffer: null,
  };
  const transition = {
    version: "work-ownership-v1",
    before: structuredClone(state),
    event: {
      kind: "transfer",
      actor: { kind: "agent-run", id: "run-1" },
      to: { kind: "agent-run", id: "run-2" },
    },
    after: structuredClone(state),
  };
  assert.equal(validateWorkOwnershipTransition(transition), transition);
});
