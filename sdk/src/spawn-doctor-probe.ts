import {
  CODEX_MANAGED_HOOKS_DIR, CODEX_MANAGED_REQUIREMENTS, reportManagedCodexHookInstallation,
  type ManagedCodexHookReport,
} from "./providers/codex-managed-hooks";
import { loadOrchestrationStaffing } from "./orchestration-staffing";
import { providerCapabilityRejectionCode } from "./orchestration-capabilities";

// `north spawn --doctor`'s TypeScript-side facts. Every verdict is produced by
// the function the real spawn path calls, never a restated policy table.

const NIX_STORE_ROOT = "/nix/store";
const NORTH_ENFORCEMENT_ROOT = "/var/lib/north-enforcement";

export interface PresetSandboxVerdict {
  role: string;
  topology: string;
  capabilities: string[];
  openai: "workspace-write" | "read-only" | "rejected";
  /** A model-issued shell command cannot reach North from the read-only sandbox. */
  directShellLoopback: "open" | "closed";
  /** Coordination-capable Codex lanes call the required North MCP hosted outside the sandbox. */
  coordinationTransport: "north-mcp-host" | "not-granted";
  sandboxNetwork: "open" | "closed";
  rejection?: string;
}

export interface SpawnDoctorProbe {
  schema: "north:spawn-doctor-probe:v1";
  managedCodexHooks: ManagedCodexHookReport;
  presets: PresetSandboxVerdict[];
}

function presetSandboxVerdicts(): PresetSandboxVerdict[] {
  return loadOrchestrationStaffing().presets.map((preset) => {
    const capabilities = [...preset.capabilities];
    const rejection = providerCapabilityRejectionCode("openai", preset.capabilities);
    const readOnly = capabilities.includes("shell.readonly");
    return {
      role: preset.name,
      topology: preset.topology,
      capabilities,
      openai: rejection ? "rejected" : readOnly ? "read-only" : "workspace-write",
      directShellLoopback: readOnly ? "closed" : "open",
      coordinationTransport: !rejection && capabilities.includes("coordination")
        ? "north-mcp-host"
        : "not-granted",
      sandboxNetwork: readOnly ? "closed" : "open",
      ...(rejection ? { rejection } : {}),
    };
  });
}

export function spawnDoctorProbe(): SpawnDoctorProbe {
  return {
    schema: "north:spawn-doctor-probe:v1",
    managedCodexHooks: reportManagedCodexHookInstallation({
      requirementsPath: CODEX_MANAGED_REQUIREMENTS,
      managedDir: CODEX_MANAGED_HOOKS_DIR,
      nixStoreRoot: NIX_STORE_ROOT,
      enforcementRoot: NORTH_ENFORCEMENT_ROOT,
      expectedOwnerUid: 0,
    }),
    presets: presetSandboxVerdicts(),
  };
}

if (import.meta.main) {
  if (process.argv.slice(2).length) {
    console.error("usage: bun run spawn-doctor-probe.ts");
    process.exit(2);
  }
  try {
    process.stdout.write(`${JSON.stringify(spawnDoctorProbe())}\n`);
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 2;
  }
}
