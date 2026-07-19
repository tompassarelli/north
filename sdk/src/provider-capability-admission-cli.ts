import { ExecutionAdmissionError, admitPinnedProvider } from "./execution-admission";
import { requireGafferCapabilities, type GafferCapability } from "./gaffer-capabilities";
import type { ProviderId } from "./providers/types";
import { parseStrictJson } from "./strict-json";

export const PROVIDER_CAPABILITY_ADMISSION_SCHEMA =
  "north:provider-capability-admission:v1" as const;

interface ProviderCapabilityAdmissionBase {
  schema: typeof PROVIDER_CAPABILITY_ADMISSION_SCHEMA;
  provider: ProviderId;
  capabilities: GafferCapability[];
  requestedTarget?: string;
}

export type ProviderCapabilityAdmission =
  | (ProviderCapabilityAdmissionBase & { status: "supported" })
  | (ProviderCapabilityAdmissionBase & {
      status: "unsupported";
      code: string;
      processOutcome: string;
      reason: string;
      retrySafeBeforeAcceptance: true;
    });

function providerId(value: unknown): ProviderId {
  if (value !== "anthropic" && value !== "openai")
    throw new Error("provider must be anthropic or openai");
  return value;
}

function requestedTarget(value: unknown): string | undefined {
  if (value === undefined) return undefined;
  if (typeof value !== "string" || !value.trim() || value !== value.trim())
    throw new Error("requested target must be a non-empty trimmed string");
  return value;
}

/**
 * Side-effect-free preview of the exact pinned-provider capability gate used by
 * spawn and dispatch before a provider can accept a turn.
 */
export function inspectPinnedProviderCapabilityAdmission(
  providerValue: unknown,
  capabilitiesValue: unknown,
  requestedTargetValue?: unknown,
): ProviderCapabilityAdmission {
  const provider = providerId(providerValue);
  const capabilities = requireGafferCapabilities(
    capabilitiesValue, "provider capability admission capabilities",
  );
  const target = requestedTarget(requestedTargetValue);
  const base: ProviderCapabilityAdmissionBase = {
    schema: PROVIDER_CAPABILITY_ADMISSION_SCHEMA,
    provider,
    capabilities,
    ...(target === undefined ? {} : { requestedTarget: target }),
  };
  try {
    // This is intentionally the production function, not a parallel policy
    // table. A dry-run and a real pinned execution therefore cannot disagree.
    admitPinnedProvider(provider, capabilities);
    return { ...base, status: "supported" };
  } catch (error) {
    if (!(error instanceof ExecutionAdmissionError)) throw error;
    return {
      ...base,
      status: "unsupported",
      code: error.code,
      processOutcome: error.processOutcome,
      reason: error.message,
      retrySafeBeforeAcceptance: true,
    };
  }
}

function usage(): never {
  console.error(
    "usage: bun run provider-capability-admission-cli.ts <anthropic|openai> <capabilities-json> [target]",
  );
  process.exit(2);
}

if (import.meta.main) {
  const [provider, capabilitiesJson, target, ...extra] = process.argv.slice(2);
  if (!provider || !capabilitiesJson || extra.length) usage();
  try {
    const result = inspectPinnedProviderCapabilityAdmission(
      provider,
      parseStrictJson(capabilitiesJson, "provider capability admission capabilities", {
        maxBytes: 4_096,
        maxDepth: 2,
        maxNodes: 32,
      }),
      target,
    );
    process.stdout.write(`${JSON.stringify(result)}\n`);
    if (result.status === "unsupported") process.exitCode = 3;
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 2;
  }
}
