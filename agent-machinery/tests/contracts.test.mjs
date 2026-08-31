import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import Ajv2020 from "ajv/dist/2020.js";
import * as publicApi from "../index.mjs";

const readJson = (relativePath) => JSON.parse(readFileSync(new URL(`../${relativePath}`, import.meta.url), "utf8"));
const catalog = readJson("catalog.json");
const ajv = new Ajv2020({ allErrors: true, strict: false });

const cases = [
  {
    id: "project-exposure-v1",
    fixture: readJson("contracts/project-exposure-profile.fixtures.json"),
    value: (item) => item.profile,
  },
  {
    id: "work-ownership-v1",
    fixture: readJson("contracts/work-ownership.fixtures.json"),
    value: (item) => item.transition,
  },
  {
    id: "routing-request-v3",
    fixture: readJson("contracts/routing-request.fixtures.json"),
    value: (item) => item.request,
  },
  {
    id: "minimum-sufficient-v2",
    fixture: readJson("contracts/selection-assessment.fixtures.json"),
    value: (item) => item.assessment,
  },
];

function applyOperations(base, operations) {
  const value = structuredClone(base);
  for (const operation of operations) {
    const parent = operation.path.slice(0, -1).reduce((current, segment) => current[segment], value);
    const key = operation.path.at(-1);
    if (operation.op === "set") parent[key] = operation.value;
    else if (operation.op === "remove") delete parent[key];
    else if (operation.op === "append") parent[key].push(operation.value);
    else throw new Error(`unsupported fixture operation: ${operation.op}`);
  }
  return value;
}

const staffingFixture = readJson("staffing/catalog.fixtures.json");
cases.push({
  id: "staffing-catalog-v3",
  fixture: staffingFixture,
  value: (item) => item.source ? readJson(item.source) : applyOperations(staffingFixture.base, item.operations),
});

const validators = new Set(catalog.contracts.map(({ validator }) => validator));
assert.deepEqual([...validators], ["validateContract"]);

for (const contractCase of cases) {
  const contract = catalog.contracts.find(({ id }) => id === contractCase.id);
  assert(contract, contractCase.id);
  assert.equal(contract.schemaScope, "structural");
  const composedValidator = publicApi[contract.validator];
  assert.equal(typeof composedValidator, "function");
  const structuralValidator = ajv.compile(readJson(contract.schema));

  for (const item of contractCase.fixture.valid) {
    test(`${contractCase.id} composed valid: ${item.name}`, () => {
      const value = contractCase.value(item);
      assert.equal(composedValidator(contractCase.id, value), value);
    });
  }

  for (const item of contractCase.fixture.invalid) {
    test(`${contractCase.id} composed invalid: ${item.name}`, () => {
      assert.throws(
        () => composedValidator(contractCase.id, contractCase.value(item)),
        new RegExp(item.errorContains),
      );
    });

    test(`${contractCase.id} raw schema classification: ${item.name}`, () => {
      assert.equal(structuralValidator(contractCase.value(item)), !item.schemaInvalid);
    });
  }
}
