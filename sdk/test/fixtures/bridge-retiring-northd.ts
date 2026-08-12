import { mkdirSync } from "node:fs";
import { createServer } from "node:net";
import { dirname } from "node:path";
import { bridgeSocketPath } from "../../src/bridge/protocol";

const socketPath = bridgeSocketPath();
const retireDelayMs = Number(process.env.NORTH_BRIDGE_RETIRE_DELAY_MS ?? "0");
mkdirSync(dirname(socketPath), { recursive: true, mode: 0o700 });

const server = createServer((socket) => {
  socket.write(`${JSON.stringify({
    type: "hello",
    identity: "a".repeat(40),
    liveExecutions: 0,
    pinningExecutions: 0,
    pid: process.pid,
  })}\n`);
  socket.once("data", () => {
    socket.end();
    server.close(() => setTimeout(() => process.exit(0), retireDelayMs));
  });
});
server.listen(socketPath);
