import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join, resolve } from "node:path";
import { expectedLog } from "./coord-wire";

export const FRAM_GRAPH_AUTHORING_CAPABILITY = "graph-authoring.fram" as const;
export const FRAM_MCP_SERVER = "fram" as const;
export const FRAM_MCP_TOOL_NAMES = Object.freeze([
  "tell",
  "retract",
  "show",
  "ask",
  "validate",
  "add-def",
  "set-body",
  "rename-def",
  "insert-after",
  "replace-in-body",
] as const);
export const FRAM_MCP_TOOLS = Object.freeze(
  FRAM_MCP_TOOL_NAMES.map((name) => `mcp__${FRAM_MCP_SERVER}__${name}`),
);

// The fram and beagle roots are deployment facts, not source constants: the
// nix package purity gate rejects checkout paths baked into the installed
// CLI. Every path below derives lazily from these two roots, so composing
// the capability without them fails closed with a named error while every
// non-capability spawn never touches this resolution at all.
export function framGraphAuthoringRoots(): { framHome: string; beagleHome: string } {
  const framHome = process.env.NORTH_FRAM_HOME;
  const beagleHome = process.env.NORTH_BEAGLE_HOME;
  if (!framHome || !beagleHome) {
    const missing = [
      !framHome && "NORTH_FRAM_HOME",
      !beagleHome && "NORTH_BEAGLE_HOME",
    ].filter(Boolean).join(", ");
    throw new Error(
      "graph_authoring_fram_roots_unset: the graph-authoring.fram capability "
      + `requires NORTH_FRAM_HOME and NORTH_BEAGLE_HOME in the dispatching `
      + `environment (missing: ${missing})`,
    );
  }
  return { framHome: resolve(framHome), beagleHome: resolve(beagleHome) };
}

export function framMcpCommand(): string {
  return join(framGraphAuthoringRoots().framHome, "bin", "fram-mcp");
}

// FRAM_CODE_PORT names the WARM code coordinator for the fram checkout itself
// (framGraphAuthoringRoots().framHome, flipped by `fram-code-on ~/code/fram`),
// not the target repo a lane happens to be authoring in. `fram-code-on` writes
// that port into framHome/.mcp.json (mcpServers.fram.env.FRAM_CODE_PORT) on
// every flip/re-warm; `fram-code-status` reads the exact same field to report
// port=/coord=. Reading it here at spawn time — rather than baking in a
// literal — is the only way the value can track a coordinator that fram-code-on
// has moved three times in one evening. No .mcp.json (or no matching field)
// means the code coordinator was never flipped for this framHome: fail closed
// by name rather than pointing the MCP server at a port nothing listens on.
function framFlipEnvironment(framHome: string): Record<string, unknown> {
  const mcpJsonPath = join(framHome, ".mcp.json");
  let raw: string;
  try {
    raw = readFileSync(mcpJsonPath, "utf8");
  } catch (cause) {
    throw new Error(
      "graph_authoring_fram_code_port_unresolved: no "
      + `${mcpJsonPath} — run \`fram-code-on ${framHome}\` to boot the warm code `
      + "coordinator before composing the graph-authoring.fram capability",
      { cause },
    );
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch (cause) {
    throw new Error(
      `graph_authoring_fram_code_port_unresolved: ${mcpJsonPath} is not valid JSON`,
      { cause },
    );
  }
  return (parsed as { mcpServers?: { fram?: { env?: Record<string, unknown> } } })
    ?.mcpServers?.fram?.env ?? {};
}

function resolveFramCodePort(flipEnv: Record<string, unknown>, framHome: string): string {
  const port = flipEnv.FRAM_CODE_PORT;
  if (typeof port !== "string" || !/^[1-9][0-9]*$/.test(port)) {
    throw new Error(
      `graph_authoring_fram_code_port_unresolved: ${join(framHome, ".mcp.json")} has no `
      + "mcpServers.fram.env.FRAM_CODE_PORT — the same field fram-code-status reads "
      + "to report the live coordinator port",
    );
  }
  return port;
}

// The interactive flip records how long an edit-prepare may take against ITS
// code log (a ~350k-fact log costs ~3s, far past fram.rt's 2000ms default read
// deadline; without the override every managed graph edit fails with a
// prepare-deadline REJECTED that reads like a semantic rejection — learning on
// thread 019f9cf1-d746). A managed lane must inherit the same budget the flip
// wrote, never a value baked in here; a framHome whose flip declares none keeps
// fram's own default rather than acquiring a synthetic one.
function resolveFramCoordReadTimeout(
  flipEnv: Record<string, unknown>,
): Readonly<Record<string, string>> {
  const timeout = flipEnv.FRAM_COORD_READ_TIMEOUT_MS;
  return typeof timeout === "string" && /^[1-9][0-9]*$/.test(timeout)
    ? { FRAM_COORD_READ_TIMEOUT_MS: timeout }
    : {};
}

// FRAM_LOG/FRAM_THREADS select the CORPUS a read (show/ask/validate) folds —
// bin/fram-mcp is explicit that both are required. The dispatching process
// (any invocation routed through bin/north) already exports both with the
// exact defaults and log-split logic reproduced here via expectedLog(); a
// managed lane's fram MCP must read the SAME corpus its own north MCP reads,
// never a hardcoded home path baked into this module.
function corpusFramEnv(): Readonly<Record<string, string>> {
  return Object.freeze({
    FRAM_LOG: expectedLog(),
    FRAM_THREADS: resolve(
      process.env.FRAM_THREADS ?? join(homedir(), ".local", "state", "north", "threads"),
    ),
  });
}

function staticFramMcpEnv(): Readonly<Record<string, string>> {
  const { framHome, beagleHome } = framGraphAuthoringRoots();
  const flipEnv = framFlipEnvironment(framHome);
  return Object.freeze({
    FRAM_FLIP: "1",
    FRAM_GRAPH_EDIT: "1",
    FRAM_CODE_PORT: resolveFramCodePort(flipEnv, framHome),
    ...resolveFramCoordReadTimeout(flipEnv),
    FRAM_CODE_LOG: join(framHome, ".fram", "code.log"),
    ...corpusFramEnv(),
    FRAM_OUT: join(framHome, "out"),
    FRAM_BIN: join(framHome, "bin"),
    FRAM_RESOLVE: join(framHome, "chartroom", "src", "resolve.clj"),
    FRAM_ROUNDTRIP: join(beagleHome, "beagle-lib", "private", "facts-roundtrip.rkt"),
    FRAM_CHECK_EMIT: join(beagleHome, "beagle-lib", "private", "facts-check-emit.rkt"),
    FRAM_BUILD_ALL: join(beagleHome, "bin", "beagle-build-all"),
    BEAGLE_HOME: beagleHome,
  });
}

export function framMcpEnvironment(cwd: string): Readonly<Record<string, string>> {
  const source = resolve(cwd);
  return Object.freeze({
    ...staticFramMcpEnv(),
    FRAM_SRC: source,
  });
}

export function framMcpServer(cwd: string) {
  return Object.freeze({
    type: "stdio" as const,
    command: framMcpCommand(),
    args: Object.freeze([]) as unknown as string[],
    env: framMcpEnvironment(cwd),
  });
}

function exactStringMap(actual: unknown, expected: Readonly<Record<string, string>>): boolean {
  if (!actual || typeof actual !== "object" || Array.isArray(actual)) return false;
  const entries = Object.entries(actual);
  const expectedEntries = Object.entries(expected);
  return entries.length === expectedEntries.length
    && entries.every(([key, value], index) =>
      key === expectedEntries[index]?.[0] && value === expectedEntries[index]?.[1]);
}

export function hasCanonicalFramMcpServer(server: unknown, cwd: string): boolean {
  if (!server || typeof server !== "object" || Array.isArray(server)) return false;
  const raw = server as Record<string, unknown>;
  return Object.keys(raw).sort().join(",") === "args,command,env,type"
    && raw.type === "stdio"
    && raw.command === framMcpCommand()
    && Array.isArray(raw.args)
    && raw.args.length === 0
    && exactStringMap(raw.env, framMcpEnvironment(cwd));
}
