import { writeFileSync } from "node:fs";
import { claimDispatchDriver } from "../../src/dispatch-driver";

const thread = process.argv[2];
const agentId = process.env.AGENT_ID;
const resultFile = process.env.NORTH_TEST_VERIFY_RESULT;

if (!thread || !agentId || !resultFile) process.exit(22);

await Bun.sleep(250);
try {
  claimDispatchDriver(thread, agentId, {
    preclaimed: true,
    port: process.env.NORTH_PORT,
  });
  writeFileSync(resultFile, `VERIFIED @${thread} by ${agentId}\n`);
  // Deliberately skip the SDK-owned release and fail before provider startup.
  // The MCP adapter must observe this detached terminal and clean its preclaim.
  process.exit(23);
} catch (error) {
  writeFileSync(
    resultFile,
    `${error instanceof Error ? `${error.name}: ${error.message}` : String(error)}\n`,
  );
  process.exit(24);
}
