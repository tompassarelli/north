import type { OrchestrationCapability } from "../orchestration-capabilities";

export interface ManagedCodexNetworkSubject {
  sandbox: "read-only" | "workspace-write";
  capabilities: readonly OrchestrationCapability[];
}

/**
 * Workspace-write lanes receive Codex's network_access default. The Gitiles
 * proxy remains web-only and no capability widens its domain allowlist.
 */
export function managedCodexNetworkPolicy(subject: ManagedCodexNetworkSubject): {
  networkAccess: boolean;
  networkProxyEnabled: boolean;
  domains: Record<string, "allow">;
} {
  const workspaceWrite = subject.sandbox === "workspace-write";
  const web = workspaceWrite && subject.capabilities.includes("web");
  const domains: Record<string, "allow"> = web
    ? { "chromium.googlesource.com": "allow" }
    : {};
  return Object.freeze({
    networkAccess: workspaceWrite,
    networkProxyEnabled: web,
    domains: Object.freeze(domains),
  });
}

/**
 * Executable counterpart to the sole managed-Codex network policy. Codex's
 * `--enable network_proxy` is shorthand for a boolean config assignment, so
 * an enabled proxy must be expressed only through its structured overrides.
 */
export function managedCodexNetworkArguments(subject: ManagedCodexNetworkSubject): string[] {
  const policy = managedCodexNetworkPolicy(subject);
  return policy.networkProxyEnabled
    ? [
      "-c", "features.network_proxy.enabled=true",
      "-c", 'features.network_proxy.domains={"chromium.googlesource.com"="allow"}',
    ]
    : ["--disable", "network_proxy"];
}
