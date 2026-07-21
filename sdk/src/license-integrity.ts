import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

export const NORTH_LICENSE_SHA256 = {
  "LICENSE": "0b025817234cc10746750fb0cd907fc0038eeded05ef275abdc4a7b2ee672288",
  "LICENSE-MIT": "51adc9bf9e72be82d08c2a694bcca11a6ac1b9e520bb537e1100a158d7d0d06d",
  "LICENSE-APACHE": "481d039b296107335037f88f33e435b75f931cf3605f222d5c3c634a4b70ec5f",
  "patches/codex-0.144.4/LICENSE.upstream":
    "d17f227e4df5da1600391338865ce0f3055211760a36688f816941d58232d8dc",
  "patches/codex-0.144.4/NOTICE.upstream":
    "9d71575ecfd9a843fc1677b0efb08053c6ba9fd686a0de1a6f5382fd3c220915",
} as const;

function sha256(path: string): string {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function fail(message: string): never {
  throw new Error(`North license integrity failed: ${message}`);
}

export function assertNorthLicenseIntegrity(
  root = resolve(import.meta.dir, "../.."),
): { files: number; license: string } {
  for (const [path, expected] of Object.entries(NORTH_LICENSE_SHA256)) {
    const observed = sha256(resolve(root, path));
    if (observed !== expected)
      fail(`${path} sha256 ${observed} != ${expected}`);
  }

  const packagePath = resolve(root, "sdk/package.json");
  const pkg = JSON.parse(readFileSync(packagePath, "utf8")) as {
    license?: unknown;
    scripts?: Record<string, unknown>;
  };
  if (pkg.license !== "MIT OR Apache-2.0")
    fail("sdk/package.json must declare MIT OR Apache-2.0");
  if (pkg.scripts?.["license:check"] !== "bun run src/license-integrity.ts")
    fail("sdk/package.json license:check is not the canonical integrity entrypoint");
  if (typeof pkg.scripts?.check !== "string"
      || !pkg.scripts.check.startsWith("bun run license:check && "))
    fail("sdk/package.json check does not run license:check first");

  const readme = readFileSync(resolve(root, "README.md"), "utf8");
  for (const link of [
    "[MIT License](LICENSE-MIT)",
    "[Apache License 2.0](LICENSE-APACHE)",
    "[license chooser](LICENSE)",
  ]) {
    if (!readme.includes(link)) fail(`README.md is missing ${link}`);
  }

  const workflow = readFileSync(resolve(root, ".github/workflows/ci.yml"), "utf8");
  if (!/^\s+bun run check\s*$/m.test(workflow))
    fail("CI does not invoke the SDK package check");

  return {
    files: Object.keys(NORTH_LICENSE_SHA256).length,
    license: pkg.license,
  };
}

if (import.meta.main) {
  const result = assertNorthLicenseIntegrity();
  console.log(`North license integrity: ${result.files} exact files; ${result.license}`);
}
