import { afterAll, expect, test } from "bun:test";
import { chmodSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const root = mkdtempSync(join(tmpdir(), "north-harness-invocation-observation-"));
const hooksDir = join(root, "hooks");
mkdirSync(hooksDir);
const firn = join(hooksDir, "firn-system-policy");
const probe = join(root, "probe.ts");

const writeFirn = (additionalContext: string) => {
  writeFileSync(firn, `#!/usr/bin/env bash
cat >/dev/null
printf '%s' '${JSON.stringify({
    hookSpecificOutput: { hookEventName: "PreToolUse", additionalContext },
  })}'
`);
  chmodSync(firn, 0o700);
};

const receipt = '{"schema":"InvocationObservation/v1","hook":"firn-system-policy",'
  + '"operation":"functions.get_goal","classification":"empty-object","decision":"pass"}';
writeFirn(receipt);

// Guard paths are sealed at harness module load. Run the harness probe in a
// fresh Bun process so another grouped test cannot populate the parent module
// cache before this fixture supplies its exact provider-hook directory.
writeFileSync(probe, `
const { harnessOptions } = await import(${JSON.stringify(
  new URL("../src/harness.ts", import.meta.url).href,
)});
const options = harnessOptions({
  self: "invocation-observation-probe",
  presenceRegistrar: false,
  presenceRenewer: false,
  cwd: ${JSON.stringify(root)},
});
const groups = options.hooks.PreToolUse;
const input = { tool_name: "functions.get_goal", tool_input: {}, session_id: "fixture" };
process.stdout.write(JSON.stringify({
  matchers: groups.map(({ matcher }) => matcher ?? null),
  result: await groups[0].hooks[0](input),
}));
`);

async function runHarnessProbe(): Promise<any> {
  const child = Bun.spawn({
    cmd: [process.execPath, "run", probe],
    env: { ...process.env, NORTH_AGENT_PROVIDER_HOOKS: hooksDir },
    stdout: "pipe",
    stderr: "pipe",
  });
  const [stdout, stderr, exitCode] = await Promise.all([
    new Response(child.stdout).text(),
    new Response(child.stderr).text(),
    child.exited,
  ]);
  if (exitCode !== 0)
    throw new Error(`isolated harness probe failed (${exitCode}): ${stderr}`);
  return JSON.parse(stdout);
}

afterAll(() => {
  rmSync(root, { recursive: true, force: true });
});

test("the all-entrance SDK hook forwards only canonical Firn receipts", async () => {
  const observed = await runHarnessProbe();
  expect(observed.matchers).toEqual([
    null, "Edit|Write|MultiEdit", "Bash",
  ]);
  expect(observed.result).toEqual({
    continue: true,
    hookSpecificOutput: { hookEventName: "PreToolUse", additionalContext: receipt },
  });

  writeFirn('{"tool_input":{"message":"raw secret"}}');
  expect((await runHarnessProbe()).result).toEqual({ continue: true });
});
