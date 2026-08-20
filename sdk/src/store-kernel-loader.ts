/** Store-backed reconstruction of the pure Store kernel snapshot. */
import { createHash } from "node:crypto";
import { StoreTriple, type Term } from "./store-rpc-codec";
import { decodeStoreSnapshot, type StoreBridgeCommand, type StoreFact, type StoreSnapshot, type StoreWireEvent } from "./store-kernel";
import { type StoreRpcClient } from "./store-rpc-client";

export interface LoadStoreSnapshotOptions {
  /** Account id without the `@account:` Store subject prefix. */
  readonly accountId?: string;
  /** Canonical `@attempt:<manifest-sha256>` Store subject. */
  readonly attemptId?: string;
  /** Exact immutable execution-attempt thread fact. */
  readonly threadId?: string;
  /** Exact immutable execution-attempt run fact. */
  readonly runId?: string;
  /** Injection seam; callers normally pass their already-attested client. */
  readonly client: Pick<StoreRpcClient, "scanAll">;
}

/** A pure kernel snapshot and the one Store version from which it was reconstructed. */
export interface LoadedStoreSnapshot {
  readonly servedVersion: number;
  readonly snapshot: StoreSnapshot;
}

type Rows = ReadonlyMap<string, ReadonlyMap<string, readonly Term[]>>;

const SHA256 = /^[0-9a-f]{64}$/;
const ACCOUNT_ID = /^[a-z0-9][a-z0-9_-]{0,63}$/;
const ATTEMPT = /^@attempt:([0-9a-f]{64})$/;
const COMMAND_KINDS = new Set(["submit-input", "interrupt-turn", "redirect-now", "terminate-session"]);
const DELIVERIES = new Set(["queued-next-turn", "active-turn", "interrupt-and-redirect", "session-terminated"]);
const CANCEL_KINDS = new Set(["interrupt-turn", "redirect-now", "terminate-session"]);

function fail(reason: string): never { throw new Error(`Store snapshot reconstruction refused: ${reason}`); }

function rowsBySubject(rows: readonly Term[]): Rows {
  const result = new Map<string, Map<string, Term[]>>();
  for (const row of rows) {
    if (!(row instanceof StoreTriple) || typeof row.t1 !== "string" || typeof row.t2 !== "string") continue;
    const subject = result.get(row.t1) ?? new Map<string, Term[]>();
    const values = subject.get(row.t2) ?? [];
    values.push(row.t3);
    subject.set(row.t2, values);
    result.set(row.t1, subject);
  }
  return result;
}

function exact(facts: ReadonlyMap<string, readonly Term[]>, predicate: string): Term {
  const values = facts.get(predicate) ?? [];
  if (values.length !== 1) fail(`${predicate} is absent or conflicting`);
  return values[0]!;
}

function text(facts: ReadonlyMap<string, readonly Term[]>, predicate: string): string {
  const value = exact(facts, predicate);
  if (typeof value !== "string" || value.length === 0) fail(`${predicate} is not nonblank text`);
  return value;
}

function digest(facts: ReadonlyMap<string, readonly Term[]>, predicate: string): string {
  const value = text(facts, predicate);
  if (!SHA256.test(value)) fail(`${predicate} is not SHA-256`);
  return value;
}

function positive(facts: ReadonlyMap<string, readonly Term[]>, predicate: string): number {
  const value = exact(facts, predicate);
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 1)
    fail(`${predicate} is not a positive safe integer`);
  return value;
}

function fence(facts: ReadonlyMap<string, readonly Term[]>, predicate: string, resource: string): void {
  const raw = text(facts, predicate);
  let value: unknown;
  try { value = JSON.parse(raw); } catch { fail(`${predicate} is not a Store lease fence`); }
  if (!value || typeof value !== "object" || Array.isArray(value)) fail(`${predicate} is not a Store lease fence`);
  const candidate = value as Record<string, unknown>;
  if (Object.keys(candidate).sort().join("\0") !== "epoch\0holder\0resource"
    || candidate.resource !== resource
    || typeof candidate.holder !== "string" || candidate.holder.length === 0
    || !Number.isSafeInteger(candidate.epoch) || (candidate.epoch as number) < 1)
    fail(`${predicate} is not a Store lease fence`);
}

function commandDigest(attemptId: string): string {
  return createHash("sha256").update(attemptId, "utf8").digest("hex");
}

interface Attempt {
  readonly subject: string;
  readonly manifest: string;
  readonly run: string;
  readonly thread: string;
  readonly accountSubject: string;
  readonly facts: ReadonlyMap<string, readonly Term[]>;
}

function attemptFromRows(subject: string, facts: ReadonlyMap<string, readonly Term[]>): Attempt | undefined {
  const match = ATTEMPT.exec(subject);
  if (!match) return undefined;
  try {
    if (text(facts, "kind") !== "execution_attempt" || digest(facts, "execution_attempt_manifest_sha256") !== match[1]
      || text(facts, "execution_attempt_version") !== "north:execution-attempt:v1") return undefined;
    const run = text(facts, "execution_attempt_run");
    const thread = text(facts, "execution_attempt_thread");
    const accountId = text(facts, "execution_attempt_account");
    if (!ACCOUNT_ID.test(accountId)) return undefined;
    const accountSubject = `@account:${accountId}`;
    text(facts, "execution_attempt_provider");
    text(facts, "execution_attempt_model");
    digest(facts, "execution_attempt_account_authority_sha256");
    digest(facts, "execution_attempt_route_observation_sha256");
    digest(facts, "execution_attempt_run_capability_sha256");
    digest(facts, "execution_attempt_run_contract_sha256");
    if (positiveText(facts, "execution_attempt_ordinal") !== 1) return undefined;
    text(facts, "execution_attempt_reporter");
    text(facts, "execution_attempt_reserved_at");
    fence(facts, "execution_attempt_thread_lease", `thread:${thread.slice("@thread:".length)}:dispatch`);
    fence(facts, "execution_attempt_account_lease", `codex-account:${accountId}:slot:0`);
    return { subject, manifest: match[1]!, run, thread, accountSubject, facts };
  } catch { return undefined; }
}

function positiveText(facts: ReadonlyMap<string, readonly Term[]>, predicate: string): number {
  const value = text(facts, predicate);
  if (!/^[1-9][0-9]*$/.test(value)) fail(`${predicate} is not a positive integer`);
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed)) fail(`${predicate} is not a positive integer`);
  return parsed;
}

function any(facts: ReadonlyMap<string, readonly Term[]>, predicates: readonly string[]): boolean {
  return predicates.some((predicate) => facts.has(predicate));
}

function lifecycle(attempt: Attempt, account: StoreSnapshot["account"]): StoreFact[] {
  const common = { account, attempt: {
    subject: attempt.subject,
    execution_attempt_id: attempt.subject,
    execution_attempt_manifest_sha256: attempt.manifest,
    execution_attempt_account_id: account.id,
    execution_attempt_account_role: account.account_role,
  } } as const;
  const make = (kind: StoreFact["kind"], ordinal: number, predicate: string): StoreFact => ({
    ...common, kind, ordinal, provenance: `${attempt.subject}#${predicate}`,
  });
  const facts = [make("reserved", 1, "execution_attempt_manifest_sha256")];
  const launch = ["execution_attempt_launch_intent_version", "execution_attempt_launch_intent_at", "execution_attempt_launch_intent_sha256"];
  const started = ["execution_attempt_provider_start_receipt_sha256", "execution_attempt_provider_started_at", "execution_attempt_provider_start_manifest_sha256"];
  if (any(attempt.facts, launch)) {
    if (text(attempt.facts, "execution_attempt_launch_intent_version") !== "north:execution-attempt-launch-intent:v1")
      fail("launch intent version is invalid");
    text(attempt.facts, "execution_attempt_launch_intent_at");
    digest(attempt.facts, "execution_attempt_launch_intent_sha256");
    facts.push(make("launch-intent", 2, "execution_attempt_launch_intent_sha256"));
  }
  if (any(attempt.facts, started)) {
    digest(attempt.facts, "execution_attempt_provider_start_receipt_sha256");
    text(attempt.facts, "execution_attempt_provider_started_at");
    digest(attempt.facts, "execution_attempt_provider_start_manifest_sha256");
    facts.push(make("started", 3, "execution_attempt_provider_start_manifest_sha256"));
  }
  return facts;
}

interface Command { readonly command: StoreBridgeCommand; readonly facts: ReadonlyMap<string, readonly Term[]>; readonly cancel: boolean; }

function commandsFor(attempt: Attempt, rows: Rows): Command[] {
  const expectedDigest = commandDigest(attempt.subject);
  const commands: Command[] = [];
  for (const [subject, facts] of rows) {
    const match = new RegExp(`^@bridge-command:${expectedDigest}:([1-9][0-9]*)$`).exec(subject);
    if (!match) continue;
    const ordinal = positive(facts, "bridge.command/ordinal");
    if (String(ordinal) !== match[1] || text(facts, "bridge.command/attempt-id") !== attempt.subject)
      fail("Bridge command subject and admission disagree");
    const kind = text(facts, "bridge.command/kind");
    const delivery = text(facts, "bridge.command/delivery");
    if (!COMMAND_KINDS.has(kind) || !DELIVERIES.has(delivery)) fail("Bridge command admission is invalid");
    digest(facts, "bridge.command/payload-digest");
    text(facts, "bridge.command/payload-artifact");
    text(facts, "bridge.command/execution-id");
    if (facts.has("bridge.command/delivery-intent") && text(facts, "bridge.command/delivery-intent") !== delivery)
      fail("Bridge command delivery intent conflicts with admission");
    if (facts.has("bridge.command/delivery-receipt")) {
      if (!facts.has("bridge.command/delivery-intent")) fail("Bridge command receipt lacks intent");
      const receipt = text(facts, "bridge.command/delivery-receipt");
      if (receipt !== "succeeded" && receipt !== "failed") fail("Bridge command receipt is invalid");
      if (facts.has("bridge.command/receipt-detail-digest")) digest(facts, "bridge.command/receipt-detail-digest");
    }
    commands.push({ command: { subject, bridge_command_ordinal: ordinal, bridge_command_attempt_id_sha256: expectedDigest }, facts, cancel: CANCEL_KINDS.has(kind) });
  }
  commands.sort((left, right) => left.command.bridge_command_ordinal - right.command.bridge_command_ordinal);
  if (commands.some((command, index) => index > 0 && command.command.bridge_command_ordinal === commands[index - 1]!.command.bridge_command_ordinal))
    fail("Bridge command ordinals conflict");
  if (commands.filter(({ cancel }) => cancel).length > 1) fail("multiple cancellation commands require reconciliation");
  return commands;
}

function commandFacts(commands: readonly Command[], account: StoreSnapshot["account"], attempt: Attempt): StoreFact[] {
  const storeAttempt = {
    subject: attempt.subject, execution_attempt_id: attempt.subject,
    execution_attempt_manifest_sha256: attempt.manifest, execution_attempt_account_id: account.id,
    execution_attempt_account_role: account.account_role,
  } as const;
  const result: StoreFact[] = [];
  for (let index = 0; index < commands.length; index += 1) {
    const current = commands[index]!;
    const ordinal = 10 + index * 3;
    const fact = (kind: StoreFact["kind"], offset: number, predicate: string): StoreFact => ({
      kind, ordinal: ordinal + offset, account, attempt: storeAttempt, command: current.command,
      provenance: `${current.command.subject}#${predicate}`,
    });
    if (current.cancel) {
      result.push(fact("cancel-request", 0, "bridge.command/admission"));
      if (current.facts.has("bridge.command/delivery-intent")) result.push(fact("cancel-intent", 1, "bridge.command/delivery-intent"));
      if (current.facts.has("bridge.command/delivery-receipt")) result.push(fact("cancel-receipt", 2, "bridge.command/delivery-receipt"));
    } else {
      result.push(fact("command-intent", 0, "bridge.command/admission"));
      if (current.facts.has("bridge.command/delivery-intent")) result.push(fact("delivery-intent", 1, "bridge.command/delivery-intent"));
      if (current.facts.has("bridge.command/delivery-receipt")) result.push(fact("delivery-receipt", 2, "bridge.command/delivery-receipt"));
    }
  }
  return result;
}

function wireEvents(attempt: Attempt, rows: Rows): StoreWireEvent[] {
  const events: Array<{ sequence: number; digest: string }> = [];
  for (const [subject, facts] of rows) {
    if (textOrUndefined(facts, "kind") !== "wire_event" || textOrUndefined(facts, "wire_run_id") !== attempt.run) continue;
    const raw = text(facts, "wire_event_sequence");
    if (!/^(0|[1-9][0-9]*)$/.test(raw)) fail(`wire event ${subject} has invalid sequence`);
    const sequence = Number(raw);
    if (!Number.isSafeInteger(sequence)) fail(`wire event ${subject} has invalid sequence`);
    text(facts, "wire_event_json");
    const eventDigest = digest(facts, "wire_event_sha256");
    events.push({ sequence, digest: eventDigest });
  }
  events.sort((left, right) => left.sequence - right.sequence);
  if (events.some((event, index) => index > 0 && event.sequence === events[index - 1]!.sequence)) fail("wire event sequences conflict");
  if (events.some((event, index) => event.sequence !== index)) fail("wire event sequence is incomplete");
  return events.map((event, index) => ({
    wire_event_sequence: index + 1, wire_event_sha256: event.digest,
    wire_event_predecessor_sha256: index === 0 ? null : events[index - 1]!.digest,
  }));
}

function textOrUndefined(facts: ReadonlyMap<string, readonly Term[]>, predicate: string): string | undefined {
  const values = facts.get(predicate) ?? [];
  return values.length === 1 && typeof values[0] === "string" && values[0].length > 0 ? values[0] : undefined;
}

/**
 * Drain one global Store scan, then reconstruct all action-relevant facts from
 * that one served version.  A per-subject scan would not prove a restart view.
 */
export async function loadStoreSnapshot(options: LoadStoreSnapshotOptions): Promise<LoadedStoreSnapshot> {
  if (!options || !options.client) fail("a Store RPC client is required");
  if (!options.accountId && !options.attemptId && !options.threadId && !options.runId)
    fail("an account, attempt, thread, or run selector is required");
  if (options.accountId !== undefined && !ACCOUNT_ID.test(options.accountId)) fail("account selector is invalid");
  if (options.attemptId !== undefined && !ATTEMPT.test(options.attemptId)) fail("attempt selector is invalid");
  const served = await options.client.scanAll(null, null, null);
  const rows = rowsBySubject(served.rows);
  const rawMatches = [...rows].filter(([subject, facts]) => ATTEMPT.test(subject)
    && (options.attemptId === undefined || subject === options.attemptId)
    && (options.threadId === undefined || textOrUndefined(facts, "execution_attempt_thread") === options.threadId)
    && (options.runId === undefined || textOrUndefined(facts, "execution_attempt_run") === options.runId)
    && (options.accountId === undefined || textOrUndefined(facts, "execution_attempt_account") === options.accountId));
  const attempts = [...rows].flatMap(([subject, facts]) => {
    const attempt = attemptFromRows(subject, facts);
    return attempt === undefined ? [] : [attempt];
  }).filter((attempt) => (options.attemptId === undefined || attempt.subject === options.attemptId)
    && (options.threadId === undefined || attempt.thread === options.threadId)
    && (options.runId === undefined || attempt.run === options.runId)
    && (options.accountId === undefined || attempt.accountSubject === `@account:${options.accountId}`));
  if (rawMatches.length !== attempts.length) fail("selected attempt reservation is incomplete or conflicting");
  if (attempts.length > 1) fail("selection identifies multiple attempts");
  const attempt = attempts[0];
  const accountSubject = attempt?.accountSubject ?? `@account:${options.accountId}`;
  if (!accountSubject || !accountSubject.startsWith("@account:")) fail("account authority is unavailable");
  const accountFacts = rows.get(accountSubject);
  if (!accountFacts) fail("account authority is unavailable");
  const id = text(accountFacts, "account_id");
  if (!ACCOUNT_ID.test(id) || accountSubject !== `@account:${id}` || text(accountFacts, "kind") !== "provider_account")
    fail("account authority identity is invalid");
  if (text(accountFacts, "provider") !== "openai") fail("account authority provider is invalid");
  const role = text(accountFacts, "account_role");
  const eligible = text(accountFacts, "execution_eligible");
  if ((role !== "execution" && role !== "oversight") || (eligible !== "true" && eligible !== "false"))
    fail("account authority role or eligibility is invalid");
  if (options.accountId !== undefined && id !== options.accountId) fail("account selector conflicts with authority");
  if (role === "execution" && eligible !== "true") fail("execution account is ineligible");
  const account = { id, account_role: role } as const;
  if (!attempt) return loaded(served.servedVersion, { account, facts: [], wireEvents: [] });
  if (attempt.accountSubject !== accountSubject) fail("attempt account conflicts with authority");
  const facts = lifecycle(attempt, account);
  const commands = commandsFor(attempt, rows);
  facts.push(...commandFacts(commands, account, attempt));
  const terminal = ["execution_attempt_terminal_receipt_sha256", "execution_attempt_terminal_at", "execution_attempt_terminal_manifest_sha256"];
  const unsent = ["execution_attempt_unsent_receipt_sha256", "execution_attempt_unsent_at", "execution_attempt_unsent_manifest_sha256"];
  const terminalOrdinal = 10 + commands.length * 3;
  if (any(attempt.facts, terminal)) {
    digest(attempt.facts, "execution_attempt_terminal_receipt_sha256");
    text(attempt.facts, "execution_attempt_terminal_at");
    digest(attempt.facts, "execution_attempt_terminal_manifest_sha256");
    facts.push({ ...facts[0]!, kind: "terminal", ordinal: terminalOrdinal,
      provenance: `${attempt.subject}#execution_attempt_terminal_manifest_sha256` });
  }
  if (any(attempt.facts, unsent)) {
    digest(attempt.facts, "execution_attempt_unsent_receipt_sha256");
    text(attempt.facts, "execution_attempt_unsent_at");
    digest(attempt.facts, "execution_attempt_unsent_manifest_sha256");
    facts.push({ ...facts[0]!, kind: "proved-unsent", ordinal: terminalOrdinal + 1,
      provenance: `${attempt.subject}#execution_attempt_unsent_manifest_sha256` });
  }
  if (any(attempt.facts, terminal) && any(attempt.facts, unsent))
    fail("attempt contains conflicting terminal receipts");
  return loaded(served.servedVersion, { account, facts, wireEvents: wireEvents(attempt, rows) });
}

function loaded(servedVersion: number, candidate: StoreSnapshot): LoadedStoreSnapshot {
  const decoded = decodeStoreSnapshot(candidate);
  if (!decoded.ok) fail(`kernel snapshot is invalid (${decoded.reason})`);
  return { servedVersion, snapshot: decoded.snapshot };
}
