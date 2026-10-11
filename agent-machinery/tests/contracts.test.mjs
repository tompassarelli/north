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
];

for (const contractCase of cases) {
  const contract = catalog.contracts.find(({ id }) => id === contractCase.id);
  assert(contract, contractCase.id);
  const composedValidator = publicApi[contract.validator];
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
        Error,
      );
    });

    test(`${contractCase.id} raw schema classification: ${item.name}`, () => {
      assert.equal(structuralValidator(contractCase.value(item)), !item.schemaInvalid);
    });
  }
}
