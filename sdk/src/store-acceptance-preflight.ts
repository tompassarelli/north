/**
 * READY-bound acceptance is an evaluator, not another routing authority.  A
 * product adapter supplies the six already-executed command results; this
 * module checks that they all describe the one selected Store release.
 */

export const NORTH_STORE_ACCEPTANCE_JOURNEYS = [
  "threads",
  "ready",
  "dashboard",
  "recover",
  "account-census",
  "authoritative-routing",
] as const;

export type NorthStoreAcceptanceJourney = typeof NORTH_STORE_ACCEPTANCE_JOURNEYS[number];

export interface NorthStoreAcceptanceEvidence {
  readonly releaseId: string;
  readonly socket: string;
  readonly persistenceConfirmed: boolean;
  readonly routingEligible: boolean;
  readonly observedAt: string;
  readonly evidenceMode: "authoritative" | "live-only";
}

/** Full command result is retained verbatim for an operator-facing failure. */
export interface NorthStoreAcceptanceJourneyResult {
  readonly journey: NorthStoreAcceptanceJourney;
  readonly exitCode: number;
  readonly stdout: string;
  readonly stderr: string;
  readonly evidence: NorthStoreAcceptanceEvidence;
  /** Required only for the complete account-census journey. */
  readonly censusAccountIds?: readonly string[];
}

export interface NorthStoreAcceptanceOptions {
  readonly releaseId: string;
  readonly socket: string;
  readonly cutoverAt: string;
  readonly expectedAccountIds: readonly string[];
  readonly output?: (line: string) => void;
}

export interface NorthStoreAcceptanceRuntime {
  runJourney(journey: NorthStoreAcceptanceJourney): Promise<NorthStoreAcceptanceJourneyResult>;
}

export interface NorthStoreAcceptanceReceipt {
  readonly version: "north:store-acceptance-preflight:v1";
  readonly releaseId: string;
  readonly socket: string;
  readonly cutoverAt: string;
  readonly accountIds: readonly string[];
  readonly journeys: readonly NorthStoreAcceptanceJourneyResult[];
}

export class NorthStoreAcceptanceError extends Error {
  constructor(
    readonly failures: readonly string[],
    /** Results already produced remain available even when acceptance refuses. */
    readonly journeys: readonly NorthStoreAcceptanceJourneyResult[],
  ) {
    super(`North Store acceptance refused: ${failures.join("; ")}`);
    this.name = "NorthStoreAcceptanceError";
  }
}

function nonblank(value: string, label: string, failures: string[]): void {
  if (!value.trim()) failures.push(`${label} must be nonblank`);
}

function instant(value: string, label: string, failures: string[]): number | undefined {
  nonblank(value, label, failures);
  const parsed = Date.parse(value);
  if (!Number.isFinite(parsed)) {
    failures.push(`${label} must be an ISO-8601 timestamp`);
    return undefined;
  }
  return parsed;
}

function exactUniqueIds(ids: readonly string[], label: string, failures: string[]): string[] {
  const normalized = ids.map((id) => id.trim());
  if (!normalized.length) failures.push(`${label} must name at least one account`);
  if (normalized.some((id) => !id)) failures.push(`${label} contains a blank account id`);
  if (new Set(normalized).size !== normalized.length) failures.push(`${label} contains duplicate account ids`);
  return [...normalized].sort();
}

function sameIds(left: readonly string[], right: readonly string[]): boolean {
  return left.length === right.length && left.every((value, index) => value === right[index]);
}

function validateJourney(
  journey: NorthStoreAcceptanceJourney,
  result: NorthStoreAcceptanceJourneyResult,
  options: NorthStoreAcceptanceOptions,
  cutoverMs: number | undefined,
  expectedAccounts: readonly string[],
): string[] {
  const failures: string[] = [];
  const prefix = `${journey}`;
  if (result.journey !== journey) failures.push(`${prefix} result is labeled ${result.journey}`);
  if (!Number.isSafeInteger(result.exitCode)) failures.push(`${prefix} exit code is not an integer`);
  else if (result.exitCode !== 0) failures.push(`${prefix} exited ${result.exitCode}`);
  nonblank(result.stdout, `${prefix} stdout`, failures);
  // Empty stderr is valid; the point is to retain it, not manufacture noise.
  if (typeof result.stderr !== "string") failures.push(`${prefix} stderr is not a string`);

  const evidence = result.evidence;
  nonblank(evidence.releaseId, `${prefix} release id`, failures);
  nonblank(evidence.socket, `${prefix} socket`, failures);
  if (evidence.releaseId !== options.releaseId)
    failures.push(`${prefix} release mismatch (${evidence.releaseId} != ${options.releaseId})`);
  if (evidence.socket !== options.socket)
    failures.push(`${prefix} socket mismatch (${evidence.socket} != ${options.socket})`);
  if (evidence.evidenceMode !== "authoritative")
    failures.push(`${prefix} is ${evidence.evidenceMode}; live-only evidence is not acceptance evidence`);
  if (evidence.persistenceConfirmed !== true)
    failures.push(`${prefix} persistence is not confirmed`);
  if (evidence.routingEligible !== true)
    failures.push(`${prefix} routing is not eligible`);
  const observedMs = instant(evidence.observedAt, `${prefix} observedAt`, failures);
  if (cutoverMs !== undefined && observedMs !== undefined && observedMs <= cutoverMs)
    failures.push(`${prefix} observation is not after cutover`);

  if (journey === "account-census") {
    if (!result.censusAccountIds) {
      failures.push("account-census omitted censusAccountIds");
    } else {
      const actual = exactUniqueIds(result.censusAccountIds, "account-census", failures);
      if (!sameIds(actual, expectedAccounts))
        failures.push(`account-census mismatch (${actual.join(",")} != ${expectedAccounts.join(",")})`);
    }
  } else if (result.censusAccountIds !== undefined) {
    failures.push(`${prefix} must not carry account-census data`);
  }
  return failures;
}

/**
 * Execute all six supplied journeys, retain every result, then make one
 * acceptance decision.  The evaluator never opens a Store socket, a provider,
 * or a protected port; its runtime is deliberately injected.
 */
export async function runNorthStoreAcceptancePreflight(
  options: NorthStoreAcceptanceOptions,
  runtime: NorthStoreAcceptanceRuntime,
): Promise<NorthStoreAcceptanceReceipt> {
  const failures: string[] = [];
  nonblank(options.releaseId, "selected release id", failures);
  nonblank(options.socket, "selected Store socket", failures);
  const cutoverMs = instant(options.cutoverAt, "cutoverAt", failures);
  const expectedAccounts = exactUniqueIds(options.expectedAccountIds, "expected account census", failures);

  const completed: Array<{
    readonly required: NorthStoreAcceptanceJourney;
    readonly result: NorthStoreAcceptanceJourneyResult;
  }> = [];
  for (const journey of NORTH_STORE_ACCEPTANCE_JOURNEYS) {
    try {
      completed.push({ required: journey, result: await runtime.runJourney(journey) });
    } catch (error) {
      failures.push(`${journey} invocation failed: ${errorMessage(error)}`);
    }
  }
  const journeys = completed.map(({ result }) => result);
  if (journeys.length !== NORTH_STORE_ACCEPTANCE_JOURNEYS.length)
    failures.push("one or more acceptance journeys produced no command result");
  for (const { required, result } of completed)
    failures.push(...validateJourney(required, result, options, cutoverMs, expectedAccounts));

  if (failures.length) throw new NorthStoreAcceptanceError(failures, journeys);
  const receipt: NorthStoreAcceptanceReceipt = Object.freeze({
    version: "north:store-acceptance-preflight:v1",
    releaseId: options.releaseId,
    socket: options.socket,
    cutoverAt: options.cutoverAt,
    accountIds: Object.freeze(expectedAccounts),
    journeys: Object.freeze(journeys),
  });
  const output = options.output;
  for (const journey of receipt.journeys)
    output?.(`PASS ${journey.journey}: release=${journey.evidence.releaseId} observed=${journey.evidence.observedAt}`);
  output?.(`ACCEPTANCE PASS 6/6 release=${receipt.releaseId}`);
  return receipt;
}

export function errorMessage(error: unknown): string {
  if (error instanceof Error && error.message.trim()) return error.message.trim();
  if (typeof error === "string" && error.trim()) return error.trim();
  try { return JSON.stringify(error) || String(error); }
  catch { return String(error); }
}
