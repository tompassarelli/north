import readline from "node:readline";
import { readSync } from "node:fs";

const fixture = JSON.parse(process.env.FAKE_CODEX_RESPONSES ?? "{}");
const delay = Number(process.env.FAKE_CODEX_DELAY_MS ?? 0);

// Tests that need to prove concurrent startup can give each fixture an
// inherited release pipe. Blocking here makes the ordering deterministic:
// no app-server request can be observed or answered until the parent has
// launched every participant and releases them together.
const barrierFd = Number(process.env.FAKE_CODEX_BARRIER_FD ?? -1);
if (Number.isInteger(barrierFd) && barrierFd >= 0) {
  const release = Buffer.allocUnsafe(1);
  if (readSync(barrierFd, release, 0, 1, null) !== 1)
    throw new Error("fake_codex_barrier_closed_before_release");
}

const rl = readline.createInterface({ input: process.stdin });

rl.on("line", (line) => {
  const request = JSON.parse(line);
  const result = fixture[request.method];
  if (result === "exit") return process.exit(9);
  if (result === "never") return;
  if (result?.$error) {
    if (typeof result.$stderr === "string") process.stderr.write(result.$stderr);
    return setTimeout(() => process.stdout.write(`${JSON.stringify({ id: request.id, error: result.$error })}\n`), delay);
  }
  setTimeout(() => process.stdout.write(`${JSON.stringify({ id: request.id, result })}\n`), delay);
});
