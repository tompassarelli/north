import { afterEach, expect, test } from "bun:test";
import { mkdtempSync, rmSync } from "node:fs";
import { connect, type Socket } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Northd } from "../src/bridge/host";
import type { BridgeProviderExecution } from "../src/bridge/provider";

const cleanups: Array<() => Promise<void> | void> = [];
afterEach(async () => {
  for (const cleanup of cleanups.splice(0).reverse()) await cleanup();
});

async function launch(open: BridgeProviderExecution["open"]): Promise<any[]> {
  const root = mkdtempSync(join(tmpdir(), "north-bridge-cause-"));
  const socketPath = join(root, "northd.sock");
  const northd = new Northd({
    socketPath, journalRoot: join(root, "journal"), provider: { open },
    sourceIdentity: () => undefined,
  });
  await northd.listen();
  cleanups.push(() => rmSync(root, { recursive: true, force: true }));
  cleanups.push(() => northd.close());

  const socket: Socket = connect(socketPath);
  await new Promise<void>((resolve, reject) => {
    socket.once("connect", resolve);
    socket.once("error", reject);
  });
  const messages: any[] = [];
  let buffer = "";
  socket.setEncoding("utf8");
  socket.on("data", (chunk: string) => {
    buffer += chunk;
    while (true) {
      const newline = buffer.indexOf("\n");
      if (newline < 0) break;
      messages.push(JSON.parse(buffer.slice(0, newline)));
      buffer = buffer.slice(newline + 1);
    }
  });
  const closed = new Promise<void>((resolve) => socket.once("close", () => resolve()));
  socket.write(`${JSON.stringify({ op: "launch", prompt: "go", cwd: root })}\n`);
  await closed;
  return messages;
}

function failure(messages: any[]): any {
  return messages.find((message) =>
    message.type === "event" && message.record.kind === "execution.failed")?.record.data;
}

test("an opaque provider code carries its cause chain into the journal", async () => {
  const messages = await launch(async () => {
    throw new Error("openai_codex_authority_preflight_failed", {
      cause: new Error("managed Codex requirements are invalid TOML", {
        cause: new Error("/etc/codex/requirements.toml is not supplied by the verified Nix closure"),
      }),
    });
  });
  expect(failure(messages)).toEqual({
    message: "openai_codex_authority_preflight_failed",
    causes: [
      "managed Codex requirements are invalid TOML",
      "/etc/codex/requirements.toml is not supplied by the verified Nix closure",
    ],
  });
});

test("a failure with no cause records no empty chain", async () => {
  const messages = await launch(async () => { throw new Error("plain failure"); });
  expect(failure(messages)).toEqual({ message: "plain failure" });
});

test("a cyclic cause chain terminates", async () => {
  const messages = await launch(async () => {
    const outer = new Error("outer");
    const inner = new Error("inner", { cause: outer });
    (outer as { cause?: unknown }).cause = inner;
    throw outer;
  });
  expect(failure(messages).causes).toHaveLength(8);
});
