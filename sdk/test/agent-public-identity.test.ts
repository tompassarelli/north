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

test("MCP spawn acknowledgement polling waits for the structured provider:auto handle", () => {
  directory = mkdtempSync(join(tmpdir(), "north-public-identity-"));
  const count = join(directory, "count");
  const fake = join(directory, "north");
  writeFileSync(count, "0");
  writeFileSync(fake, `#!/usr/bin/env bash
n=$(cat ${JSON.stringify(count)})
n=$((n + 1))
printf '%s' "$n" > ${JSON.stringify(count)}
if [ "$n" -lt 5 ]; then exit 1; fi
printf '%s\n' '[{"predicate":"display_handle","value":"openai-sol-xhigh-designer-a205e9ce"}]'
`);
  chmodSync(fake, 0o700);

  expect(evaluateMcp('(println (await-display-handle "sdk-a205e9ce" {"provider" "auto"}))', fake))
    .toBe("openai-sol-xhigh-designer-a205e9ce");
  expect(Number(readFileSync(count, "utf8"))).toBe(5);
});

test("MCP fallback identity keeps explicit unknowns and never exposes sdk as provider", () => {
  directory = mkdtempSync(join(tmpdir(), "north-public-fallback-"));
  const fake = join(directory, "north");
  writeFileSync(fake, "#!/usr/bin/env bash\nexit 1\n");
  chmodSync(fake, 0o700);

  expect(evaluateMcp('(println (fallback-display-handle "sdk-a205e9ce" {"provider" "openai" "model" "gpt-5.6-sol" "reasoning" "xhigh" "composition" {"kind" "preset" "id" "designer"}}))', fake))
    .toBe("openai-sol-xhigh-designer-a205e9ce");
});
