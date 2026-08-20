import * as stream from "node:stream";
import * as acp from "@agentclientprotocol/sdk";
import { createBridgeAcpApplication, type BridgeAcpAgentOptions } from "./agent";

type BridgeAcpStdioOptions = Omit<BridgeAcpAgentOptions, "attemptId"> & {
  attemptId?: string;
};

/** Serve one isolated ACP client over stdio; stdout is exclusively the ACP transport. */
export async function runBridgeAcpStdio(options: BridgeAcpStdioOptions = {}): Promise<void> {
  const { app, agent } = createBridgeAcpApplication({
    ...options,
    attemptId: options.attemptId ?? process.env.NORTH_ATTEMPT_ID ?? "",
  });
  const output = stream.Writable.toWeb(process.stdout);
  const input = stream.Readable.toWeb(process.stdin) as ReadableStream<Uint8Array>;
  const connection = app.connect(acp.ndJsonStream(output, input));
  try { await connection.closed; }
  finally { agent.dispose(); }
}

if (import.meta.main) await runBridgeAcpStdio();
