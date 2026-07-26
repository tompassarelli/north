import type { OrchestrationCapability } from "../orchestration-capabilities";

export interface ManagedCodexNetworkSubject {
  sandbox: "read-only" | "workspace-write";
  capabilities: readonly OrchestrationCapability[];
}

/**
 * The sole managed-Codex command-network grant. A web-capable authoring lane
 * needs Gitiles only; every other surface keeps both sandbox networking and
 * the proxy disabled. No caller may add a second network exception.
 */
export function managedCodexNetworkPolicy(subject: ManagedCodexNetworkSubject): {
  networkAccess: boolean;
  networkProxyEnabled: boolean;
  domains: Record<string, "allow">;
} {
  const enabled = subject.sandbox === "workspace-write"
    && subject.capabilities.includes("web");
  const domains: Record<string, "allow"> = enabled
    ? { "chromium.googlesource.com": "allow" }
    : {};
  return Object.freeze({
    networkAccess: enabled,
    networkProxyEnabled: enabled,
    domains: Object.freeze(domains),
  });
}
