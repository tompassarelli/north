import { createHash, randomUUID } from "node:crypto";
import {
  StoreRpcClient,
  StoreRpcServerError,
  StoreRpcTransportError,
} from "../store-rpc-client";
import {
  RPC_SUBJECT_ANY,
  StoreTriple,
  triple,
  type BatchAction,
  type Term,
} from "../store-rpc-codec";
import { validExecutionAttemptIdentity } from "../delivery-evidence";
import {
  createWireEventStorePublisher,
  wireRunLedgerIdentity,
  type WireEventStorePublisher,
  type WireLedgerBatchWriter,
  type WireRunLedgerIdentity,
} from "../run-ledger";
import { wireRunId, type WireRunId } from "../wire";
import type {
  BridgeLaunchProvider,
} from "./generated/north/bridge/protocol.js";

export interface BridgeAttemptRouteRequest {
  provider?: BridgeLaunchProvider;
  model?: string;
}

export interface BridgeAttemptRouteAuthority {
  attemptId: string;
  provider: BridgeLaunchProvider;
  accountId: string;
  credentialProfile?: string;
  model: string;
  accountAuthorityReceiptSha256: string;
  routeObservationReceiptSha256: string;
  launchIntentSha256: string;
}

export interface BridgeAttemptWireAuthority extends BridgeAttemptRouteAuthority {
  wireRunId: WireRunId;
  wireLedgerIdentity: WireRunLedgerIdentity;
}

export type BridgeCommandKind =
  | "submit-input"
  | "interrupt-turn"
  | "redirect-now"
  | "terminate-session";

export type BridgeCommandDelivery =
  | "queued-next-turn"
  | "active-turn"
  | "interrupt-and-redirect"
  | "session-terminated";

export interface BridgeCommandAdmissionRequest {
  executionId: string;
  attemptId: string;
  kind: BridgeCommandKind;
  payloadDigest: string;
  payloadArtifact: string;
  delivery: BridgeCommandDelivery;
}

export interface BridgeCommandAdmission extends BridgeCommandAdmissionRequest {
  commandId: string;
  ordinal: number;
}

export type BridgeCommandReceiptOutcome = "succeeded" | "failed";

export interface BridgeCommandRecovery {
  pending: BridgeCommandAdmission[];
  unresolvedIntents: BridgeCommandAdmission[];
}

export interface BridgeCommandReceipts {
  bindExecution(
    executionId: string,
    attemptId: string,
    request: BridgeAttemptRouteRequest,
  ): Promise<BridgeAttemptWireAuthority>;
  routeForExecution(executionId: string): Promise<BridgeAttemptWireAuthority>;
  createWirePublisher(authority: BridgeAttemptWireAuthority): WireEventStorePublisher;
  admit(request: BridgeCommandAdmissionRequest): Promise<BridgeCommandAdmission>;
  reconcile(attemptId: string): Promise<BridgeCommandRecovery>;
  commitIntent(command: BridgeCommandAdmission): Promise<void>;
  commitReceipt(
    command: BridgeCommandAdmission,
    outcome: BridgeCommandReceiptOutcome,
    detailDigest?: string,
  ): Promise<void>;
  close?(): void;
}

const PREDICATES = Object.freeze({
  executionId: "bridge.command/execution-id",
  attemptId: "bridge.command/attempt-id",
  ordinal: "bridge.command/ordinal",
  kind: "bridge.command/kind",
  payloadDigest: "bridge.command/payload-digest",
  payloadArtifact: "bridge.command/payload-artifact",
  delivery: "bridge.command/delivery",
  intent: "bridge.command/delivery-intent",
  receipt: "bridge.command/delivery-receipt",
  receiptDetailDigest: "bridge.command/receipt-detail-digest",
});

const EXECUTION_PREDICATES = Object.freeze({
  executionId: "bridge.execution/execution-id",
  attemptId: "bridge.execution/attempt-id",
});

const SHA256 = /^[0-9a-f]{64}$/;
const ACCOUNT_ID = /^[a-z0-9][a-z0-9_-]{0,63}$/;
const MAX_OCC_ATTEMPTS = 8;

function requireIdentity(label: string, value: string): string {
  if (value.length === 0 || value.trim() !== value) {
    throw new Error(`Bridge command ${label} must be a nonblank canonical string`);
  }
  return value;
}

function requireDigest(label: string, value: string): string {
  if (!SHA256.test(value)) throw new Error(`Bridge command ${label} must be lowercase SHA-256`);
  return value;
}

function requireAttemptId(value: string): string {
  if (!validExecutionAttemptIdentity(value)) {
    throw new Error("Bridge command attempt ID is not a canonical Store attempt identity");
  }
  return value;
}

function sha256(value: string): string {
  return createHash("sha256").update(value, "utf8").digest("hex");
}

export function bridgeCommandPayloadDigest(kind: BridgeCommandKind, payload = ""): string {
  return sha256(`north:bridge-command-payload:v1\0${kind}\0${payload}`);
}

export function bridgeCommandResultDigest(error: unknown): string {
  const detail = error instanceof Error ? error.message : String(error);
  return sha256(`north:bridge-command-result:v1\0${detail}`);
}

export function bridgeCommandArtifactLocator(executionId: string, controlSeq: number): string {
  requireIdentity("execution ID", executionId);
  if (!Number.isSafeInteger(controlSeq) || controlSeq < 1) {
    throw new Error("Bridge command control sequence must be a positive safe integer");
  }
  return `bridge-journal:${executionId}:events.log#record=${controlSeq}`;
}

function commandSubject(attemptId: string, ordinal: number): string {
  return `@bridge-command:${sha256(attemptId)}:${ordinal}`;
}

function executionSubject(executionId: string): string {
  return `@bridge-execution:${sha256(executionId)}`;
}

function ordinalFromSubject(subject: Term, attemptId: string): number | undefined {
  if (typeof subject !== "string") return undefined;
  const prefix = `@bridge-command:${sha256(attemptId)}:`;
  if (!subject.startsWith(prefix)) return undefined;
  const ordinal = Number(subject.slice(prefix.length));
  return Number.isSafeInteger(ordinal) && ordinal > 0 ? ordinal : undefined;
}

function action(subject: string, predicate: string, value: Term): BatchAction {
  return {
    op: "assert",
    proposition: triple(subject, predicate, value),
    policy: RPC_SUBJECT_ANY,
  };
}

function admissionActions(command: BridgeCommandAdmission): BatchAction[] {
  return [
    action(command.commandId, PREDICATES.executionId, command.executionId),
    action(command.commandId, PREDICATES.attemptId, command.attemptId),
    action(command.commandId, PREDICATES.ordinal, command.ordinal),
    action(command.commandId, PREDICATES.kind, command.kind),
    action(command.commandId, PREDICATES.payloadDigest, command.payloadDigest),
    action(command.commandId, PREDICATES.payloadArtifact, command.payloadArtifact),
    action(command.commandId, PREDICATES.delivery, command.delivery),
  ];
}

function rowsByPredicate(rows: readonly Term[]): Map<string, Term[]> {
  const result = new Map<string, Term[]>();
  for (const row of rows) {
    if (!(row instanceof StoreTriple) || typeof row.t2 !== "string") continue;
    const values = result.get(row.t2) ?? [];
    values.push(row.t3);
    result.set(row.t2, values);
  }
  return result;
}

function requireAttemptString(
  facts: ReadonlyMap<string, readonly Term[]>, predicate: string,
): string {
  const value = singleton(facts, predicate);
  if (typeof value !== "string" || value.length === 0 || value.trim() !== value) {
    throw new Error(`Bridge launch attempt lacks exact ${predicate}`);
  }
  return value;
}

function requireAttemptDigest(
  facts: ReadonlyMap<string, readonly Term[]>, predicate: string,
): string {
  const value = requireAttemptString(facts, predicate);
  if (!SHA256.test(value)) throw new Error(`Bridge launch attempt has malformed ${predicate}`);
  return value;
}

function requireInstant(value: string, label: string): void {
  if (!Number.isFinite(Date.parse(value))) throw new Error(`Bridge launch attempt has malformed ${label}`);
}

function requireLease(value: string, resource: string, label: string): void {
  let parsed: unknown;
  try { parsed = JSON.parse(value); }
  catch { throw new Error(`Bridge launch attempt has malformed ${label}`); }
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    throw new Error(`Bridge launch attempt has malformed ${label}`);
  }
  const lease = parsed as Record<string, unknown>;
  if (Object.keys(lease).sort().join("\0") !== ["epoch", "holder", "resource"].join("\0")
    || lease.resource !== resource
    || typeof lease.holder !== "string" || lease.holder.length === 0
    || !Number.isSafeInteger(lease.epoch) || (lease.epoch as number) < 1) {
    throw new Error(`Bridge launch attempt has malformed ${label}`);
  }
}

function assertRequestedRoute(
  authority: BridgeAttemptRouteAuthority,
  request: BridgeAttemptRouteRequest,
): void {
  if (request.provider !== undefined && request.provider !== authority.provider) {
    throw new Error("Bridge launch provider conflicts with its Store attempt authority");
  }
  if (request.model !== undefined && request.model !== authority.model) {
    throw new Error("Bridge launch model conflicts with its Store attempt authority");
  }
}

function singleton(
  facts: ReadonlyMap<string, readonly Term[]>, predicate: string,
): Term | undefined {
  const values = facts.get(predicate) ?? [];
  if (values.length > 1) throw new Error(`Bridge command has multiple ${predicate} facts`);
  return values[0];
}

function stringFact(
  facts: ReadonlyMap<string, readonly Term[]>, predicate: string,
): string {
  const value = singleton(facts, predicate);
  if (typeof value !== "string") throw new Error(`Bridge command lacks string ${predicate}`);
  return value;
}

function admissionFromRows(commandId: string, rows: readonly Term[]): BridgeCommandAdmission {
  const facts = rowsByPredicate(rows);
  const ordinal = singleton(facts, PREDICATES.ordinal);
  if (typeof ordinal !== "number" || !Number.isSafeInteger(ordinal) || ordinal < 1) {
    throw new Error("Bridge command lacks a positive ordinal");
  }
  const kind = stringFact(facts, PREDICATES.kind);
  if (!["submit-input", "interrupt-turn", "redirect-now", "terminate-session"].includes(kind)) {
    throw new Error("Bridge command kind is invalid");
  }
  const delivery = stringFact(facts, PREDICATES.delivery);
  if (!["queued-next-turn", "active-turn", "interrupt-and-redirect", "session-terminated"]
    .includes(delivery)) {
    throw new Error("Bridge command delivery is invalid");
  }
  return {
    commandId,
    executionId: stringFact(facts, PREDICATES.executionId),
    attemptId: stringFact(facts, PREDICATES.attemptId),
    ordinal,
    kind: kind as BridgeCommandKind,
    payloadDigest: requireDigest(
      "payload digest",
      stringFact(facts, PREDICATES.payloadDigest),
    ),
    payloadArtifact: stringFact(facts, PREDICATES.payloadArtifact),
    delivery: delivery as BridgeCommandDelivery,
  };
}

function exactAdmission(
  actual: BridgeCommandAdmission,
  expected: BridgeCommandAdmission,
): boolean {
  return Object.entries(expected).every(([key, value]) =>
    actual[key as keyof BridgeCommandAdmission] === value);
}

function expectedResults(actions: readonly BatchAction[], results: readonly { inputIndex: number }[]): boolean {
  return actions.length === results.length
    && results.every((result, index) => result.inputIndex === index);
}

function occConflict(error: unknown): boolean {
  return error instanceof StoreRpcServerError && error.code === "rpc/conflict";
}

function attemptRouteAuthority(
  attemptId: string,
  attemptRows: readonly Term[],
  accountRows: readonly Term[],
): BridgeAttemptWireAuthority {
  const facts = rowsByPredicate(attemptRows);
  const manifestSha256 = attemptId.slice("@attempt:".length);
  if (singleton(facts, "kind") !== "execution_attempt"
    || singleton(facts, "execution_attempt_version") !== "north:execution-attempt:v1"
    || singleton(facts, "execution_attempt_manifest_sha256") !== manifestSha256) {
    throw new Error("Bridge launch attempt is not canonically acknowledged by Store");
  }
  const provider = requireAttemptString(facts, "execution_attempt_provider");
  if (provider !== "anthropic" && provider !== "openai") {
    throw new Error("Bridge launch attempt has an unsupported provider");
  }
  const accountId = requireAttemptString(facts, "execution_attempt_account");
  if (!ACCOUNT_ID.test(accountId)) throw new Error("Bridge launch attempt has a malformed account");
  const model = requireAttemptString(facts, "execution_attempt_model");
  if (/\s/.test(model) || model.length > 256) {
    throw new Error("Bridge launch attempt has a malformed model");
  }
  const thread = requireAttemptString(facts, "execution_attempt_thread");
  const threadId = thread.startsWith("@thread:")
    ? thread.slice("@thread:".length)
    : thread.startsWith("@") ? thread.slice(1) : "";
  if (!threadId) {
    throw new Error("Bridge launch attempt has a malformed thread");
  }
  let authoritativeRunId: WireRunId;
  try { authoritativeRunId = wireRunId(requireAttemptString(facts, "execution_attempt_run")); }
  catch { throw new Error("Bridge launch attempt has a malformed run"); }
  const reporter = requireAttemptString(facts, "execution_attempt_reporter");
  if (!reporter.startsWith("@agent:") || reporter.length === "@agent:".length) {
    throw new Error("Bridge launch attempt has a malformed reporter");
  }
  let wireLedgerIdentity: WireRunLedgerIdentity;
  try {
    wireLedgerIdentity = wireRunLedgerIdentity({
      thread,
      agent: reporter.slice("@agent:".length),
    });
  } catch {
    throw new Error("Bridge launch attempt has a malformed wire ledger identity");
  }
  const ordinal = Number(requireAttemptString(facts, "execution_attempt_ordinal"));
  if (!Number.isSafeInteger(ordinal) || ordinal < 1) {
    throw new Error("Bridge launch attempt has a malformed ordinal");
  }
  requireAttemptDigest(facts, "execution_attempt_run_capability_sha256");
  requireAttemptDigest(facts, "execution_attempt_run_contract_sha256");
  const accountAuthorityReceiptSha256 = requireAttemptDigest(
    facts, "execution_attempt_account_authority_sha256",
  );
  const routeObservationReceiptSha256 = requireAttemptDigest(
    facts, "execution_attempt_route_observation_sha256",
  );
  requireLease(
    requireAttemptString(facts, "execution_attempt_thread_lease"),
    `thread:${threadId}:dispatch`,
    "thread lease",
  );
  requireLease(
    requireAttemptString(facts, "execution_attempt_account_lease"),
    `codex-account:${accountId}:slot:0`,
    "account lease",
  );
  requireInstant(
    requireAttemptString(facts, "execution_attempt_reserved_at"),
    "reservation time",
  );
  if (singleton(facts, "execution_attempt_launch_intent_version")
    !== "north:execution-attempt-launch-intent:v1") {
    throw new Error("Bridge launch attempt lacks its canonical launch intent");
  }
  requireInstant(
    requireAttemptString(facts, "execution_attempt_launch_intent_at"),
    "launch-intent time",
  );
  const launchIntentSha256 = requireAttemptDigest(
    facts, "execution_attempt_launch_intent_sha256",
  );
  let credentialProfile: string | undefined;
  if (provider === "openai") {
    const account = rowsByPredicate(accountRows);
    const profile = singleton(account, "provider_profile");
    if (singleton(account, "kind") !== "provider_account"
      || singleton(account, "account_id") !== accountId
      || singleton(account, "provider") !== "openai"
      || typeof profile !== "string" || !ACCOUNT_ID.test(profile)
      || singleton(account, "account_role") !== "execution"
      || singleton(account, "execution_eligible") !== "true") {
      throw new Error("Bridge launch attempt account lacks an execution Store role");
    }
    credentialProfile = profile;
  }
  return Object.freeze({
    attemptId,
    provider,
    accountId,
    ...(credentialProfile ? { credentialProfile } : {}),
    model,
    wireRunId: authoritativeRunId,
    wireLedgerIdentity,
    accountAuthorityReceiptSha256,
    routeObservationReceiptSha256,
    launchIntentSha256,
  });
}

export class StoreBridgeCommandReceipts implements BridgeCommandReceipts {
  readonly #client: StoreRpcClient;
  readonly #ownsClient: boolean;

  constructor(client?: StoreRpcClient) {
    this.#client = client ?? StoreRpcClient.create({ maxAttempts: 1 });
    this.#ownsClient = client === undefined;
  }

  close(): void {
    if (this.#ownsClient) this.#client.close();
  }

  async #commandRows(commandId: string): Promise<{ rows: Term[]; servedVersion: number }> {
    const snapshot = await this.#client.scanAll(commandId, null, null);
    return { rows: snapshot.rows, servedVersion: snapshot.servedVersion };
  }

  async #exactReadback(command: BridgeCommandAdmission): Promise<boolean> {
    try {
      const readback = await this.#commandRows(command.commandId);
      return readback.rows.length > 0
        && exactAdmission(admissionFromRows(command.commandId, readback.rows), command);
    } catch {
      return false;
    }
  }

  async #attemptRouteSnapshot(attemptId: string): Promise<{
    authority: BridgeAttemptWireAuthority;
    servedVersion: number;
  }> {
    for (let attempt = 0; attempt < MAX_OCC_ATTEMPTS; attempt += 1) {
      const snapshot = await this.#commandRows(attemptId);
      const facts = rowsByPredicate(snapshot.rows);
      const accountId = requireAttemptString(facts, "execution_attempt_account");
      if (!ACCOUNT_ID.test(accountId)) {
        throw new Error("Bridge launch attempt has a malformed account");
      }
      const account = await this.#commandRows(`@account:${accountId}`);
      if (account.servedVersion !== snapshot.servedVersion) continue;
      return {
        authority: attemptRouteAuthority(attemptId, snapshot.rows, account.rows),
        servedVersion: snapshot.servedVersion,
      };
    }
    throw new Error("Bridge launch attempt could not obtain one Store authority snapshot");
  }

  async bindExecution(
    executionId: string,
    attemptId: string,
    request: BridgeAttemptRouteRequest,
  ): Promise<BridgeAttemptWireAuthority> {
    requireIdentity("execution ID", executionId);
    requireAttemptId(attemptId);
    const subject = executionSubject(executionId);
    for (let attempt = 0; attempt < MAX_OCC_ATTEMPTS; attempt += 1) {
      const [attemptSnapshot, snapshot, bindingIndex] = await Promise.all([
        this.#attemptRouteSnapshot(attemptId),
        this.#commandRows(subject),
        this.#client.scanAll(null, EXECUTION_PREDICATES.attemptId, attemptId),
      ]);
      if (attemptSnapshot.servedVersion !== snapshot.servedVersion
        || bindingIndex.servedVersion !== snapshot.servedVersion) continue;
      const authority = attemptSnapshot.authority;
      assertRequestedRoute(authority, request);
      const bindingSubjects = bindingIndex.rows
        .map((row) => row instanceof StoreTriple ? row.t1 : undefined)
        .filter((value): value is string => typeof value === "string");
      if (bindingSubjects.some((bindingSubject) => bindingSubject !== subject)) {
        throw new Error("Bridge launch attempt is already bound to another execution");
      }
      const facts = rowsByPredicate(snapshot.rows);
      const presentExecution = singleton(facts, EXECUTION_PREDICATES.executionId);
      const presentAttempt = singleton(facts, EXECUTION_PREDICATES.attemptId);
      if (presentExecution !== undefined || presentAttempt !== undefined) {
        if (presentExecution !== executionId || presentAttempt !== attemptId) {
          throw new Error("Bridge execution has a conflicting Store attempt binding");
        }
        return authority;
      }
      const actions = [
        action(subject, EXECUTION_PREDICATES.executionId, executionId),
        action(subject, EXECUTION_PREDICATES.attemptId, attemptId),
      ];
      try {
        const committed = await this.#client.batch(actions, {
          expectedVersion: snapshot.servedVersion,
        });
        if (!expectedResults(actions, committed.results)) {
          throw new Error("Store returned an incomplete Bridge execution binding receipt");
        }
        return authority;
      } catch (error) {
        if (occConflict(error)) continue;
        if (error instanceof StoreRpcTransportError && error.requestSent) {
          const readback = rowsByPredicate((await this.#commandRows(subject)).rows);
          if (singleton(readback, EXECUTION_PREDICATES.executionId) === executionId
            && singleton(readback, EXECUTION_PREDICATES.attemptId) === attemptId) return authority;
        }
        throw error;
      }
    }
    throw new Error("Bridge execution binding exhausted its Store OCC retries");
  }

  async routeForExecution(executionId: string): Promise<BridgeAttemptWireAuthority> {
    requireIdentity("execution ID", executionId);
    for (let attempt = 0; attempt < MAX_OCC_ATTEMPTS; attempt += 1) {
      const binding = await this.#commandRows(executionSubject(executionId));
      const facts = rowsByPredicate(binding.rows);
      if (singleton(facts, EXECUTION_PREDICATES.executionId) !== executionId) {
        throw new Error("Bridge execution lacks its Store execution binding");
      }
      const attemptId = singleton(facts, EXECUTION_PREDICATES.attemptId);
      if (typeof attemptId !== "string" || !validExecutionAttemptIdentity(attemptId)) {
        throw new Error("Bridge execution lacks its Store attempt binding");
      }
      const acknowledged = await this.#attemptRouteSnapshot(attemptId);
      if (acknowledged.servedVersion !== binding.servedVersion) continue;
      return acknowledged.authority;
    }
    throw new Error("Bridge execution attempt lookup could not obtain one Store snapshot");
  }

  createWirePublisher(authority: BridgeAttemptWireAuthority): WireEventStorePublisher {
    return createWireEventStorePublisher(authority.wireLedgerIdentity);
  }

  async admit(request: BridgeCommandAdmissionRequest): Promise<BridgeCommandAdmission> {
    requireIdentity("execution ID", request.executionId);
    requireIdentity("attempt ID", request.attemptId);
    requireDigest("payload digest", request.payloadDigest);
    requireIdentity("payload artifact", request.payloadArtifact);
    for (let attempt = 0; attempt < MAX_OCC_ATTEMPTS; attempt += 1) {
      const snapshot = await this.#client.scanAll(null, PREDICATES.attemptId, request.attemptId);
      let lastOrdinal = 0;
      for (const row of snapshot.rows) {
        if (!(row instanceof StoreTriple)) continue;
        const ordinal = ordinalFromSubject(row.t1, request.attemptId);
        if (ordinal !== undefined) lastOrdinal = Math.max(lastOrdinal, ordinal);
      }
      const ordinal = lastOrdinal + 1;
      const command: BridgeCommandAdmission = {
        ...request,
        commandId: commandSubject(request.attemptId, ordinal),
        ordinal,
      };
      const actions = admissionActions(command);
      try {
        const committed = await this.#client.batch(actions, {
          expectedVersion: snapshot.servedVersion,
        });
        if (!expectedResults(actions, committed.results)) {
          throw new Error("Store returned an incomplete Bridge command admission receipt");
        }
        return command;
      } catch (error) {
        if (occConflict(error)) continue;
        if (error instanceof StoreRpcTransportError && error.requestSent
          && await this.#exactReadback(command)) return command;
        throw error;
      }
    }
    throw new Error("Bridge command admission exhausted its Store OCC retries");
  }

  async reconcile(attemptId: string): Promise<BridgeCommandRecovery> {
    requireIdentity("attempt ID", attemptId);
    for (let readAttempt = 0; readAttempt < MAX_OCC_ATTEMPTS; readAttempt += 1) {
      const index = await this.#client.scanAll(null, PREDICATES.attemptId, attemptId);
      const subjects = index.rows
        .map((row) => row instanceof StoreTriple ? row.t1 : undefined)
        .filter((value): value is string => typeof value === "string")
        .filter((value, position, all) => all.indexOf(value) === position);
      const snapshots = await Promise.all(subjects.map(async (subject) => ({
        subject,
        snapshot: await this.#commandRows(subject),
      })));
      if (snapshots.some(({ snapshot }) => snapshot.servedVersion !== index.servedVersion)) continue;
      const pending: BridgeCommandAdmission[] = [];
      const unresolvedIntents: BridgeCommandAdmission[] = [];
      for (const { subject, snapshot } of snapshots) {
        const command = admissionFromRows(subject, snapshot.rows);
        const facts = rowsByPredicate(snapshot.rows);
        const intent = singleton(facts, PREDICATES.intent);
        const receipt = singleton(facts, PREDICATES.receipt);
        if (receipt !== undefined && intent === undefined) {
          throw new Error("Bridge command has a delivery receipt without an intent");
        }
        if (intent === undefined) pending.push(command);
        else if (receipt === undefined) unresolvedIntents.push(command);
      }
      const byOrdinal = (left: BridgeCommandAdmission, right: BridgeCommandAdmission) =>
        left.ordinal - right.ordinal;
      return {
        pending: pending.sort(byOrdinal),
        unresolvedIntents: unresolvedIntents.sort(byOrdinal),
      };
    }
    throw new Error("Bridge command reconciliation could not obtain one Store snapshot");
  }

  async #commitSingleton(
    command: BridgeCommandAdmission,
    predicate: string,
    value: string,
    companion?: { predicate: string; value: string },
  ): Promise<void> {
    for (let attempt = 0; attempt < MAX_OCC_ATTEMPTS; attempt += 1) {
      const snapshot = await this.#commandRows(command.commandId);
      const actual = admissionFromRows(command.commandId, snapshot.rows);
      if (!exactAdmission(actual, command)) {
        throw new Error("Bridge command receipt does not match its Store admission");
      }
      const facts = rowsByPredicate(snapshot.rows);
      if (predicate === PREDICATES.receipt
        && singleton(facts, PREDICATES.intent) !== command.delivery) {
        throw new Error("Bridge command cannot receive a delivery receipt before its intent");
      }
      const present = singleton(facts, predicate);
      if (present !== undefined) {
        if (present !== value) throw new Error(`Bridge command has conflicting ${predicate}`);
        if (companion !== undefined && singleton(facts, companion.predicate) !== companion.value) {
          throw new Error(`Bridge command has conflicting ${companion.predicate}`);
        }
        return;
      }
      const actions = [
        action(command.commandId, predicate, value),
        ...(companion === undefined
          ? []
          : [action(command.commandId, companion.predicate, companion.value)]),
      ];
      try {
        const committed = await this.#client.batch(actions, {
          expectedVersion: snapshot.servedVersion,
        });
        if (!expectedResults(actions, committed.results)) {
          throw new Error("Store returned an incomplete Bridge command effect receipt");
        }
        return;
      } catch (error) {
        if (occConflict(error)) continue;
        if (error instanceof StoreRpcTransportError && error.requestSent) {
          const readback = rowsByPredicate((await this.#commandRows(command.commandId)).rows);
          if (singleton(readback, predicate) === value
            && (companion === undefined
              || singleton(readback, companion.predicate) === companion.value)) return;
        }
        throw error;
      }
    }
    throw new Error("Bridge command receipt exhausted its Store OCC retries");
  }

  commitIntent(command: BridgeCommandAdmission): Promise<void> {
    return this.#commitSingleton(command, PREDICATES.intent, command.delivery);
  }

  commitReceipt(
    command: BridgeCommandAdmission,
    outcome: BridgeCommandReceiptOutcome,
    detailDigest?: string,
  ): Promise<void> {
    if (detailDigest !== undefined) requireDigest("receipt detail digest", detailDigest);
    return this.#commitSingleton(
      command,
      PREDICATES.receipt,
      outcome,
      detailDigest === undefined
        ? undefined
        : { predicate: PREDICATES.receiptDetailDigest, value: detailDigest },
    );
  }
}

export class MemoryBridgeCommandReceipts implements BridgeCommandReceipts {
  readonly commands: BridgeCommandAdmission[] = [];
  readonly intents: string[] = [];
  readonly receipts: Array<{ commandId: string; outcome: BridgeCommandReceiptOutcome }> = [];
  readonly executionAttempts = new Map<string, string>();
  readonly acknowledgedAttempts: Set<string>;
  readonly attemptAuthorities = new Map<string, BridgeAttemptRouteAuthority>();
  readonly #wireWriter: WireLedgerBatchWriter;

  constructor(
    attempts: readonly (string | BridgeAttemptRouteAuthority)[] = [],
    options: { wireWriter?: WireLedgerBatchWriter } = {},
  ) {
    this.acknowledgedAttempts = new Set(attempts.map((attempt) =>
      typeof attempt === "string" ? attempt : attempt.attemptId));
    for (const attempt of attempts) {
      if (typeof attempt !== "string") this.attemptAuthorities.set(attempt.attemptId, attempt);
    }
    this.#wireWriter = options.wireWriter ?? (async () => "recorded");
  }

  #wireAuthority(
    executionId: string,
    authority: BridgeAttemptRouteAuthority,
  ): BridgeAttemptWireAuthority {
    const extended = authority as Partial<BridgeAttemptWireAuthority>;
    return Object.freeze({
      ...authority,
      wireRunId: extended.wireRunId ?? wireRunId(`bridge:${executionId}`),
      wireLedgerIdentity: extended.wireLedgerIdentity ?? wireRunLedgerIdentity({
        thread: "(ad-hoc)",
        agent: `bridge:${executionId}`,
      }),
    });
  }

  async bindExecution(
    executionId: string,
    attemptId: string,
    request: BridgeAttemptRouteRequest,
  ): Promise<BridgeAttemptWireAuthority> {
    if (!this.acknowledgedAttempts.has(attemptId)) {
      throw new Error("Bridge launch attempt is not acknowledged by Store");
    }
    for (const [boundExecutionId, boundAttemptId] of this.executionAttempts) {
      if (boundAttemptId === attemptId && boundExecutionId !== executionId) {
        throw new Error("Bridge launch attempt is already bound to another execution");
      }
    }
    const present = this.executionAttempts.get(executionId);
    if (present !== undefined && present !== attemptId) {
      throw new Error("Bridge execution already has a different attempt binding");
    }
    const provider = request.provider ?? "openai";
    const authority = this.attemptAuthorities.get(attemptId) ?? Object.freeze({
      attemptId,
      provider,
      accountId: provider,
      ...(provider === "openai" ? { credentialProfile: provider } : {}),
      model: request.model ?? (provider === "openai" ? "gpt-5.6-terra" : "claude-sonnet-4-6"),
      accountAuthorityReceiptSha256: sha256(`memory-account-authority\0${attemptId}`),
      routeObservationReceiptSha256: sha256(`memory-route-observation\0${attemptId}`),
      launchIntentSha256: sha256(`memory-launch-intent\0${attemptId}`),
    });
    assertRequestedRoute(authority, request);
    this.executionAttempts.set(executionId, attemptId);
    return this.#wireAuthority(executionId, authority);
  }

  async routeForExecution(executionId: string): Promise<BridgeAttemptWireAuthority> {
    const attemptId = this.executionAttempts.get(executionId);
    if (attemptId === undefined) throw new Error("Bridge execution lacks an attempt binding");
    const authority = this.attemptAuthorities.get(attemptId) ?? {
      attemptId,
      provider: "openai" as const,
      accountId: "openai",
      credentialProfile: "openai",
      model: "gpt-5.6-terra",
      accountAuthorityReceiptSha256: sha256(`memory-account-authority\0${attemptId}`),
      routeObservationReceiptSha256: sha256(`memory-route-observation\0${attemptId}`),
      launchIntentSha256: sha256(`memory-launch-intent\0${attemptId}`),
    };
    return this.#wireAuthority(executionId, authority);
  }

  createWirePublisher(authority: BridgeAttemptWireAuthority): WireEventStorePublisher {
    return createWireEventStorePublisher(authority.wireLedgerIdentity, {
      writer: this.#wireWriter,
    });
  }

  async admit(request: BridgeCommandAdmissionRequest): Promise<BridgeCommandAdmission> {
    const command = {
      ...request,
      commandId: `memory-bridge-command:${randomUUID()}`,
      ordinal: this.commands.length + 1,
    };
    this.commands.push(command);
    return command;
  }

  async reconcile(attemptId: string): Promise<BridgeCommandRecovery> {
    const receiptIds = new Set(this.receipts.map(({ commandId }) => commandId));
    const intentIds = new Set(this.intents);
    const commands = this.commands.filter((command) => command.attemptId === attemptId);
    return {
      pending: commands.filter((command) => !intentIds.has(command.commandId)),
      unresolvedIntents: commands.filter((command) =>
        intentIds.has(command.commandId) && !receiptIds.has(command.commandId)),
    };
  }

  async commitIntent(command: BridgeCommandAdmission): Promise<void> {
    if (!this.intents.includes(command.commandId)) this.intents.push(command.commandId);
  }

  async commitReceipt(
    command: BridgeCommandAdmission,
    outcome: BridgeCommandReceiptOutcome,
  ): Promise<void> {
    const existing = this.receipts.find(({ commandId }) => commandId === command.commandId);
    if (existing && existing.outcome !== outcome) {
      throw new Error("Bridge command already has a different receipt");
    }
    if (!existing) this.receipts.push({ commandId: command.commandId, outcome });
  }
}
