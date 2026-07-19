import { expect, test } from "bun:test";
import { spawn, spawnSync } from "node:child_process";

const enabled = process.env.NORTH_RUN_INSTALLED_CODEX_SIGNAL_SMOKE === "1";

function within<T>(promise: Promise<T>, timeoutMs: number, label: string): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`${label} timed out`)), timeoutMs);
    promise.then(
      (value) => {
        clearTimeout(timer);
        resolve(value);
      },
      (error) => {
        clearTimeout(timer);
        reject(error);
      },
    );
  });
}

test.skipIf(!enabled)(
  "installed Codex single-client stdio preserves SIGTERM as terminal provenance",
  async () => {
    const executable = process.env.NORTH_INSTALLED_CODEX_BIN ?? "codex";
    const version = spawnSync(executable, ["--version"], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
      timeout: 2_000,
    });
    expect(version.status).toBe(0);
    expect(version.stdout.trim()).toMatch(/^codex-cli \d+\.\d+\.\d+$/);

    // An open stdin selects the exact `codex exec -` single-client stdio path
    // without submitting a task. That path must not translate North's SIGTERM
    // into a normal exit, or terminal-signal provenance would be laundered.
    const child = spawn(executable, [
      "exec",
      "--json",
      "--color", "never",
      "--skip-git-repo-check",
      "-",
    ], {
      stdio: ["pipe", "pipe", "pipe"],
    });
    child.stdout.resume();
    child.stderr.resume();
    try {
      await within(new Promise<void>((resolve, reject) => {
        child.once("spawn", resolve);
        child.once("error", reject);
      }), 2_000, "installed Codex spawn");
      await new Promise<void>((resolve) => setTimeout(resolve, 500));
      expect(child.exitCode).toBeNull();
      expect(child.signalCode).toBeNull();
      expect(child.kill("SIGTERM")).toBe(true);
      const terminal = await within(new Promise<{
        code: number | null;
        signal: NodeJS.Signals | null;
      }>((resolve) => {
        child.once("exit", (code, signal) => resolve({ code, signal }));
      }), 2_000, "installed Codex SIGTERM");
      expect(terminal).toEqual({ code: null, signal: "SIGTERM" });
    } finally {
      if (child.exitCode === null && child.signalCode === null) child.kill("SIGKILL");
      child.stdin.destroy();
      child.stdout.destroy();
      child.stderr.destroy();
    }
  },
);
