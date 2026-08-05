import { afterEach, expect, test } from "bun:test";
import { createServer } from "node:net";
import type { AddressInfo } from "node:net";
import { kw } from "../src/coord-wire";
import {
  decodeFrame, encodeResponseFrame, rpcRecord, RPC_UNIT, RPC_V1_HEADER_BYTES,
  type RpcResponse,
} from "../src/framrpc-codec";
import { admitExecution, admitPinnedProvider } from "../src/execution-admission";
import { gatedTest } from "./support/capabilities";

const inheritedPort = process.env.NORTH_PORT;
afterEach(() => {
  if (inheritedPort === undefined) delete process.env.NORTH_PORT;
  else process.env.NORTH_PORT = inheritedPort;
});

const directorCapabilities = [
  "filesystem.read", "filesystem.search", "shell.readonly", "web", "coordination",
] as const;
const workerCapabilities = [
  "filesystem.read", "filesystem.search", "shell.readonly",
] as const;

function canonicalEnvironment(
  port: number | string,
  overrides: Record<string, string> = {},
): Record<string, string> {
  return {
    NORTH_PORT: String(port),
    FRAM_SERVER_PORT: String(port),
    FRAM_SPACE_ID: "north-coordination",
    NORTH_FRAMRPC_HOST: "127.0.0.1",
    ...overrides,
  };
}

function statusResponse(
  request: NonNullable<ReturnType<typeof decodeFrame>["request"]>,
  overrides: Partial<RpcResponse> = {},
): RpcResponse {
  return {
    space: request.space,
    op: request.op,
    servedVersion: 7,
    page: null,
    error: null,
    payload: rpcRecord(kw("rpc/status"), [
      kw("serving"), 0, kw("native"),
      rpcRecord(kw("rpc/result-cache"), [0, 0, 0, 0]),
    ]),
    ...overrides,
  };
}

async function withFramedServer(
  reply: (frame: ReturnType<typeof decodeFrame>) => Uint8Array | null,
  body: (port: number) => Promise<void>,
): Promise<void> {
  const server = createServer((socket) => {
    const chunks: Buffer[] = [];
    socket.on("data", (chunk) => {
      chunks.push(chunk);
      const buffer = Buffer.concat(chunks);
      if (buffer.length < RPC_V1_HEADER_BYTES) return;
      const bodyLength = new DataView(
        buffer.buffer, buffer.byteOffset, buffer.length,
      ).getUint32(14, true);
      if (buffer.length < RPC_V1_HEADER_BYTES + bodyLength) return;
      const response = reply(decodeFrame(Uint8Array.from(buffer)));
      if (response === null) socket.destroy();
      else socket.end(Buffer.from(response));
    });
    socket.on("error", () => { /* client-side assertions own the outcome */ });
  });
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const address = server.address();
  if (address === null || typeof address === "string") throw new Error("no port");
  try {
    await body(address.port);
  } finally {
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
}

async function closedPort(): Promise<number> {
  const server = createServer();
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const port = (server.address() as AddressInfo).port;
  await new Promise<void>((resolve) => server.close(() => resolve()));
  return port;
}

test("a pinned OpenAI orchestrator is admitted to the managed North surface", () => {
  expect(() => admitPinnedProvider("openai", directorCapabilities)).not.toThrow();
});

test("OpenAI cached web authority is admitted for pinned and automatic routes", () => {
  const webCapabilities = [
    "filesystem.read", "filesystem.search", "shell.readonly", "web",
  ] as const;
  expect(() => admitPinnedProvider("auto", webCapabilities)).not.toThrow();
  expect(() => admitPinnedProvider(undefined, webCapabilities)).not.toThrow();
  expect(() => admitPinnedProvider("openai", webCapabilities)).not.toThrow();
});

test("every managed lane requires a live canonical FRAMRPC endpoint", async () => {
  const port = await closedPort();
  for (const capabilities of [directorCapabilities, workerCapabilities]) {
    await expect(admitExecution(
      "anthropic", capabilities, process.cwd(),
      { mcpServers: { north: { env: canonicalEnvironment(port) } } },
    )).rejects.toMatchObject({
      code: "blocked_preflight",
      processOutcome: "blocked_preflight",
      retrySafeBeforeAcceptance: true,
    });
  }
});

gatedTest("loopback-bind", "admission never falls back to an ambient North port", async () => {
  await withFramedServer(
    (frame) => encodeResponseFrame(
      frame.requestId, statusResponse(frame.request!),
    ),
    async (port) => {
      process.env.NORTH_PORT = String(port);
      await expect(admitExecution(
        "openai", workerCapabilities, process.cwd(),
        { mcpServers: { north: { env: {} } } },
      )).rejects.toThrow("north_coordination_port_missing");
      await expect(admitExecution(
        "openai", workerCapabilities, process.cwd(),
      )).rejects.toThrow("north_framrpc_environment_missing");
    },
  );
});

gatedTest("loopback-bind", "admission probes the exact inherited FRAMRPC space", async () => {
  const requests: ReturnType<typeof decodeFrame>[] = [];
  await withFramedServer(
    (frame) => {
      requests.push(frame);
      return encodeResponseFrame(frame.requestId, statusResponse(frame.request!));
    },
    async (port) => {
      await expect(admitExecution(
        "openai", workerCapabilities, process.cwd(),
        { mcpServers: { north: { env: canonicalEnvironment(port) } } },
      )).resolves.toBeUndefined();
    },
  );
  expect(requests).toHaveLength(1);
  expect(requests[0]!.request!.space).toBe("north-coordination");
  expect(requests[0]!.request!.op.name).toBe("rpc/status");
  expect(requests[0]!.request!.payload).toEqual(RPC_UNIT);
});

gatedTest("loopback-bind", "admission rejects contradictory endpoint identity before opening a socket", async () => {
  let accepts = 0;
  await withFramedServer(
    (frame) => {
      accepts += 1;
      return encodeResponseFrame(frame.requestId, statusResponse(frame.request!));
    },
    async (port) => {
      const cases = [
        canonicalEnvironment(port, { NORTH_PORT: "not-a-port" }),
        canonicalEnvironment(port, { FRAM_SERVER_PORT: String(port + 1) }),
        canonicalEnvironment(port, { FRAM_SPACE_ID: " " }),
        canonicalEnvironment(port, { NORTH_FRAMRPC_HOST: " " }),
      ];
      for (const environment of cases) {
        await expect(admitExecution(
          "openai", workerCapabilities, process.cwd(),
          { mcpServers: { north: { env: environment } } },
        )).rejects.toBeInstanceOf(Error);
      }
    },
  );
  expect(accepts).toBe(0);
});

gatedTest("loopback-bind", "admission rejects a response from another FRAMRPC space", async () => {
  await withFramedServer(
    (frame) => encodeResponseFrame(
      frame.requestId, statusResponse(frame.request!, { space: "other-space" }),
    ),
    async (port) => {
      await expect(admitExecution(
        "openai", workerCapabilities, process.cwd(),
        { mcpServers: { north: { env: canonicalEnvironment(port) } } },
      )).rejects.toThrow("north_coordinator_preflight_failed");
    },
  );
});

test("a shell-bearing capability set cannot hide effective file authority", async () => {
  await expect(admitExecution(
    "anthropic",
    ["filesystem.read", "filesystem.search", "shell"],
    process.cwd(),
  )).rejects.toThrow("anthropic_adapter_cannot_enforce_orchestration_capabilities");
  await expect(admitExecution(
    "openai",
    ["shell.readonly"],
    process.cwd(),
  )).rejects.toThrow("openai_adapter_cannot_enforce_orchestration_capabilities");
});
