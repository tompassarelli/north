import { afterAll, expect, test } from "bun:test";
import { chmodSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const root = mkdtempSync(join(tmpdir(), "north-harness-invocation-observation-"));
const hooksDir = join(root, "hooks");
mkdirSync(hooksDir);
const firn = join(hooksDir, "firn-system-policy");
const savedHooks = process.env.NORTH_AGENT_PROVIDER_HOOKS;
process.env.NORTH_AGENT_PROVIDER_HOOKS = hooksDir;

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

// Dynamic import is required: guard paths are sealed at harness module load.
const { harnessOptions } = await import("../src/harness");

afterAll(() => {
  if (savedHooks === undefined) delete process.env.NORTH_AGENT_PROVIDER_HOOKS;
  else process.env.NORTH_AGENT_PROVIDER_HOOKS = savedHooks;
  rmSync(root, { recursive: true, force: true });
});

test("the all-entrance SDK hook forwards only canonical Firn receipts", async () => {
  const options = harnessOptions({
    self: "invocation-observation-probe",
    presenceRegistrar: false,
    presenceRenewer: false,
    cwd: root,
  }) as any;
  const groups = options.hooks.PreToolUse as Array<{
    matcher?: string;
    hooks: Array<(input: unknown) => Promise<any>>;
  }>;
  expect(groups.map(({ matcher }) => matcher)).toEqual([
    undefined, "Edit|Write|MultiEdit", "Bash",
  ]);
  const hook = groups[0]!.hooks[0]!;
  const input = { tool_name: "functions.get_goal", tool_input: {}, session_id: "fixture" };
  expect(await hook(input)).toEqual({
    continue: true,
    hookSpecificOutput: { hookEventName: "PreToolUse", additionalContext: receipt },
  });

  writeFirn('{"tool_input":{"message":"raw secret"}}');
  expect(await hook(input)).toEqual({ continue: true });
});
