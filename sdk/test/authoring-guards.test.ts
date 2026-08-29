// Unit tests for the SDK worker authoring-guard bridge (authoring-guards.ts).
// Hermetic: no real guard scripts, no coordinator — synthetic fixture scripts written
// to a temp dir cover each rung of the guard-result protocol (deny-JSON, exit-2+stderr,
// exit-0 allow, timeout/missing unavailable, and the first-deny-wins chain.
import { test, expect, describe, beforeAll, afterAll } from "bun:test";
import {
  chmodSync, existsSync, mkdtempSync, readFileSync, rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  authoringHooksDir, runGuardScript, evaluateGuards, resolveManagedGuardChain,
} from "../src/authoring-guards";

let dir: string;
const script = (name: string, body: string): string => {
  const p = join(dir, name);
  writeFileSync(p, body);
  chmodSync(p, 0o755);
  return p;
};

// Fixtures modeled on the real guards' output shapes.
const DENY_JSON = `#!/usr/bin/env bash
cat >/dev/null   # drain the hook JSON on stdin, exactly as the real guards do
printf '%s' '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"This checkout is launch-critical"}}'
exit 0
`;
const EXIT2_STDERR = `#!/usr/bin/env bash
cat >/dev/null
printf 'tripwire: recursive delete outside safe roots\\n' >&2
exit 2
`;
const ALLOW_EXIT0 = `#!/usr/bin/env bash
cat >/dev/null
exit 0
`;
const ALLOW_CONTEXT = `#!/usr/bin/env bash
cat >/dev/null
printf '%s' '{"hookSpecificOutput":{"hookEventName":"PreToolUse","additionalContext":"heads up"}}'
exit 0
`;
const RECEIPT = '{"schema":"InvocationObservation/v1","hook":"firn-system-policy","operation":"functions.get_goal","classification":"empty-object","decision":"pass"}';
const ALLOW_RECEIPT = `#!/usr/bin/env bash
cat >/dev/null
printf '%s' '${JSON.stringify({
  hookSpecificOutput: { hookEventName: "PreToolUse", additionalContext: RECEIPT },
})}'
exit 0
`;
const SLEEP_PAST = `#!/usr/bin/env bash
cat >/dev/null
sleep 5
exit 0
`;
const ECHO_STDIN = `#!/usr/bin/env bash
cat > "$STDIN_CAP"   # capture what the harness fed on stdin
exit 0
`;
const OVERSIZED_OUTPUT = `#!/usr/bin/env bash
cat >/dev/null
head -c 70000 /dev/zero
`;
const FORKED_HELD_PIPE = `#!/usr/bin/env bash
cat >/dev/null
(
  trap '' TERM
  while :; do sleep 1; done
) &
printf '%s' "$!" > "$DESCENDANT_PID_FILE"
sleep 5
`;

beforeAll(() => {
  dir = mkdtempSync(join(tmpdir(), "guard-test-"));
});
afterAll(() => {
  rmSync(dir, { recursive: true, force: true });
});

const HOOK = { tool_name: "Write", tool_input: { file_path: "/x" }, cwd: "/x", session_id: "s" };

describe("authoringHooksDir — generation default and exact override", () => {
  test("defaults to the current immutable generation", () => {
    const home = join(dir, "hooks-home");
    const dflt = authoringHooksDir({ HOME: home });
    expect(dflt).toBe(join(home, ".local", "state", "north", "agents", "current", "provider-hooks"));
  });

  test("an exact provider-hook override wins outright and is home-independent", () => {
    const override = join(dir, "portable-hooks");
    expect(authoringHooksDir({ HOME: join(dir, "hooks-home"), NORTH_AGENT_PROVIDER_HOOKS: override }))
      .toBe(override);
    expect(authoringHooksDir({ NORTH_AGENT_PROVIDER_HOOKS: override })).toBe(override);
    // A blank override is ignored and falls back to the portable default.
    expect(authoringHooksDir({ HOME: join(dir, "hooks-home"), NORTH_AGENT_PROVIDER_HOOKS: "  " }))
      .toBe(join(dir, "hooks-home", ".local", "state", "north", "agents", "current", "provider-hooks"));
  });
});

describe("runGuardScript — result protocol", () => {
  test("deny JSON on stdout -> deny surfaced with its reason", async () => {
    const d = await runGuardScript(script("deny.sh", DENY_JSON), HOOK);
    expect(d.decision).toBe("deny");
    if (d.decision === "deny") expect(d.reason).toContain("launch-critical");
  });

  test("exit 2 + stderr -> deny with stderr as reason", async () => {
    const d = await runGuardScript(script("exit2.sh", EXIT2_STDERR), HOOK);
    expect(d.decision).toBe("deny");
    if (d.decision === "deny") expect(d.reason).toContain("recursive delete");
  });

  test("exit 0, empty stdout -> allow", async () => {
    const d = await runGuardScript(script("allow.sh", ALLOW_EXIT0), HOOK);
    expect(d.decision).toBe("allow");
  });

  test("JSON without a deny decision (additionalContext only) -> allow", async () => {
    const d = await runGuardScript(script("ctx.sh", ALLOW_CONTEXT), HOOK);
    expect(d).toEqual({ decision: "allow" });
  });

  test("only the named Firn adapter preserves an exact canonical receipt", async () => {
    const d = await runGuardScript(script("firn-system-policy", ALLOW_RECEIPT), HOOK);
    expect(d).toEqual({
      decision: "allow",
      observation: {
        raw: RECEIPT,
        observation: {
          schema: "InvocationObservation/v1",
          hook: "firn-system-policy",
          operation: "functions.get_goal",
          classification: "empty-object",
          decision: "pass",
        },
      },
    });
    expect(await runGuardScript(script("other-guard", ALLOW_RECEIPT), HOOK))
      .toEqual({ decision: "allow" });
  });

  test("script that sleeps past the timeout -> unavailable", async () => {
    const d = await runGuardScript(script("slow.sh", SLEEP_PAST), HOOK, 200);
    expect(d.decision).toBe("unavailable");
  });

  test("missing script -> unavailable", async () => {
    const d = await runGuardScript(join(dir, "does-not-exist.sh"), HOOK);
    expect(d.decision).toBe("unavailable");
  });

  test("guard output is bounded and fails unavailable", async () => {
    const decision = await runGuardScript(
      script("oversized.sh", OVERSIZED_OUTPUT),
      HOOK,
      1_000,
    );
    expect(decision).toEqual({
      decision: "unavailable",
      reason: "guard process output exceeded bounded size",
    });
  });

  test.skipIf(process.platform === "win32")(
    "timeout terminates a forked descendant that holds inherited pipes",
    async () => {
      const pidFile = join(dir, "held-pipe-descendant.pid");
      const decisionPromise = runGuardScript(
        script("held-pipe.sh", FORKED_HELD_PIPE),
        HOOK,
        100,
        { ...process.env, DESCENDANT_PID_FILE: pidFile },
      );
      const deadline = Date.now() + 1_000;
      while (!existsSync(pidFile) && Date.now() < deadline) await Bun.sleep(10);
      expect(existsSync(pidFile)).toBe(true);
      const pid = Number(readFileSync(pidFile, "utf8"));
      expect(Number.isSafeInteger(pid) && pid > 1).toBe(true);
      expect(await decisionPromise).toEqual({
        decision: "unavailable",
        reason: "guard process timed out",
      });
      let alive = true;
      const goneBy = Date.now() + 1_000;
      while (alive && Date.now() < goneBy) {
        try {
          process.kill(pid, 0);
          await Bun.sleep(10);
        } catch {
          alive = false;
        }
      }
      expect(alive).toBe(false);
    },
  );

  test("hook input is delivered on stdin as JSON the guards can parse", async () => {
    const cap = join(dir, "stdin.json");
    process.env.STDIN_CAP = cap;
    await runGuardScript(script("echo.sh", ECHO_STDIN), HOOK);
    delete process.env.STDIN_CAP;
    const seen = JSON.parse(require("node:fs").readFileSync(cap, "utf8"));
    expect(seen.tool_name).toBe("Write");
    expect(seen.tool_input.file_path).toBe("/x");
  });
});

describe("evaluateGuards — chain, first deny wins", () => {
  test("all allow -> allow", async () => {
    const chain = [script("a1.sh", ALLOW_EXIT0), script("a2.sh", ALLOW_EXIT0)];
    expect((await evaluateGuards(chain, HOOK)).decision).toBe("allow");
  });

  test("a middle deny short-circuits and wins", async () => {
    const chain = [
      script("c1.sh", ALLOW_EXIT0),
      script("c2.sh", DENY_JSON),
      script("c3.sh", EXIT2_STDERR), // must NOT run — first deny already won
    ];
    const d = await evaluateGuards(chain, HOOK);
    expect(d.decision).toBe("deny");
    if (d.decision === "deny") expect(d.reason).toContain("launch-critical");
  });

  test("empty chain -> allow", async () => {
    expect((await evaluateGuards([], HOOK)).decision).toBe("allow");
  });

  test("chain construction filters missing guards", async () => {
    const missingDir = join(dir, "absent-at-import");
    const existing = script("existing-guard.sh", ALLOW_EXIT0);
    const chain = resolveManagedGuardChain([
      "missing.sh", "existing-guard.sh",
    ], dir);
    expect(chain).toEqual([existing]);
    expect(await evaluateGuards(chain, HOOK)).toEqual({ decision: "allow" });
  });
});
