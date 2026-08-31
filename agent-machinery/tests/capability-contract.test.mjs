import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import {
  AGENT_SOURCE_PATHS,
  CATALOG_SCHEMA_ID,
  PROJECT_EXPOSURE_PROFILE_SCHEMA_ID,
  MODEL_SELECTION_CATALOG_SCHEMA_ID,
  ROUTING_FIELDS,
  ROUTING_REQUEST_SCHEMA_ID,
  SELECTION_ASSESSMENT_SCHEMA_ID,
  STAFFING_CATALOG_SCHEMA_ID,
  WORK_OWNERSHIP_SCHEMA_ID,
  effectiveCapabilities,
  effectiveFilesystemAuthority,
  loadExportCatalog,
  loadStaffingCatalog,
  renderAgent,
  schemaPath,
  validateCapabilityClosure,
  validateRoutingRequest,
} from "../index.mjs";
import { validatePackage } from "../scripts/validate.mjs";

test("catalog assets and contracts bind stable versioned IDs to shipped schemas", () => {
  const catalog = loadExportCatalog();
  const expected = [
    CATALOG_SCHEMA_ID,
    PROJECT_EXPOSURE_PROFILE_SCHEMA_ID,
    MODEL_SELECTION_CATALOG_SCHEMA_ID,
    WORK_OWNERSHIP_SCHEMA_ID,
    STAFFING_CATALOG_SCHEMA_ID,
    ROUTING_REQUEST_SCHEMA_ID,
    SELECTION_ASSESSMENT_SCHEMA_ID,
  ];
  assert.deepEqual(Object.keys(catalog).sort(), ["$schema", "assets", "contracts", "package", "schema", "units"]);
  assert.deepEqual(
    [...new Set(catalog.assets.map(({ type }) => type))].sort(),
    ["catalog", "generated-templates", "instructions", "source-blocks"],
  );
  assert.deepEqual(
    catalog.units.filter(({ kind }) => kind === "module").map(({ id, members }) => [id, members]),
    [
      ["agent-machinery", ["delegation", "agent-practice"]],
      ["delegation", ["work-ownership-distilled", "agent-run-design-distilled"]],
      ["agent-practice", ["build-vs-reuse-distilled", "external-code-distilled", "greenfield-distilled", "planning-distilled", "prior-art-distilled", "production-hardening-distilled", "program-craftsmanship-distilled", "program-stewardship-distilled", "rust-development-distilled", "skill-maintenance-distilled", "terse-distilled", "verification-distilled"]],
    ],
  );
  for (const id of expected) {
    const schema = JSON.parse(readFileSync(schemaPath(id), "utf8"));
    assert.equal(schema.$id, id);
  }
});

test("routing ABI remains exactly nine fields while template identity differs from role", () => {
  assert.deepEqual(ROUTING_FIELDS, [
    "role", "taskGrade", "domainRequirements", "topology", "capabilityFloor", "serviceClass", "reasoning", "posture", "composition",
  ]);
  const request = {
    role: "agent-machinery-integration-owner",
    taskGrade: "senior",
    domainRequirements: [],
    topology: "worker",
    capabilityFloor: "advanced",
    serviceClass: "balanced",
    reasoning: "high",
    posture: "deliver",
    composition: { kind: "template", id: "integrator", overrides: [] },
  };
  assert.equal(validateRoutingRequest(request), request);
  assert.notEqual(request.role, request.composition.id);
  for (const field of ["model", "provider", "account", "actor", "runtime"]) {
    assert.throws(
      () => validateRoutingRequest({ ...request, [field]: "consumer-owned" }),
      new RegExp(`routing request has unknown field\\(s\\): ${field}`),
    );
  }
});

test("export catalog covers every generated-agent input", () => {
  const catalog = loadExportCatalog();
  const packageMetadata = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8"));
  const declared = new Set([
    ...catalog.units.map(({ source }) => source),
    ...catalog.assets.map(({ path }) => path),
  ]);
  for (const input of AGENT_SOURCE_PATHS) {
    assert(declared.has(input), input);
    const exportKey = input.startsWith("docs/") ? "./docs/*" : "./staffing/*";
    assert.equal(packageMetadata.exports[exportKey], `${exportKey.slice(0, -1)}*`, input);
  }
  assert.equal(validatePackage({ checkGenerated: false }).templates, 16);
});

test("shell capabilities close over effective filesystem authority", () => {
  assert.deepEqual(
    effectiveCapabilities(["shell"]),
    ["shell", "filesystem.read", "filesystem.search", "filesystem.write"],
  );
  assert.deepEqual(
    effectiveFilesystemAuthority(["shell.readonly"]),
    { read: true, search: true, write: false },
  );
  assert.throws(
    () => validateCapabilityClosure(["filesystem.read", "shell"]),
    /missing implied filesystem\.search, filesystem\.write/,
  );
});

test("routing rejects capability lists that conceal shell write authority", () => {
  const request = {
    role: "migration-author",
    taskGrade: "senior",
    domainRequirements: [],
    topology: "worker",
    capabilityFloor: "advanced",
    serviceClass: "balanced",
    reasoning: "high",
    posture: "deliver",
    composition: {
      kind: "bespoke",
      id: "migration-author",
      bespokeReason: "specialized migration boundary",
      promotionCandidate: false,
      contract: {
        responsibility: "author a migration",
        deliverable: "a checked migration",
        capabilities: ["filesystem.read", "filesystem.search", "shell"],
        mayDecide: ["implementation details"],
        mustEscalate: ["contract changes"],
        doneWhen: ["the named check passes"],
        report: "change and evidence",
      },
    },
  };
  assert.throws(() => validateRoutingRequest(request), /missing implied filesystem\.write/);
});

test("generated source declares effective filesystem authority fail closed", () => {
  const catalog = loadStaffingCatalog();
  const executor = renderAgent(catalog.presets.find(({ name }) => name === "executor"));
  const designer = renderAgent(catalog.presets.find(({ name }) => name === "designer"));
  assert.match(executor, /## Effective filesystem authority[\s\S]*write: allowed/);
  assert.match(designer, /## Effective filesystem authority[\s\S]*write: denied/);
  assert.match(designer, /filesystem writes are denied[\s\S]*fail closed/);
});
