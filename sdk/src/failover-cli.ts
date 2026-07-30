import {
  activeSessionRoute,
  activeSessionIdentityFacts,
  checkFailover,
  composeFailoverSpawn,
  fireFailover,
  failoverThreshold,
  loadAvailabilityRows,
  type FailoverRuntime,
} from "./failover";

const USAGE = `usage: north failover <command>

  north failover check [--provider anthropic|openai] [--threshold N]
  north failover fire --thread <root> --brief <path> [--dry-run]

Environment:
  AGENT_PROVIDER / AGENT_TARGET / AGENT_MODEL / AGENT_TIER  active route
  NORTH_FAILOVER_WARN_THRESHOLD                             default 80
  NORTH_FAILOVER_NOTIFY or AGENT_COORDINATOR                human notification target
  NORTH_FAILOVER_AUTO_FIRE                                  default off; 1|true|on enables
  NORTH_FAILOVER_ROOT_THREAD / NORTH_FAILOVER_BRIEF          automatic-fire context`;

interface Parsed {
  values: Record<string, string>;
  flags: Set<string>;
}

export interface FailoverCliRuntime extends FailoverRuntime {
  stdout?: (line: string) => void;
  stderr?: (line: string) => void;
}

function parse(args: string[], valueFlags: Set<string>, booleanFlags = new Set<string>()): Parsed {
  const values: Record<string, string> = {};
  const flags = new Set<string>();
  for (let index = 0; index < args.length; index++) {
    const arg = args[index];
    if (booleanFlags.has(arg)) {
      if (flags.has(arg)) throw new Error(`duplicate ${arg}`);
      flags.add(arg);
      continue;
    }
    if (!valueFlags.has(arg)) throw new Error(`unknown failover option ${arg}`);
    if (values[arg] !== undefined) throw new Error(`duplicate ${arg}`);
    const value = args[++index];
    if (!value || value.startsWith("--")) throw new Error(`${arg} requires a value`);
    values[arg] = value;
  }
  return { values, flags };
}

function threshold(env: NodeJS.ProcessEnv, raw?: string): number {
  return failoverThreshold(raw ?? env.NORTH_FAILOVER_WARN_THRESHOLD ?? 80);
}

function renderCheck(check: ReturnType<typeof checkFailover>): string {
  const active = [
    check.active.provider,
    check.active.account,
    check.active.model,
  ].filter(Boolean).join("/");
  return [
    `active ${active}${check.active.tier ? ` tier=${check.active.tier}` : ""} threshold=${check.threshold}`,
    `classification ${check.classification}`,
    ...(check.unknownReason ? [`unknown ${check.unknownReason}`] : []),
    ...(check.trigger ? [
      `trigger ${check.trigger.rung}:${check.trigger.name} ${check.trigger.pct}% resets=${check.trigger.resetsAt}`,
    ] : []),
    ...(check.heir ? [
      `heir ${check.heir.provider}/${check.heir.account}/${check.heir.model} tier=${check.heir.tier} observed=${check.heir.observedAt}`,
    ] : []),
  ].join("\n");
}

export function runFailoverCli(
  args: string[],
  runtime: FailoverCliRuntime = {},
): number {
  const out = runtime.stdout ?? console.log;
  const err = runtime.stderr ?? console.error;
  const env = runtime.env ?? process.env;
  const loadRows = runtime.loadRows ?? (() => loadAvailabilityRows(runtime.northBin));
  try {
    const [command, ...rest] = args;
    if (command === "help" || command === "--help" || command === "-h") {
      out(USAGE);
      return 0;
    }
    if (command === "check") {
      const parsed = parse(rest, new Set(["--provider", "--threshold"]));
      const rows = loadRows();
      const route = activeSessionRoute(
        rows,
        parsed.values["--provider"],
        env,
        activeSessionIdentityFacts(parsed.values["--provider"], runtime),
      );
      out(renderCheck(checkFailover(rows, route, threshold(env, parsed.values["--threshold"]))));
      return 0;
    }
    if (command === "fire") {
      const parsed = parse(rest, new Set(["--thread", "--brief"]), new Set(["--dry-run"]));
      const rootThread = parsed.values["--thread"];
      const brief = parsed.values["--brief"];
      if (!rootThread || !brief) throw new Error("failover fire requires --thread <root> and --brief <path>");
      const rows = loadRows();
      const route = activeSessionRoute(
        rows,
        undefined,
        env,
        activeSessionIdentityFacts(undefined, runtime),
      );
      const check = checkFailover(rows, route, threshold(env));
      const notify = env.NORTH_FAILOVER_NOTIFY ?? env.AGENT_COORDINATOR ?? "";
      const spawn = composeFailoverSpawn(check, rootThread, brief, notify, runtime);
      if (parsed.flags.has("--dry-run")) {
        out(JSON.stringify(spawn, null, 2));
        return 0;
      }
      fireFailover(spawn, runtime);
      out(`spawned heir ${spawn.check.heir!.provider}/${spawn.check.heir!.account}/${spawn.check.heir!.model} for @${rootThread}`);
      return 0;
    }
    throw new Error(USAGE);
  } catch (error) {
    err(error instanceof Error ? error.message : String(error));
    return 2;
  }
}

if (import.meta.main) process.exit(runFailoverCli(process.argv.slice(2)));
