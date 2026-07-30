import {
  appendDecisionFact,
  claimFire,
  decideSuccession,
  DEFAULT_COMMAND_TIMEOUT_MS,
  DEFAULT_COOKED_THRESHOLD,
  DEFAULT_COORDINATOR_MODEL,
  DEFAULT_HEARTBEAT_STALE_MS,
  flushDecisionSpool,
  parseAvailabilityDocument,
  productionCommandRunner,
  readHeartbeatEvidence,
  recordPulse,
  spoolDecisionFact,
  type CommandRunner,
} from "./succession";

const TASK_THREAD = "019fa552-bdf1-7b6d-bfb4-b039889b9610";

interface CliConfig {
  northBin: string;
  thread: string;
  fallbackFile: string;
  markerFile: string;
  pendingFile: string;
  cookedThreshold: number;
  heartbeatStaleMs: number;
  coordinatorModel: string;
  timeoutMs: number;
  fireCommand: string;
  fireArgs: string[];
  fireTimeoutMs: number;
}

function positiveNumber(value: string | undefined, fallback: number, label: string): number {
  if (value === undefined) return fallback;
  const parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed <= 0) throw new Error(`${label} must be positive`);
  return parsed;
}

function config(env: NodeJS.ProcessEnv = process.env): CliConfig {
  const state = env.XDG_STATE_HOME ?? `${env.HOME}/.local/state`;
  const northBin = env.NORTH_BIN ?? "north";
  return {
    northBin,
    thread: env.NORTH_SUCCESSION_THREAD ?? TASK_THREAD,
    fallbackFile: env.NORTH_SUCCESSION_HEARTBEAT_FILE
      ?? `${state}/north/moat-coordinator-heartbeat`,
    markerFile: env.NORTH_SUCCESSION_MARKER_FILE ?? `${state}/north/moat-heir-fired`,
    pendingFile: env.NORTH_SUCCESSION_PENDING_FILE ?? `${state}/north/succession-pending.jsonl`,
    cookedThreshold: positiveNumber(
      env.NORTH_SUCCESSION_COOKED_THRESHOLD,
      DEFAULT_COOKED_THRESHOLD,
      "NORTH_SUCCESSION_COOKED_THRESHOLD",
    ),
    heartbeatStaleMs: positiveNumber(
      env.NORTH_SUCCESSION_HEARTBEAT_STALE_MS,
      DEFAULT_HEARTBEAT_STALE_MS,
      "NORTH_SUCCESSION_HEARTBEAT_STALE_MS",
    ),
    coordinatorModel: env.NORTH_SUCCESSION_COORDINATOR_MODEL ?? DEFAULT_COORDINATOR_MODEL,
    timeoutMs: positiveNumber(
      env.NORTH_SUCCESSION_TIMEOUT_MS,
      DEFAULT_COMMAND_TIMEOUT_MS,
      "NORTH_SUCCESSION_TIMEOUT_MS",
    ),
    fireCommand: env.NORTH_SUCCESSION_FIRE_COMMAND ?? northBin,
    fireArgs: env.NORTH_SUCCESSION_FIRE_ARGS
      ? env.NORTH_SUCCESSION_FIRE_ARGS.split("\u001f")
      : [
          "failover",
          "fire",
          "--thread",
          env.NORTH_SUCCESSION_ROOT_THREAD ?? "019fa4d4-93aa-7447-aae5-0a5bcfca6849",
          "--brief",
          env.NORTH_SUCCESSION_BRIEF ?? "",
        ],
    fireTimeoutMs: positiveNumber(
      env.NORTH_SUCCESSION_FIRE_TIMEOUT_MS,
      5 * 60 * 1_000,
      "NORTH_SUCCESSION_FIRE_TIMEOUT_MS",
    ),
  };
}

export function runSuccessionCli(
  args: string[],
  env: NodeJS.ProcessEnv = process.env,
  run: CommandRunner = productionCommandRunner,
  now = new Date(),
): number {
  const cfg = config(env);
  const [verb] = args;
  if (verb === "pulse") {
    recordPulse({
      northBin: cfg.northBin,
      thread: cfg.thread,
      fallbackFile: cfg.fallbackFile,
      now,
      timeoutMs: cfg.timeoutMs,
      run,
    });
    console.log(JSON.stringify({ action: "pulse", observedAt: now.toISOString() }));
    return 0;
  }
  if (verb !== "check")
    throw new Error("usage: north-succession <pulse|check>");
  if (cfg.fireArgs.includes(""))
    throw new Error("NORTH_SUCCESSION_BRIEF is required for check");

  flushDecisionSpool({
    path: cfg.pendingFile,
    northBin: cfg.northBin,
    thread: cfg.thread,
    timeoutMs: cfg.timeoutMs,
    run,
  });

  const availabilityResult = run(
    cfg.northBin,
    ["account", "availability", "--json"],
    cfg.timeoutMs,
  );
  let availability;
  if (availabilityResult.status === 0 && !availabilityResult.timedOut) {
    try {
      availability = parseAvailabilityDocument(availabilityResult.stdout);
    } catch {
      availability = { accounts: [] };
    }
  } else {
    availability = { accounts: [] };
  }
  const heartbeat = readHeartbeatEvidence({
    northBin: cfg.northBin,
    thread: cfg.thread,
    fallbackFile: cfg.fallbackFile,
    now,
    staleMs: cfg.heartbeatStaleMs,
    timeoutMs: cfg.timeoutMs,
    run,
  });
  const decision = decideSuccession(
    availability,
    heartbeat,
    cfg.cookedThreshold,
    cfg.coordinatorModel,
  );
  const envelope = { decidedAt: now.toISOString(), decision };
  try {
    appendDecisionFact({
      northBin: cfg.northBin,
      thread: cfg.thread,
      predicate: "succession_decision",
      value: envelope,
      timeoutMs: cfg.timeoutMs,
      run,
    });
  } catch {
    spoolDecisionFact(cfg.pendingFile, { predicate: "succession_decision", value: envelope });
  }

  if (decision.action === "fire" && claimFire(cfg.markerFile, decision, now)) {
    const fire = run(cfg.fireCommand, cfg.fireArgs, cfg.fireTimeoutMs);
    const result = {
      firedAt: now.toISOString(),
      command: [cfg.fireCommand, ...cfg.fireArgs],
      status: fire.status,
      timedOut: fire.timedOut,
      decision,
    };
    try {
      appendDecisionFact({
        northBin: cfg.northBin,
        thread: cfg.thread,
        predicate: "succession_fire",
        value: result,
        timeoutMs: cfg.timeoutMs,
        run,
      });
    } catch {
      spoolDecisionFact(cfg.pendingFile, { predicate: "succession_fire", value: result });
    }
    console.log(JSON.stringify(result));
    return fire.status === 0 && !fire.timedOut ? 0 : 1;
  }

  console.log(JSON.stringify(envelope));
  return 0;
}

if (import.meta.main) {
  try {
    process.exit(runSuccessionCli(process.argv.slice(2)));
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exit(1);
  }
}
