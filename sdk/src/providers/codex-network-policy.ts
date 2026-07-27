import { FRAM_GRAPH_AUTHORING_CAPABILITY } from "../fram-graph-authoring";
import type { OrchestrationCapability } from "../orchestration-capabilities";

export interface ManagedCodexNetworkSubject {
  sandbox: "read-only" | "workspace-write";
  capabilities: readonly OrchestrationCapability[];
}

/**
 * A workspace-write graph-authoring lane needs a local daemon connection;
 * Codex exposes that through the same network_access bit as web access. The
 * Gitiles proxy remains web-only, so graph-authoring never gains public web.
 */
export function managedCodexNetworkPolicy(subject: ManagedCodexNetworkSubject): {
  networkAccess: boolean;
  networkProxyEnabled: boolean;
  domains: Record<string, "allow">;
} {
  const workspaceWrite = subject.sandbox === "workspace-write";
  const web = workspaceWrite && subject.capabilities.includes("web");
  const daemonAccess = workspaceWrite
    && subject.capabilities.includes(FRAM_GRAPH_AUTHORING_CAPABILITY);
  const domains: Record<string, "allow"> = web
    ? { "chromium.googlesource.com": "allow" }
    : {};
  return Object.freeze({
    networkAccess: web || daemonAccess,
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
