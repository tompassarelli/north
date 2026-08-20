import { safeNext, type StoreAction } from "./store-kernel";
import {
  loadStoreSnapshot,
  type LoadStoreSnapshotOptions,
  type LoadedStoreSnapshot,
} from "./store-kernel-loader";
import { StoreRpcClient } from "./store-rpc-client";

const ATTEMPT = /^@attempt:[0-9a-f]{64}$/;
const USAGE = `usage: north recover <@attempt:sha256> [--json]

Read one Store snapshot and report the conservative safe next action for the
selected execution attempt. This command is read-only; it never selects a new
account, changes a route, or performs the reported action.`;

type RecoveryClient = Pick<StoreRpcClient, "scanAll" | "close">;

export interface StoreRecoveryCliRuntime {
  readonly connect?: () => Promise<RecoveryClient>;
  readonly loadSnapshot?: (
    options: LoadStoreSnapshotOptions,
  ) => Promise<LoadedStoreSnapshot>;
  readonly stdout?: (output: string) => void;
  readonly stderr?: (output: string) => void;
}

interface RecoveryReport {
  readonly version: "north:store-recovery-report:v2";
  readonly attempt: string;
  readonly servedVersion: number;
  readonly account: LoadedStoreSnapshot["snapshot"]["account"];
  readonly authority: NonNullable<LoadedStoreSnapshot["authority"]>;
  readonly action: StoreAction;
}

interface RecoveryRequest {
  readonly attempt: string;
  readonly json: boolean;
}

function parseArgs(argv: readonly string[]): RecoveryRequest | "help" {
  if (argv.length === 1 && ["help", "--help", "-h"].includes(argv[0]!)) return "help";
  const attempt = argv.find((argument) => argument !== "--json");
  const jsonCount = argv.filter((argument) => argument === "--json").length;
  if (!attempt || !ATTEMPT.test(attempt) || jsonCount > 1
    || argv.length !== 1 + jsonCount
    || argv.some((argument) => argument !== attempt && argument !== "--json")) {
    throw new Error(USAGE);
  }
  return { attempt, json: jsonCount === 1 };
}

function reportFor(
  attempt: string,
  loaded: LoadedStoreSnapshot,
): RecoveryReport {
  const action = safeNext(loaded.snapshot);
  if (action.kind === "invalid") {
    throw new Error(`Store kernel refused the reconstructed snapshot (${action.reason})`);
  }
  if (!loaded.authority || loaded.authority.subject !== attempt
    || loaded.authority.accountId !== loaded.snapshot.account.id) {
    throw new Error("Store snapshot omitted the selected attempt authority");
  }
  return {
    version: "north:store-recovery-report:v2",
    attempt,
    servedVersion: loaded.servedVersion,
    account: loaded.snapshot.account,
    authority: loaded.authority,
    action,
  };
}

function renderHuman(report: RecoveryReport): string {
  const reason = report.action.kind === "no-op" ? ` (${report.action.reason})` : "";
  const command = "command" in report.action
    ? [`command ${report.action.command.subject}`]
    : [];
  return [
    `attempt ${report.attempt}`,
    `Store version ${report.servedVersion}`,
    `route ${report.authority.provider}/${report.authority.accountId}/${report.authority.model} (${report.account.account_role})`,
    `work ${report.authority.threadId} run=${report.authority.runId} reporter=${report.authority.reporterAgentId}`,
    `capability ${report.authority.runCapabilitySha256} contract=${report.authority.runContractSha256}`,
    `authorization account=${report.authority.accountAuthorityReceiptSha256} route=${report.authority.routeObservationReceiptSha256}`,
    `thread lease ${report.authority.threadLease.resource} holder=${report.authority.threadLease.holder} epoch=${report.authority.threadLease.epoch}`,
    `account lease ${report.authority.accountLease.resource} holder=${report.authority.accountLease.holder} epoch=${report.authority.accountLease.epoch}`,
    `safe next ${report.action.kind}${reason}`,
    `replay cursor ${report.action.replayPosition}`,
    ...command,
  ].join("\n");
}

export async function runStoreRecoveryCli(
  argv: readonly string[],
  runtime: StoreRecoveryCliRuntime = {},
): Promise<number> {
  const out = runtime.stdout ?? console.log;
  const err = runtime.stderr ?? console.error;
  let request: RecoveryRequest | "help";
  try {
    request = parseArgs(argv);
  } catch (error) {
    err(error instanceof Error ? error.message : String(error));
    return 2;
  }
  if (request === "help") {
    out(USAGE);
    return 0;
  }

  let client: RecoveryClient | undefined;
  try {
    client = await (runtime.connect ?? (() => StoreRpcClient.connect()))();
    const loaded = await (runtime.loadSnapshot ?? loadStoreSnapshot)({
      attemptId: request.attempt,
      client,
    });
    const report = reportFor(request.attempt, loaded);
    out(request.json ? JSON.stringify(report, null, 2) : renderHuman(report));
    return 0;
  } catch (error) {
    err(`north recover: ${error instanceof Error ? error.message : String(error)}`);
    return 1;
  } finally {
    client?.close();
  }
}

if (import.meta.main) process.exitCode = await runStoreRecoveryCli(process.argv.slice(2));
