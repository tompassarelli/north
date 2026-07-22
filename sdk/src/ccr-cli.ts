#!/usr/bin/env bun
import { readFileSync } from "node:fs";
import {
  CCR_COVERAGE, prepareCcrOutput, retrieveCcrOutput,
  type CcrPrepareResult, type CcrRetrieveResult,
} from "./ccr";

const USAGE = `usage:
  north ccr prepare --workspace ID --run ID [--enable] [--ttl-seconds N] [--state-dir PATH]
  north ccr retrieve --workspace ID --run ID [--state-dir PATH] HASH

CCR is an explicit canary only. prepare is byte-exact passthrough unless --enable
is present and durable compression succeeds before provider side effects.`;

interface Arguments {
  command: "prepare" | "retrieve";
  workspace: string;
  run: string;
  stateDir?: string;
  ttlMs?: number;
  enabled: boolean;
  providerSideEffectsStarted: boolean;
  hash?: string;
}

function usage(message?: string): never {
  if (message) process.stderr.write(`north ccr: ${message}\n`);
  process.stderr.write(`${USAGE}\n`);
  process.exit(2);
}

function takeValue(argv: string[], index: number, flag: string): string {
  const value = argv[index + 1];
  if (!value || value.startsWith("--")) usage(`${flag} requires a value`);
  argv.splice(index, 2);
  return value;
}

function parseArguments(input: string[]): Arguments {
  const argv = [...input];
  const command = argv.shift();
  if (command === undefined || command === "--help" || command === "-h") usage();
  if (command !== "prepare" && command !== "retrieve") usage(`unknown command ${command}`);

  let workspace: string | undefined;
  let run: string | undefined;
  let stateDir: string | undefined;
  let ttlMs: number | undefined;
  let enabled = false;
  let providerSideEffectsStarted = false;
  for (let index = 0; index < argv.length;) {
    const flag = argv[index];
    if (flag === "--workspace") workspace = takeValue(argv, index, flag);
    else if (flag === "--run") run = takeValue(argv, index, flag);
    else if (flag === "--state-dir") stateDir = takeValue(argv, index, flag);
    else if (flag === "--ttl-seconds") {
      const raw = takeValue(argv, index, flag);
      const seconds = Number(raw);
      if (!Number.isSafeInteger(seconds) || seconds <= 0) usage("--ttl-seconds must be a positive integer");
      ttlMs = seconds * 1_000;
      if (!Number.isSafeInteger(ttlMs)) usage("--ttl-seconds is too large");
    } else if (flag === "--enable") {
      enabled = true;
      argv.splice(index, 1);
    } else if (flag === "--provider-side-effects-started") {
      providerSideEffectsStarted = true;
      argv.splice(index, 1);
    } else index++;
  }
  if (!workspace) usage("--workspace is required");
  if (!run) usage("--run is required");
  if (command === "retrieve") {
    if (enabled || providerSideEffectsStarted || ttlMs !== undefined)
      usage("retrieve does not accept prepare-only flags");
    if (argv.length !== 1 || !/^[0-9a-f]{16}$/.test(argv[0])) usage("retrieve requires one 16-hex HASH");
    return { command, workspace, run, stateDir, enabled, providerSideEffectsStarted, hash: argv[0] };
  }
  if (argv.length !== 0) usage(`unexpected argument ${argv[0]}`);
  return { command, workspace, run, stateDir, ttlMs, enabled, providerSideEffectsStarted };
}

function reportPrepare(result: CcrPrepareResult<unknown, object>): void {
  process.stderr.write(`${JSON.stringify({
    version: 1,
    coverage: result.coverage,
    decision: result.telemetry.decision,
    transformed: result.transformed,
    hash: result.hash,
    dropped: result.dropped,
    markerOutcome: result.telemetry.markerOutcome,
    storeOutcome: result.telemetry.storeOutcome,
    retrievalOutcome: result.telemetry.retrievalOutcome,
    fallbackReason: result.telemetry.fallbackReason,
    telemetryRecorded: result.telemetryRecorded,
  })}\n`);
}

function reportRetrieve(result: CcrRetrieveResult): void {
  process.stderr.write(`${JSON.stringify({
    version: 1,
    coverage: result.coverage,
    decision: result.telemetry.decision,
    hash: result.telemetry.hash,
    retrievalOutcome: result.status,
    telemetryRecorded: result.telemetryRecorded,
  })}\n`);
}

async function prepare(args: Arguments): Promise<number> {
  const raw = readFileSync(0);
  let output: unknown;
  try { output = JSON.parse(raw.toString("utf8")); }
  catch {
    process.stdout.write(raw);
    process.stderr.write(`${JSON.stringify({
      version: 1, coverage: CCR_COVERAGE, decision: "fallback", transformed: false,
      fallbackReason: "invalid_json_array", telemetryRecorded: false,
    })}\n`);
    return 0;
  }
  if (!Array.isArray(output)) {
    process.stdout.write(raw);
    process.stderr.write(`${JSON.stringify({
      version: 1, coverage: CCR_COVERAGE, decision: "fallback", transformed: false,
      fallbackReason: "invalid_json_array", telemetryRecorded: false,
    })}\n`);
    return 0;
  }

  const stablePrefix = Object.freeze({ canary: true });
  let result: CcrPrepareResult<unknown, object>;
  try {
    result = await prepareCcrOutput({
      output,
      stablePrefix,
      workspaceIdentity: args.workspace,
      managedRunId: args.run,
      stateDir: args.stateDir,
      ttlMs: args.ttlMs,
      enabled: args.enabled,
      providerSideEffectsStarted: args.providerSideEffectsStarted,
    });
  } catch {
    process.stdout.write(raw);
    process.stderr.write(`${JSON.stringify({
      version: 1, coverage: CCR_COVERAGE, decision: "fallback", transformed: false,
      fallbackReason: "compression_error", telemetryRecorded: false,
    })}\n`);
    return 0;
  }
  if (result.transformed) process.stdout.write(`${JSON.stringify(result.output)}\n`);
  else process.stdout.write(raw);
  reportPrepare(result);
  return 0;
}

async function retrieve(args: Arguments): Promise<number> {
  const result = await retrieveCcrOutput({
    workspaceIdentity: args.workspace,
    managedRunId: args.run,
    stateDir: args.stateDir,
    hash: args.hash!,
  });
  if (result.status === "hit") process.stdout.write(`${JSON.stringify(result.output)}\n`);
  reportRetrieve(result);
  return result.status === "hit" ? 0 : result.status === "miss" ? 3 : result.status === "expired" ? 4 : 5;
}

export async function main(argv = process.argv.slice(2)): Promise<number> {
  const args = parseArguments(argv);
  return args.command === "prepare" ? prepare(args) : retrieve(args);
}

if (import.meta.main) {
  try { process.exitCode = await main(); }
  catch {
    // prepare's implementation catches transform/storage failures and writes the
    // exact input. Reaching here means argument/scope setup failed before stdin.
    process.stderr.write(`${JSON.stringify({
      version: 1, coverage: CCR_COVERAGE, decision: "fallback",
      fallbackReason: "compression_error", telemetryRecorded: false,
    })}\n`);
    process.exitCode = 2;
  }
}
