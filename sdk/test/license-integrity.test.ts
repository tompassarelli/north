import { afterEach, expect, test } from "bun:test";
import {
  appendFileSync, cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import {
  assertNorthLicenseIntegrity, NORTH_LICENSE_SHA256,
} from "../src/license-integrity";

const north = resolve(import.meta.dir, "../..");
const roots: string[] = [];

afterEach(() => {
  for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});

function fixture(): string {
  const root = mkdtempSync(join(tmpdir(), "north-license-integrity-"));
  roots.push(root);
  const paths = [
    ...Object.keys(NORTH_LICENSE_SHA256),
    "README.md",
    "sdk/package.json",
    ".github/workflows/ci.yml",
  ];
  for (const path of paths) {
    const target = resolve(root, path);
    mkdirSync(dirname(target), { recursive: true });
    cpSync(resolve(north, path), target);
  }
  return root;
}

test("North dual-license files, metadata, package check, and CI wiring are exact", () => {
  expect(assertNorthLicenseIntegrity(north)).toEqual({
    files: 5,
    license: "MIT OR Apache-2.0",
  });
});

test("every first-party and preserved upstream license artifact fails closed on drift", () => {
  for (const path of Object.keys(NORTH_LICENSE_SHA256)) {
    const root = fixture();
    appendFileSync(resolve(root, path), "hostile drift\n");
    expect(() => assertNorthLicenseIntegrity(root))
      .toThrow(`North license integrity failed: ${path} sha256`);
  }
});

test("license metadata, README links, package wiring, and CI wiring fail closed", () => {
  {
    const root = fixture();
    const path = resolve(root, "sdk/package.json");
    const pkg = JSON.parse(readFileSync(path, "utf8"));
    pkg.license = "MIT";
    writeFileSync(path, `${JSON.stringify(pkg, null, 2)}\n`);
    expect(() => assertNorthLicenseIntegrity(root)).toThrow("must declare MIT OR Apache-2.0");
  }
  {
    const root = fixture();
    const path = resolve(root, "sdk/package.json");
    const pkg = JSON.parse(readFileSync(path, "utf8"));
    pkg.scripts.check = pkg.scripts.check.replace("bun run license:check && ", "");
    writeFileSync(path, `${JSON.stringify(pkg, null, 2)}\n`);
    expect(() => assertNorthLicenseIntegrity(root)).toThrow("check does not run license:check first");
  }
  {
    const root = fixture();
    const path = resolve(root, "README.md");
    writeFileSync(path, readFileSync(path, "utf8").replace("LICENSE-MIT", "LICENSE-MISSING"));
    expect(() => assertNorthLicenseIntegrity(root)).toThrow("README.md is missing [MIT License]");
  }
  {
    const root = fixture();
    const path = resolve(root, ".github/workflows/ci.yml");
    writeFileSync(path, readFileSync(path, "utf8").replace("          bun run check\n", ""));
    expect(() => assertNorthLicenseIntegrity(root)).toThrow("CI does not invoke the SDK package check");
  }
});
