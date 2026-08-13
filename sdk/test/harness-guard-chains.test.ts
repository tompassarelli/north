// The bridge's PreToolUse chains, exercised end to end: real guard scripts run
// through the real hook callbacks harnessOptions() hands the provider.
//
// Listing a guard in EDIT_GUARDS/BASH_GUARDS/WORKER_BASH_GUARDS proves nothing —
// resolveManagedGuardChain silently drops a name it cannot resolve, so a typo is a
// guard that never runs and never complains. These cases invoke the composed hook
// and read back the decision.
//
// Hermetic: LAUNCH_CRITICAL_CODE_ROOT points the worktree guard at a fixture
// container tree, so no assertion depends on this machine's ~/code.
import { afterAll, beforeAll, expect, test } from "bun:test";
import { existsSync, mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { HOOKS_DIR } from "../src/authoring-guards";
import { harnessOptions } from "../src/harness";

const REQUIRED = ["launch-critical-worktree-guard.sh", "git-blind-stage-guard.sh"];
const installed = REQUIRED.every((name) => existsSync(resolve(HOOKS_DIR, name)));

let root: string;
let savedRoot: string | undefined;

beforeAll(() => {
  root = mkdtempSync(join(tmpdir(), "north-guard-chain-"));
  // A `main` is protected when it holds a `.git` (dynamic detection), so the
  // fixture container needs one; `wt-lane` is the sanctioned destination.
  for (const project of ["north", "fram"]) mkdirSync(join(root, project, "main", ".git"), { recursive: true });
  mkdirSync(join(root, "north", "wt-lane"), { recursive: true });
  savedRoot = process.env.LAUNCH_CRITICAL_CODE_ROOT;
  process.env.LAUNCH_CRITICAL_CODE_ROOT = root;
});

afterAll(() => {
  if (savedRoot === undefined) delete process.env.LAUNCH_CRITICAL_CODE_ROOT;
  else process.env.LAUNCH_CRITICAL_CODE_ROOT = savedRoot;
  rmSync(root, { recursive: true, force: true });
});

type Hook = (input: unknown) => Promise<any>;

// Composing a lane shells out to the orchestration CLI, and every probe spawns the
// whole chain. Build each lane once and give the cases room for a real chain rather
// than trimming the matrix to fit a default timeout.
const CASE_TIMEOUT_MS = 60_000;
const lanes = new Map<string, ReturnType<typeof composeLane>>();
const lane = (routingMetadata?: unknown) => {
  const key = JSON.stringify(routingMetadata ?? null);
  const cached = lanes.get(key);
  if (cached) return cached;
  const built = composeLane(routingMetadata);
  lanes.set(key, built);
  return built;
};

function composeLane(routingMetadata?: unknown) {
  const options = harnessOptions({
    self: "guard-chain-probe",
    presenceRegistrar: false,
    presenceRenewer: false,
    cwd: root,
    ...(routingMetadata ? { routingMetadata: routingMetadata as never } : {}),
  }) as any;
  const blocks = options.hooks.PreToolUse as Array<{ matcher: string; hooks: Hook[] }>;
  const pick = (matcher: string) => blocks.find((block) => block.matcher === matcher)!.hooks[0];
  return {
    orchestrator: (options.allowedTools ?? []).includes("mcp__north-peer__command_peer"),
    edit: pick("Edit|Write|MultiEdit"),
    bash: pick("Bash"),
  };
}

const ORCHESTRATOR_ROUTE = {
  role: "director", taskGrade: "staff", domainRequirements: [], topology: "orchestrator",
  tier: "frontier", reasoning: "xhigh", posture: "deliver",
  composition: { kind: "preset", id: "director", overrides: [] },
};

async function decide(hook: Hook, input: unknown) {
  const result = await hook(input);
  const output = result?.hookSpecificOutput;
  return {
    decision: output?.permissionDecision === "deny" ? "deny" : "allow",
    reason: String(output?.permissionDecisionReason ?? ""),
  };
}

const edit = (path: string) => ({
  tool_name: "Edit", session_id: "probe", cwd: root,
  tool_input: { file_path: path, old_string: "a", new_string: "b" },
});
const bash = (command: string) => ({
  tool_name: "Bash", session_id: "probe", cwd: root, tool_input: { command },
});

test.skipIf(!installed)("EDIT_GUARDS refuses a write into a protected main and passes a worktree", async () => {
  const { edit: hook } = lane();
  const denied = await decide(hook, edit(join(root, "north", "main", "sdk", "harness.ts")));
  expect(denied.decision).toBe("deny");
  // Never trap a lane: the refusal has to name the move that works.
  expect(denied.reason).toContain("worktree add");
  expect(denied.reason).toContain("wt-<slug>");

  expect((await decide(hook, edit(join(root, "north", "wt-lane", "sdk", "harness.ts")))).decision)
    .toBe("allow");
}, CASE_TIMEOUT_MS);

// Both Bash chains: they differ only by orchestration permission (agent-spawn-guard),
// and repository layout plus staging discipline bind an orchestrator exactly as they
// bind a plain worker.
for (const [name, route] of [
  ["WORKER_BASH_GUARDS", undefined],
  ["BASH_GUARDS", ORCHESTRATOR_ROUTE],
] as const) {
  test.skipIf(!installed)(`${name} refuses main-checkout writes and blind staging`, async () => {
    const composed = lane(route);
    expect(composed.orchestrator).toBe(route !== undefined);
    const hook = composed.bash;

    const inPlace = await decide(hook, bash(`sed -i s/a/b/ ${join(root, "north", "main", "x.ts")}`));
    expect(inPlace.decision).toBe("deny");
    expect(inPlace.reason).toContain("worktree add");

    expect((await decide(hook, bash(`printf x > ${join(root, "fram", "main", "x.rs")}`))).decision)
      .toBe("deny");

    const blind = await decide(hook, bash("git add -A"));
    expect(blind.decision).toBe("deny");
    expect(blind.reason).toContain("git add path/to/file");
    expect((await decide(hook, bash("printf ready && git add ."))).decision).toBe("deny");

    const broadcast = await decide(hook, bash("kill -9 -1"));
    expect(broadcast.decision).toBe("deny");
    // Never trap a lane: the refusal names the scoped alternative.
    expect(broadcast.reason).toContain("pkill -f");
    expect((await decide(hook, bash("pkill -u tom"))).decision).toBe("deny");
    expect((await decide(hook, bash("loginctl terminate-user tom"))).decision).toBe("deny");
  }, CASE_TIMEOUT_MS);

  test.skipIf(!installed)(`${name} never traps a lane`, async () => {
    const hook = lane(route).bash;
    const main = join(root, "north", "main");
    for (const command of [
      `cat ${main}/AGENTS.md`,
      `git -C ${main} worktree add ${join(root, "north", "wt-lane")} -b lane`,
      `git -C ${main} pull --ff-only`,
      "git add sdk/src/harness.ts",
      `printf x > ${join(root, "north", "wt-lane", "x.txt")}`,
      "git commit -m 'stop using git add -A'",
      "kill -TERM 1234",
      "pkill -f 'wrangler dev --port 8788'",
    ]) {
      expect(`${command} => ${(await decide(hook, bash(command))).decision}`)
        .toBe(`${command} => allow`);
    }
  }, CASE_TIMEOUT_MS);
}
