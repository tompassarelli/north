import { afterEach, expect, test } from "bun:test";
import { chmodSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { spawnSync } from "node:child_process";

const north = resolve(import.meta.dir, "../..");
let directory = "";
afterEach(() => { if (directory) rmSync(directory, { recursive: true, force: true }); directory = ""; });

function evaluateMcp(expression: string, engine: string): string {
  const result = spawnSync("bb", ["-e", `(load-file ${JSON.stringify(resolve(north, "bin/north-mcp"))}) ${expression}`], {
    encoding: "utf8", input: "", env: { ...process.env, NORTH_BIN: engine },
  });
  expect(result.status).toBe(0);
  return result.stdout.trim();
}

test("MCP startup observation reads the structured provider:auto identity", () => {
  directory = mkdtempSync(join(tmpdir(), "north-public-identity-"));
  const count = join(directory, "count");
  const fake = join(directory, "north");
  writeFileSync(count, "0");
  writeFileSync(fake, `#!/usr/bin/env bash
n=$(cat ${JSON.stringify(count)})
n=$((n + 1))
printf '%s' "$n" > ${JSON.stringify(count)}
printf '%s\n' '[{"predicate":"kind","value":"lane"},{"predicate":"provider","value":"openai"},{"predicate":"display_handle","value":"openai-sol-xhigh-gaffer-designer-a205e9ce"}]'
`);
  chmodSync(fake, 0o700);

  expect(evaluateMcp('(println (get (observed-agent-facts "sdk-a205e9ce") "display_handle"))', fake))
    .toBe("openai-sol-xhigh-gaffer-designer-a205e9ce");
  expect(Number(readFileSync(count, "utf8"))).toBe(1);
});

test("MCP ids retain sortable time and the full UUID collision domain", () => {
  directory = mkdtempSync(join(tmpdir(), "north-public-id-"));
  const fake = join(directory, "north");
  writeFileSync(fake, "#!/usr/bin/env bash\nexit 1\n");
  chmodSync(fake, 0o700);
  const id = evaluateMcp('(println (create-mcp-agent-id "019f6c5e-61d0-7880-98a0-f8999eac7b03"))', fake);
  expect(id).toMatch(/^sdk-f8999eac7b03-[a-z0-9]+-[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
});
