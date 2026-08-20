/** Pure, fail-closed decisions over Store's typed snapshot adapter boundary. */
export type StoreRole = "execution" | "oversight";

export interface StoreAccount {
  readonly id: string;
  readonly account_role: StoreRole;
}

/** Immutable execution-attempt facts, addressed by their manifest digest. */
export interface StoreAttempt {
  readonly subject: string;
  readonly execution_attempt_id: string;
  readonly execution_attempt_manifest_sha256: string;
  readonly execution_attempt_account_id: string;
  readonly execution_attempt_account_role: StoreRole;
}

/** A bridge command is addressed by the attested SHA-256 of its attempt id. */
export interface StoreBridgeCommand {
  readonly subject: string;
  readonly bridge_command_ordinal: number;
  readonly bridge_command_attempt_id_sha256: string;
}

/**
 * Wire replay is an authority separate from lifecycle and bridge-command facts.
 * Its sequence and digest predecessor form the only cursor safeNext may expose.
 */
export interface StoreWireEvent {
  readonly wire_event_sequence: number;
  readonly wire_event_sha256: string;
  readonly wire_event_predecessor_sha256: string | null;
}

export type StoreFactKind =
  | "reserved"
  | "launch-intent"
  | "started"
  | "command-intent"
  | "delivery-intent"
  | "delivery-receipt"
  | "cancel-request"
  | "cancel-intent"
  | "cancel-receipt"
  | "terminal"
  | "proved-unsent"
  | "advanced";

/** An immutable lifecycle or bridge-command Store fact; ordinal is not replay. */
export interface StoreFact {
  readonly kind: StoreFactKind;
  readonly ordinal: number;
  readonly account: StoreAccount;
  readonly provenance: string;
  readonly attempt: StoreAttempt;
  readonly command?: StoreBridgeCommand;
  readonly terminal?: "succeeded" | "failed" | "cancelled";
}

export interface StoreSnapshot {
  readonly account: StoreAccount;
  readonly facts: readonly StoreFact[];
  readonly wireEvents: readonly StoreWireEvent[];
}

export type StoreInvalidReason =
  | "invalid-snapshot"
  | "missing-role-or-provenance"
  | "sequence-gap"
  | "digest-conflict"
  | "account-conflict"
  | "attempt-conflict"
  | "illegal-combination";

export type StoreSnapshotDecode =
  | Readonly<{ ok: true; snapshot: StoreSnapshot }>
  | Readonly<{ ok: false; reason: StoreInvalidReason }>;

export type StoreAction =
  | Readonly<{ kind: "reserve"; replayPosition: number }>
  | Readonly<{ kind: "launch"; attempt: StoreAttempt; replayPosition: number }>
  | Readonly<{ kind: "send"; attempt: StoreAttempt; command: StoreBridgeCommand; replayPosition: number }>
  | Readonly<{ kind: "cancel"; attempt: StoreAttempt; replayPosition: number }>
  | Readonly<{ kind: "reconcile-launch"; attempt: StoreAttempt; replayPosition: number }>
  | Readonly<{ kind: "reconcile-command"; attempt: StoreAttempt; command: StoreBridgeCommand; replayPosition: number }>
  | Readonly<{ kind: "advance"; attempt: StoreAttempt; replayPosition: number }>
  | Readonly<{ kind: "no-op"; reason: "oversight-account" | "awaiting-terminal" | "settled" | "nothing-pending"; replayPosition: number }>
  | Readonly<{ kind: "invalid"; reason: StoreInvalidReason; replayPosition: 0 }>;

interface Analysis {
  readonly reason?: StoreInvalidReason;
  readonly replayPosition: number;
  readonly facts: readonly StoreFact[];
}

const MAX_WIRE_EVENTS = 4_096;
const FACT_KINDS: ReadonlySet<string> = new Set<StoreFactKind>([
  "reserved", "launch-intent", "started", "command-intent", "delivery-intent",
  "delivery-receipt", "cancel-request", "cancel-intent", "cancel-receipt", "terminal", "proved-unsent", "advanced",
]);
const SHA256 = /^[a-f0-9]{64}$/;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isText(value: unknown): value is string {
  return typeof value === "string" && value.length > 0;
}

function isDigest(value: unknown): value is string {
  return typeof value === "string" && SHA256.test(value);
}

function decodeAccount(value: unknown): StoreAccount | undefined {
  if (!isRecord(value) || !isText(value.id)
    || (value.account_role !== "execution" && value.account_role !== "oversight")) return undefined;
  return { id: value.id, account_role: value.account_role };
}

function decodeAttempt(value: unknown): StoreAttempt | undefined {
  if (!isRecord(value) || !isText(value.execution_attempt_id)
    || !isDigest(value.execution_attempt_manifest_sha256)
    || !isText(value.execution_attempt_account_id)
    || (value.execution_attempt_account_role !== "execution" && value.execution_attempt_account_role !== "oversight")
    || value.subject !== `@attempt:${value.execution_attempt_manifest_sha256}`) return undefined;
  return {
    subject: value.subject,
    execution_attempt_id: value.execution_attempt_id,
    execution_attempt_manifest_sha256: value.execution_attempt_manifest_sha256,
    execution_attempt_account_id: value.execution_attempt_account_id,
    execution_attempt_account_role: value.execution_attempt_account_role,
  };
}

function decodeCommand(value: unknown): StoreBridgeCommand | undefined {
  if (!isRecord(value) || !Number.isSafeInteger(value.bridge_command_ordinal)
    || (value.bridge_command_ordinal as number) < 1 || !isDigest(value.bridge_command_attempt_id_sha256)
    || value.subject !== `@bridge-command:${value.bridge_command_attempt_id_sha256}:${value.bridge_command_ordinal}`) return undefined;
  return {
    subject: value.subject,
    bridge_command_ordinal: value.bridge_command_ordinal as number,
    bridge_command_attempt_id_sha256: value.bridge_command_attempt_id_sha256,
  };
}

function decodeFact(value: unknown): StoreFact | undefined {
  if (!isRecord(value) || !FACT_KINDS.has(String(value.kind))
    || !Number.isSafeInteger(value.ordinal) || (value.ordinal as number) < 1 || !isText(value.provenance)) return undefined;
  const account = decodeAccount(value.account);
  const attempt = decodeAttempt(value.attempt);
  if (!account || !attempt) return undefined;
  if (value.command !== undefined && !decodeCommand(value.command)) return undefined;
  if (value.terminal !== undefined && value.terminal !== "succeeded"
    && value.terminal !== "failed" && value.terminal !== "cancelled") return undefined;
  return {
    kind: value.kind as StoreFactKind,
    ordinal: value.ordinal as number,
    account,
    provenance: value.provenance,
    attempt,
    command: value.command === undefined ? undefined : decodeCommand(value.command),
    terminal: value.terminal as StoreFact["terminal"],
  };
}

function decodeWireEvent(value: unknown): StoreWireEvent | undefined {
  if (!isRecord(value) || !Number.isSafeInteger(value.wire_event_sequence)
    || (value.wire_event_sequence as number) < 1 || !isDigest(value.wire_event_sha256)
    || (value.wire_event_predecessor_sha256 !== null && !isDigest(value.wire_event_predecessor_sha256))) return undefined;
  return {
    wire_event_sequence: value.wire_event_sequence as number,
    wire_event_sha256: value.wire_event_sha256,
    wire_event_predecessor_sha256: value.wire_event_predecessor_sha256 as string | null,
  };
}

/** Decode Store adapter data without throwing or accepting a mutable replay cursor. */
export function decodeStoreSnapshot(value: unknown): StoreSnapshotDecode {
  if (!isRecord(value) || !Array.isArray(value.facts) || !Array.isArray(value.wireEvents))
    return { ok: false, reason: "invalid-snapshot" };
  const account = decodeAccount(value.account);
  if (!account) return { ok: false, reason: "missing-role-or-provenance" };
  const facts = value.facts.map(decodeFact);
  const wireEvents = value.wireEvents.map(decodeWireEvent);
  if (facts.some((fact) => !fact) || wireEvents.some((event) => !event))
    return { ok: false, reason: "missing-role-or-provenance" };
  const snapshot: StoreSnapshot = { account, facts: facts as StoreFact[], wireEvents: wireEvents as StoreWireEvent[] };
  const analysis = analyze(snapshot);
  return analysis.reason === undefined ? { ok: true, snapshot } : { ok: false, reason: analysis.reason };
}

function sameAccount(left: StoreAccount, right: StoreAccount): boolean {
  return left.id === right.id && left.account_role === right.account_role;
}

function sameAttempt(left: StoreAttempt, right: StoreAttempt): boolean {
  return left.subject === right.subject && left.execution_attempt_id === right.execution_attempt_id
    && left.execution_attempt_manifest_sha256 === right.execution_attempt_manifest_sha256
    && left.execution_attempt_account_id === right.execution_attempt_account_id
    && left.execution_attempt_account_role === right.execution_attempt_account_role;
}

function one(facts: readonly StoreFact[], kind: StoreFactKind): StoreFact | undefined {
  const matches = facts.filter((fact) => fact.kind === kind);
  return matches.length === 1 ? matches[0] : undefined;
}

function analyzeWireEvents(events: readonly StoreWireEvent[]): StoreInvalidReason | number {
  if (events.length > MAX_WIRE_EVENTS) return "invalid-snapshot";
  const ordered = [...events].sort((left, right) => left.wire_event_sequence - right.wire_event_sequence);
  let previous: StoreWireEvent | undefined;
  for (const event of ordered) {
    if (!Number.isSafeInteger(event.wire_event_sequence) || event.wire_event_sequence < 1
      || !isDigest(event.wire_event_sha256)
      || (event.wire_event_predecessor_sha256 !== null && !isDigest(event.wire_event_predecessor_sha256)))
      return "invalid-snapshot";
    if (!previous) {
      if (event.wire_event_sequence !== 1) return "sequence-gap";
      if (event.wire_event_predecessor_sha256 !== null) return "digest-conflict";
    } else {
      if (event.wire_event_sequence === previous.wire_event_sequence) return "digest-conflict";
      if (event.wire_event_sequence !== previous.wire_event_sequence + 1) return "sequence-gap";
      if (event.wire_event_predecessor_sha256 !== previous.wire_event_sha256) return "digest-conflict";
    }
    previous = event;
  }
  return previous?.wire_event_sequence ?? 0;
}

function analyze(snapshot: StoreSnapshot): Analysis {
  if (!snapshot || !snapshot.account || !Array.isArray(snapshot.facts) || !Array.isArray(snapshot.wireEvents))
    return { reason: "invalid-snapshot", replayPosition: 0, facts: [] };
  const replay = analyzeWireEvents(snapshot.wireEvents);
  if (typeof replay !== "number") return { reason: replay, replayPosition: 0, facts: [] };
  if (!isText(snapshot.account.id)
    || (snapshot.account.account_role !== "execution" && snapshot.account.account_role !== "oversight"))
    return { reason: "missing-role-or-provenance", replayPosition: 0, facts: [] };
  if (snapshot.facts.length === 0) return { replayPosition: replay, facts: [] };

  const facts = [...snapshot.facts].sort((left, right) => left.ordinal - right.ordinal);
  const first = facts[0]!;
  for (let index = 0; index < facts.length; index += 1) {
    const fact = facts[index]!;
    if (!FACT_KINDS.has(fact.kind) || !Number.isSafeInteger(fact.ordinal) || fact.ordinal < 1
      || !isText(fact.provenance) || !sameAccount(snapshot.account, fact.account)
      || !sameAttempt(first.attempt, fact.attempt)
      || fact.attempt.execution_attempt_account_id !== snapshot.account.id
      || fact.attempt.execution_attempt_account_role !== snapshot.account.account_role)
      return { reason: "account-conflict", replayPosition: 0, facts };
    if (index > 0 && fact.ordinal === facts[index - 1]!.ordinal)
      return { reason: "illegal-combination", replayPosition: 0, facts };
  }
  if (first.kind !== "reserved" || facts.filter((fact) => fact.kind === "reserved").length !== 1)
    return { reason: "illegal-combination", replayPosition: 0, facts };
  for (const kind of ["launch-intent", "started", "cancel-request", "cancel-intent", "cancel-receipt", "terminal", "proved-unsent", "advanced"] as const)
    if (facts.filter((fact) => fact.kind === kind).length > 1)
      return { reason: "illegal-combination", replayPosition: 0, facts };

  const launch = one(facts, "launch-intent");
  const started = one(facts, "started");
  const terminal = one(facts, "terminal");
  const provedUnsent = one(facts, "proved-unsent");
  const advanced = one(facts, "advanced");
  if ((started || terminal || provedUnsent) && (!launch || (started && started.ordinal <= launch.ordinal)
    || (terminal && terminal.ordinal <= launch.ordinal) || (provedUnsent && provedUnsent.ordinal <= launch.ordinal)))
    return { reason: "illegal-combination", replayPosition: 0, facts };
  if (terminal && provedUnsent) return { reason: "illegal-combination", replayPosition: 0, facts };
  if (advanced && ((!terminal && !provedUnsent) || advanced.ordinal <= (terminal ?? provedUnsent)!.ordinal))
    return { reason: "illegal-combination", replayPosition: 0, facts };

  const cancelRequest = one(facts, "cancel-request");
  const cancelIntent = one(facts, "cancel-intent");
  const cancelReceipt = one(facts, "cancel-receipt");
  if ((cancelRequest && (!started || cancelRequest.ordinal <= started.ordinal))
    || (cancelIntent && (!cancelRequest || cancelIntent.ordinal <= cancelRequest.ordinal))
    || (cancelReceipt && (!cancelIntent || cancelReceipt.ordinal <= cancelIntent.ordinal))
    || (cancelIntent && !cancelIntent.command)
    || (cancelReceipt && (!cancelReceipt.command || cancelReceipt.command.subject !== cancelIntent!.command!.subject)))
    return { reason: "illegal-combination", replayPosition: 0, facts };

  const commandSubjects = new Set<string>();
  for (const fact of facts) if (fact.kind === "command-intent" || fact.kind === "delivery-intent" || fact.kind === "delivery-receipt") {
    if (!fact.command || !started || fact.ordinal <= started.ordinal) return { reason: "illegal-combination", replayPosition: 0, facts };
    commandSubjects.add(fact.command.subject);
  }
  for (const subject of commandSubjects) {
    const commandFacts = facts.filter((fact) => fact.command?.subject === subject);
    const intent = commandFacts.filter((fact) => fact.kind === "command-intent");
    const deliveryIntent = commandFacts.filter((fact) => fact.kind === "delivery-intent");
    const receipt = commandFacts.filter((fact) => fact.kind === "delivery-receipt");
    if (intent.length !== 1 || deliveryIntent.length > 1 || receipt.length > 1
      || (deliveryIntent[0] && deliveryIntent[0]!.ordinal <= intent[0]!.ordinal)
      || (receipt[0] && (!deliveryIntent[0] || receipt[0]!.ordinal <= deliveryIntent[0]!.ordinal)))
      return { reason: "illegal-combination", replayPosition: 0, facts };
  }
  return { replayPosition: replay, facts };
}

/** Greatest contiguous validated `wire_event_sequence`; lifecycle facts never advance it. */
export function storeReplayPosition(snapshot: StoreSnapshot): number {
  const analysis = analyze(snapshot);
  return analysis.reason === undefined ? analysis.replayPosition : 0;
}

/** Determine the next conservative Store action from immutable Store authorities alone. */
export function safeNext(snapshot: StoreSnapshot): StoreAction {
  const analysis = analyze(snapshot);
  if (analysis.reason !== undefined) return { kind: "invalid", reason: analysis.reason, replayPosition: 0 };
  const { facts, replayPosition } = analysis;
  if (snapshot.account.account_role === "oversight")
    return { kind: "no-op", reason: "oversight-account", replayPosition };
  if (facts.length === 0) return { kind: "reserve", replayPosition };

  const attempt = facts[0]!.attempt;
  const has = (kind: StoreFactKind) => facts.some((fact) => fact.kind === kind);
  if (has("terminal") || has("proved-unsent")) return has("advanced")
    ? { kind: "no-op", reason: "settled", replayPosition }
    : { kind: "advance", attempt, replayPosition };
  if (!has("launch-intent")) return { kind: "launch", attempt, replayPosition };
  if (!has("started")) return { kind: "reconcile-launch", attempt, replayPosition };
  if (has("cancel-request")) {
    if (!has("cancel-intent")) return { kind: "cancel", attempt, replayPosition };
    if (!has("cancel-receipt")) return {
      kind: "reconcile-command", attempt,
      command: facts.find((fact) => fact.kind === "cancel-intent")!.command!, replayPosition,
    };
    return { kind: "no-op", reason: "awaiting-terminal", replayPosition };
  }
  for (const fact of facts) if (fact.kind === "command-intent") {
    const command = fact.command!;
    if (facts.some((candidate) => candidate.kind === "delivery-receipt" && candidate.command?.subject === command.subject)) continue;
    return facts.some((candidate) => candidate.kind === "delivery-intent" && candidate.command?.subject === command.subject)
      ? { kind: "reconcile-command", attempt, command, replayPosition }
      : { kind: "send", attempt, command, replayPosition };
  }
  return { kind: "no-op", reason: "nothing-pending", replayPosition };
}
