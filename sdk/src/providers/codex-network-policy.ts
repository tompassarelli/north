import type { OrchestrationCapability } from "../orchestration-capabilities";

export interface ManagedCodexNetworkSubject {
  sandbox: "read-only" | "workspace-write";
  capabilities: readonly OrchestrationCapability[];
}

/**
 * Workspace-write lanes receive Codex's network_access default — that governs
 * what a *sandboxed shell command* may reach, so it correctly tracks the shell
 * surface.
 *
 * `web` does NOT track the shell surface, and previously did: gating it on
 * workspace-write silently dropped declared web access from every read-only
 * lane. That hit every orchestrator template — director, team-lead, program,
 * portfolio all carry `shell.readonly` by design — so an OpenAI orchestrator
 * declared web-capable launched with `--disable network_proxy` while the
 * unsandboxed Anthropic orchestrator kept its web. Read-only + web is a
 * coherent, common shape: coordinate and research, execute nothing.
 */
export function managedCodexNetworkPolicy(subject: ManagedCodexNetworkSubject): {
  networkAccess: boolean;
  networkProxyEnabled: boolean;
  domains: Record<string, "allow">;
} {
  const workspaceWrite = subject.sandbox === "workspace-write";
  const web = subject.capabilities.includes("web");
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
