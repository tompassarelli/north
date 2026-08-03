import { accessSync, constants, statSync } from "node:fs";
import { join } from "node:path";
import {
  CODEX_MANAGED_HOOKS_DIR, CODEX_MANAGED_REQUIREMENTS, reportManagedCodexHookInstallation,
  type ManagedCodexHookReport,
} from "./providers/codex-managed-hooks";
import {
  FRAM_GRAPH_AUTHORING_CAPABILITY, FRAM_MCP_TOOL_NAMES, FRAM_MCP_SERVER,
  graphAuthoringRoot,
} from "./fram-graph-authoring";
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
  /** Codex's read-only sandbox blocks :7977, so a coordinating lane cannot claim. */
  coordinationUnderReadOnly: boolean;
  rejection?: string;
}

export interface GraphAuthoringProbe {
  capability: string;
  server: string;
  /** Tool names North grants a graph-authoring lane; the server must advertise all of them. */
  declaredTools: string[];
  framHome: string | null;
  beagleHome: string | null;
  rootsError?: string;
  framMcpCommand?: string;
  framMcpExecutable?: boolean;
  framDaemonCommand?: string;
  framDaemonExecutable?: boolean;
  checkouts: Array<{ name: string; path: string; exists: boolean; isGitCheckout: boolean }>;
}

export interface SpawnDoctorProbe {
  schema: "north:spawn-doctor-probe:v1";
  managedCodexHooks: ManagedCodexHookReport;
  presets: PresetSandboxVerdict[];
  graphAuthoring: GraphAuthoringProbe;
}

function executable(path: string): boolean {
  try {
    accessSync(path, constants.X_OK);
    return statSync(path).isFile();
  } catch {
    return false;
  }
}

function directory(path: string): boolean {
  try {
    return statSync(path).isDirectory();
  } catch {
    return false;
  }
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
      coordinationUnderReadOnly: !rejection && readOnly && capabilities.includes("coordination"),
      ...(rejection ? { rejection } : {}),
    };
  });
}

function graphAuthoringProbe(): GraphAuthoringProbe {
  const base: GraphAuthoringProbe = {
    capability: FRAM_GRAPH_AUTHORING_CAPABILITY,
    server: FRAM_MCP_SERVER,
    declaredTools: [...FRAM_MCP_TOOL_NAMES],
    // Same resolver the real spawn path uses, so a bare env reports the
    // standard-layout default rather than a wall the lane would not hit.
    framHome: graphAuthoringRoot("NORTH_FRAM_HOME") ?? null,
    beagleHome: graphAuthoringRoot("NORTH_BEAGLE_HOME") ?? null,
    checkouts: [],
  };
  if (!base.framHome || !base.beagleHome) {
    const missing = [
      !base.framHome && "NORTH_FRAM_HOME", !base.beagleHome && "NORTH_BEAGLE_HOME",
    ].filter(Boolean).join(", ");
    return { ...base, rootsError: `graph_authoring_fram_roots_unset: missing ${missing}` };
  }
  const framMcpCommand = join(base.framHome, "bin", "fram-mcp");
  const framDaemonCommand = join(base.framHome, "bin", "fram-daemon");
  return {
    ...base,
    framMcpCommand,
    framMcpExecutable: executable(framMcpCommand),
    framDaemonCommand,
    framDaemonExecutable: executable(framDaemonCommand),
    checkouts: [
      { name: "NORTH_FRAM_HOME", path: base.framHome },
      { name: "NORTH_BEAGLE_HOME", path: base.beagleHome },
    ].map(({ name, path }) => ({
      name,
      path,
      exists: directory(path),
      isGitCheckout: directory(join(path, ".git")) || executable(join(path, ".git")),
    })),
  };
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
    graphAuthoring: graphAuthoringProbe(),
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
