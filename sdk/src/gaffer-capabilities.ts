import type { ProviderId } from "./providers/types";

export const GAFFER_CAPABILITIES = [
  "filesystem.read",
  "filesystem.search",
  "filesystem.write",
  "shell",
  "shell.readonly",
  "web",
  "coordination",
] as const;
export type GafferCapability = typeof GAFFER_CAPABILITIES[number];

export function requireGafferCapabilities(value: unknown, label = "capabilities"): GafferCapability[] {
  if (!Array.isArray(value) || value.length === 0
      || value.some((capability) => typeof capability !== "string"
        || !GAFFER_CAPABILITIES.includes(capability as GafferCapability))) {
    throw new Error(label + " must be a non-empty array of canonical Gaffer capabilities");
  }
  if (new Set(value).size !== value.length) throw new Error(label + " must not contain duplicates");
  return [...value] as GafferCapability[];
}

export function validateTopologyCapabilities(
  topology: "worker" | "orchestrator",
  capabilities: readonly GafferCapability[],
  label = "capabilities",
): void {
  const has = (capability: GafferCapability) => capabilities.includes(capability);
  if (has("shell") && has("shell.readonly"))
    throw new Error(label + ": shell and shell.readonly are mutually exclusive");
  if (topology === "orchestrator") {
    if (!has("coordination"))
      throw new Error(label + ": orchestrator topology requires coordination capability");
    if (has("filesystem.write"))
      throw new Error(label + ": orchestrator topology forbids filesystem.write capability");
    if (has("shell"))
      throw new Error(label + ": orchestrator topology forbids unrestricted shell capability");
  } else if (has("coordination")) {
    throw new Error(label + ": worker topology forbids coordination capability");
  }
}

/** Exact pre-acceptance reason when an adapter cannot realize the authority. */
export function providerCapabilityRejectionCode(
  provider: ProviderId,
  capabilities: readonly GafferCapability[] | undefined,
): string | undefined {
  if (!capabilities) return undefined;
  const generic = `${provider}_adapter_cannot_enforce_gaffer_capabilities`;
  const fileRead = capabilities.includes("filesystem.read");
  const fileSearch = capabilities.includes("filesystem.search");
  const fileWrite = capabilities.includes("filesystem.write");
  const shell = capabilities.includes("shell");
  const readonlyShell = capabilities.includes("shell.readonly");
  // Both concrete shell surfaces can read and search the checkout, and the
  // unrestricted shell can write it. Reject a bespoke set that omits those
  // effective authorities instead of advertising a narrower boundary than the
  // provider can actually enforce.
  if ((shell || readonlyShell) && (!fileRead || !fileSearch)) return generic;
  if (shell && !fileWrite) return generic;
  if (provider === "anthropic") return undefined;
  // Codex managed workers have an enforceable North MCP surface, but its native
  // exec adapter cannot yet prove child receipt/reconciliation. Orchestrator
  // authority therefore routes elsewhere instead of spending a turn and
  // reporting a coordinator-shaped prompt as operational coordination.
  if (capabilities.includes("coordination"))
    return "openai_adapter_orchestrator_authority_unavailable";
  // `codex exec --search` enables a changing provider-native surface. North has
  // not yet proven its exact item/receipt and policy boundary, so managed web
  // work routes to an adapter that can enforce it instead of silently enabling
  // a capability on faith.
  if (capabilities.includes("web"))
    return "openai_adapter_web_capability_unproven";
  // Codex exec always owns a shell surface. It can hard-sandbox that whole run
  // read-only, but cannot presently make only shell read-only while preserving
  // built-in file edits. Authority shapes outside those two modes route to an
  // adapter that can realize them instead of silently widening or narrowing.
  return (shell && !readonlyShell) || (readonlyShell && !shell && !fileWrite)
    ? undefined
    : generic;
}

/** Whether the adapter can realize this exact authority boundary before work starts. */
export function providerSupportsCapabilities(
  provider: ProviderId,
  capabilities: readonly GafferCapability[] | undefined,
): boolean {
  return providerCapabilityRejectionCode(provider, capabilities) === undefined;
}
