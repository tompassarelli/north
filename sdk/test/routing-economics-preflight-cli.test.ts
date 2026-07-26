// Thread 019f9cc2. Observed 2026-07-26 ~12:36: three simultaneous `north
// delegate` admissions (role executor, three different threads, one openai pin
// with valid pin-evidence). TWO died printing exactly
//
//     routing economics preflight failed
//
// and nothing else; the THIRD admitted; an immediate sequential retry of the
// failed two admitted fine. Two defects rode together — a concurrency failure,
// and an error path that swallowed whatever the concurrency failure actually
// said. This file pins both at the preflight CLI boundary: N simultaneous
// admissions against identical fixture state all succeed, and a rejection
// carries its underlying text to the caller's stdout/stderr rather than
// collapsing into a bare adjective.
import { expect, test } from "bun:test";
import { resolve } from "node:path";

const CLI = resolve(import.meta.dir, "../src/routing-economics-preflight-cli.ts");

/** Canonical complete eight-field request for the stock `executor` preset. */
function executorRequest() {
  return {
    role: "executor", taskGrade: "novice", domainRequirements: [], topology: "worker",
    tier: "economy", reasoning: "low", posture: "deliver",
    composition: { kind: "preset", id: "executor", overrides: [] },
  };
}

function openaiPinEvidence() {
  const now = new Date();
  return {
    policyVersion: "north-routing-pin-v1",
    issuedAt: now.toISOString(),
    expiresAt: new Date(now.getTime() + 6 * 60 * 60 * 1_000).toISOString(),
    reasonCode: "explicit-human-request",
    detail: "concurrent admission fixture",
    pins: [{ kind: "provider", value: "openai" }],
  };
}

async function preflight(payload: unknown): Promise<{
  exitCode: number; stdout: string; stderr: string;
}> {
  const child = Bun.spawn([process.execPath, "run", CLI], {
    stdin: new TextEncoder().encode(JSON.stringify(payload)),
    stdout: "pipe", stderr: "pipe", env: process.env,
  });
  const [stdout, stderr, exitCode] = await Promise.all([
    new Response(child.stdout).text(),
    new Response(child.stderr).text(),
    child.exited,
  ]);
  return { exitCode, stdout, stderr };
}

// The exact failed shape: same role and same provider pin, admitted at the same
// instant. Three concurrent preflights must ALL produce an admission receipt —
// no partial survivor, no anonymous death.
test("three simultaneous admissions of the same pinned route all admit", async () => {
  const payload = {
    routingMetadata: executorRequest(),
    provider: "openai",
    pinEvidence: openaiPinEvidence(),
  };
  const results = await Promise.all([preflight(payload), preflight(payload), preflight(payload)]);
  for (const [index, result] of results.entries()) {
    expect(
      result.exitCode,
      `concurrent preflight ${index} failed: ${result.stderr || "(no stderr)"}`,
    ).toBe(0);
    const receipt = JSON.parse(result.stdout) as { version: number; pinEvidenceStatus: string };
    expect(receipt.version).toBe(1);
    expect(receipt.pinEvidenceStatus).toBe("current");
  }
  // Identical inputs must yield an identical receipt: concurrency may not
  // perturb the admitted route, only its timing.
  const [first, ...rest] = results.map((result) => result.stdout);
  for (const other of rest) expect(other).toBe(first);
});

test("a preflight rejection prints its underlying reason, never a bare adjective", async () => {
  // Explicit pin with NO pin-evidence — a real, named refusal from the economics
  // layer, several frames below this CLI's catch.
  const rejected = await preflight({
    routingMetadata: executorRequest(), provider: "openai", target: "codex-personal",
  });
  expect(rejected.exitCode).not.toBe(0);
  expect(rejected.stderr).toContain("require current typed pinEvidence");
  expect(rejected.stderr.trim()).not.toBe("routing economics preflight failed");
});

test("a malformed envelope names the malformation instead of failing anonymously", async () => {
  const notAnObject = await preflight(["routingMetadata"]);
  expect(notAnObject.exitCode).not.toBe(0);
  expect(notAnObject.stderr).toContain("expects one JSON object on stdin");

  const incomplete = await preflight({ routingMetadata: { role: "executor" } });
  expect(incomplete.exitCode).not.toBe(0);
  expect(incomplete.stderr).toContain("complete eight-field Orchestration request");
});

// Fault injection for the swallow itself. A non-Error throw used to print the
// exact same string the Clojure caller printed for an unheard subprocess, so
// two unrelated failures were indistinguishable on the terminal; an Error with
// an empty message printed a blank line. Drive the CLI's own catch directly.
test("a non-Error or empty-message throw still names itself", async () => {
  const probe = resolve(import.meta.dir, "support/preflight-catch-probe.ts");
  for (const [thrown, expected] of [
    ["non-error", "non-Error string"],
    ["empty-error", "threw Error with no message"],
    ["object", "non-Error object"],
  ] as const) {
    const child = Bun.spawn([process.execPath, "run", probe, thrown], {
      stdout: "pipe", stderr: "pipe", env: process.env,
    });
    const [stderr, exitCode] = await Promise.all([
      new Response(child.stderr).text(), child.exited,
    ]);
    expect(exitCode).not.toBe(0);
    expect(stderr).toContain(expected);
    expect(stderr.trim()).not.toBe("routing economics preflight failed");
    expect(stderr.trim()).not.toBe("");
  }
});
