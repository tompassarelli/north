import { validAgentEntity } from "./delivery-verification";

export function normalizedAgentId(raw: string | undefined): string {
  const value = raw?.replace(/^@?agent:/, "").trim() ?? "";
  if (!validAgentEntity(`@agent:${value}`)) {
    throw new Error("delivery attest requires a valid target agent id");
  }
  return value;
}

export function attestationActorFromEnv(env: NodeJS.ProcessEnv = process.env): string {
  const raw = env.AGENT_ID?.replace(/^@?agent:/, "").trim() ?? "";
  if (!validAgentEntity(`@agent:${raw}`)) {
    throw new Error(
      "delivery attest must run inside a North-managed verifier or judge lane (AGENT_ID unavailable)",
    );
  }
  return `agent:${raw}`;
}

export async function attestDelivery(
  target: string,
): Promise<string> {
  normalizedAgentId(target);
  throw new Error(
    "independent delivery attestation is unavailable: managed lanes share one OS uid, "
    + "so AGENT_ID cannot prove a second verifier. Delivery remains reported and is "
    + "excluded from template promotion until an isolated verifier capability exists",
  );
}

if (import.meta.main) {
  const [verb, target, ...extra] = process.argv.slice(2);
  if (verb !== "attest" || !target || extra.length) {
    console.error("usage: north delivery attest <target-agent-id>");
    process.exit(2);
  }
  try {
    console.log(await attestDelivery(target));
  } catch (error) {
    console.error(`north delivery attest: ${error instanceof Error ? error.message : String(error)}`);
    process.exit(1);
  }
}
