/**
 * The Store decision kernel deliberately knows only immutable Store facts.
 * It performs no IO and does not manufacture replay state: the contiguous log
 * prefix is derived on every call.
 */
export type StoreRole = "executor" | "oversight";

export interface StoreAccount {
  readonly id: string;
  readonly role: StoreRole;
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
  | "advanced";

/** One signed, ordered Store fact for a single attempt. */
export interface StoreFact {
  readonly kind: StoreFactKind;
  readonly sequence: number;
  readonly digest: string;
  readonly previousDigest: string | null;
  readonly account: StoreAccount;
  readonly provenance: string;
  readonly attempt: string;
  /** Required only by command and delivery facts. */
  readonly command?: string;
  /** Required only by a terminal fact. */
  readonly terminal?: "succeeded" | "failed" | "cancelled";
}

export interface StoreSnapshot {
  readonly account: StoreAccount;
  readonly facts: readonly StoreFact[];
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
  | Readonly<{ kind: "launch"; attempt: string; replayPosition: number }>
  | Readonly<{ kind: "send"; attempt: string; command: string; replayPosition: number }>
  | Readonly<{ kind: "cancel"; attempt: string; replayPosition: number }>
  | Readonly<{ kind: "reconcile-launch"; attempt: string; replayPosition: number }>
  | Readonly<{ kind: "reconcile-command"; attempt: string; command: string; replayPosition: number }>
  | Readonly<{ kind: "advance"; attempt: string; replayPosition: number }>
  | Readonly<{ kind: "no-op"; reason: "oversight-account" | "awaiting-terminal" | "settled" | "nothing-pending"; replayPosition: number }>
  | Readonly<{ kind: "invalid"; reason: StoreInvalidReason; replayPosition: 0 }>;

interface Analysis {
  readonly reason?: StoreInvalidReason;
  readonly replayPosition: number;
  readonly facts: readonly StoreFact[];
}

const FACT_KINDS: ReadonlySet<string> = new Set<StoreFactKind>([
  "reserved", "launch-intent", "started", "command-intent", "delivery-intent",
  "delivery-receipt", "cancel-request", "cancel-intent", "cancel-receipt", "terminal", "advanced",
]);

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isText(value: unknown): value is string {
  return typeof value === "string" && value.length > 0;
}

function decodeAccount(value: unknown): StoreAccount | undefined {
  if (!isRecord(value) || !isText(value.id)
    || (value.role !== "executor" && value.role !== "oversight")) return undefined;
  return { id: value.id, role: value.role };
}

function decodeFact(value: unknown): StoreFact | undefined {
  if (!isRecord(value) || !FACT_KINDS.has(String(value.kind))
    || !Number.isSafeInteger(value.sequence) || (value.sequence as number) < 1
    || !isText(value.digest) || !isText(value.provenance) || !isText(value.attempt)) return undefined;
  if (value.previousDigest !== null && !isText(value.previousDigest)) return undefined;
  const account = decodeAccount(value.account);
  if (!account) return undefined;
  if (value.command !== undefined && !isText(value.command)) return undefined;
  if (value.terminal !== undefined && value.terminal !== "succeeded"
    && value.terminal !== "failed" && value.terminal !== "cancelled") return undefined;
  return {
    kind: value.kind as StoreFactKind,
    sequence: value.sequence as number,
    digest: value.digest,
    previousDigest: value.previousDigest as string | null,
    account,
    provenance: value.provenance,
    attempt: value.attempt,
    command: value.command as string | undefined,
    terminal: value.terminal as StoreFact["terminal"],
  };
}

/** Decode untrusted data without throwing or accepting a mutable replay cursor. */
export function decodeStoreSnapshot(value: unknown): StoreSnapshotDecode {
  if (!isRecord(value) || !Array.isArray(value.facts))
    return { ok: false, reason: "invalid-snapshot" };
  const account = decodeAccount(value.account);
  if (!account) return { ok: false, reason: "missing-role-or-provenance" };
  const facts: StoreFact[] = [];
  for (const encoded of value.facts) {
    const fact = decodeFact(encoded);
    if (!fact) return { ok: false, reason: "missing-role-or-provenance" };
    facts.push(fact);
  }
  const snapshot: StoreSnapshot = { account, facts };
  const analysis = analyze(snapshot);
  return analysis.reason === undefined
    ? { ok: true, snapshot }
    : { ok: false, reason: analysis.reason };
}

function sameAccount(left: StoreAccount, right: StoreAccount): boolean {
  return left.id === right.id && left.role === right.role;
}

function hasOnlyOne(facts: readonly StoreFact[], kind: StoreFactKind): boolean {
  return facts.filter((fact) => fact.kind === kind).length <= 1;
}

function after(facts: readonly StoreFact[], left: StoreFactKind, right: StoreFactKind): boolean {
  const leftFact = facts.find((fact) => fact.kind === left);
  const rightFact = facts.find((fact) => fact.kind === right);
  return leftFact !== undefined && rightFact !== undefined && leftFact.sequence > rightFact.sequence;
}

function analyze(snapshot: StoreSnapshot): Analysis {
  if (!snapshot || !snapshot.account || !Array.isArray(snapshot.facts))
    return { reason: "invalid-snapshot", replayPosition: 0, facts: [] };
  if (!isText(snapshot.account.id)
    || (snapshot.account.role !== "executor" && snapshot.account.role !== "oversight"))
    return { reason: "missing-role-or-provenance", replayPosition: 0, facts: [] };
  if (snapshot.facts.length === 0) return { replayPosition: 0, facts: [] };

  const facts = [...snapshot.facts].sort((left, right) => left.sequence - right.sequence);
  let previous: StoreFact | undefined;
  for (const fact of facts) {
    if (!FACT_KINDS.has(fact.kind)
      || !Number.isSafeInteger(fact.sequence) || fact.sequence < 1
      || !isText(fact.digest) || !isText(fact.provenance) || !isText(fact.attempt)
      || !isText(fact.account.id)
      || (fact.account.role !== "executor" && fact.account.role !== "oversight")) {
      return { reason: "missing-role-or-provenance", replayPosition: 0, facts };
    }
    if (!sameAccount(snapshot.account, fact.account)) {
      return { reason: "account-conflict", replayPosition: 0, facts };
    }
    if (previous === undefined) {
      if (fact.sequence !== 1 || fact.previousDigest !== null)
        return { reason: fact.sequence !== 1 ? "sequence-gap" : "digest-conflict", replayPosition: 0, facts };
    } else {
      if (fact.sequence === previous.sequence) return { reason: "digest-conflict", replayPosition: 0, facts };
      if (fact.sequence !== previous.sequence + 1) return { reason: "sequence-gap", replayPosition: 0, facts };
      if (fact.previousDigest !== previous.digest) return { reason: "digest-conflict", replayPosition: 0, facts };
      if (fact.attempt !== previous.attempt) return { reason: "attempt-conflict", replayPosition: 0, facts };
    }
    previous = fact;
  }

  const first = facts[0]!;
  if (first.kind !== "reserved" || !hasOnlyOne(facts, "reserved"))
    return { reason: "illegal-combination", replayPosition: 0, facts };
  for (const kind of ["launch-intent", "started", "cancel-request", "cancel-intent", "cancel-receipt", "terminal", "advanced"] as const) {
    if (!hasOnlyOne(facts, kind)) return { reason: "illegal-combination", replayPosition: 0, facts };
  }
  if (facts.some((fact) => (fact.kind === "command-intent" || fact.kind === "delivery-intent" || fact.kind === "delivery-receipt") && !isText(fact.command)))
    return { reason: "illegal-combination", replayPosition: 0, facts };
  if (facts.some((fact) => fact.kind === "terminal" && fact.terminal === undefined))
    return { reason: "illegal-combination", replayPosition: 0, facts };
  if (facts.some((fact) => fact.kind !== "reserved" && !after(facts, fact.kind, "reserved")))
    return { reason: "illegal-combination", replayPosition: 0, facts };
  if ((facts.some((fact) => fact.kind === "started") || facts.some((fact) => fact.kind === "terminal"))
    && !facts.some((fact) => fact.kind === "launch-intent"))
    return { reason: "illegal-combination", replayPosition: 0, facts };
  if (facts.some((fact) => fact.kind === "started") && !after(facts, "started", "launch-intent"))
    return { reason: "illegal-combination", replayPosition: 0, facts };
  if (facts.some((fact) => fact.kind === "terminal") && !after(facts, "terminal", "launch-intent"))
    return { reason: "illegal-combination", replayPosition: 0, facts };
  if (facts.some((fact) => fact.kind === "advanced") && !after(facts, "advanced", "terminal"))
    return { reason: "illegal-combination", replayPosition: 0, facts };
  const cancelRequest = facts.find((fact) => fact.kind === "cancel-request");
  const startReceipt = facts.find((fact) => fact.kind === "started");
  if (cancelRequest && (!startReceipt || cancelRequest.sequence <= startReceipt.sequence))
    return { reason: "illegal-combination", replayPosition: 0, facts };
  if (facts.some((fact) => fact.kind === "cancel-intent") && !after(facts, "cancel-intent", "cancel-request"))
    return { reason: "illegal-combination", replayPosition: 0, facts };
  if (facts.some((fact) => fact.kind === "cancel-receipt") && !after(facts, "cancel-receipt", "cancel-intent"))
    return { reason: "illegal-combination", replayPosition: 0, facts };

  const commands = new Map<string, StoreFactKind[]>();
  for (const fact of facts) if (fact.command) {
    const kinds = commands.get(fact.command) ?? [];
    kinds.push(fact.kind);
    commands.set(fact.command, kinds);
  }
  for (const [command, kinds] of commands) {
    const intent = facts.find((fact) => fact.kind === "command-intent" && fact.command === command);
    const deliveryIntent = facts.find((fact) => fact.kind === "delivery-intent" && fact.command === command);
    const deliveryReceipt = facts.find((fact) => fact.kind === "delivery-receipt" && fact.command === command);
    const started = facts.find((fact) => fact.kind === "started");
    if (!intent || !started || intent.sequence <= started.sequence
      || kinds.filter((kind) => kind === "command-intent").length !== 1
      || kinds.filter((kind) => kind === "delivery-intent").length > 1
      || kinds.filter((kind) => kind === "delivery-receipt").length > 1
      || (deliveryIntent && deliveryIntent.sequence <= intent.sequence)
      || (deliveryReceipt && (!deliveryIntent || deliveryReceipt.sequence <= deliveryIntent.sequence)))
      return { reason: "illegal-combination", replayPosition: 0, facts };
  }
  return { replayPosition: facts.at(-1)!.sequence, facts };
}

/** Greatest contiguous, validated Store sequence; invalid input has no replay position. */
export function storeReplayPosition(snapshot: StoreSnapshot): number {
  const analysis = analyze(snapshot);
  return analysis.reason === undefined ? analysis.replayPosition : 0;
}

/** Determine the next conservative Store action from immutable facts alone. */
export function safeNext(snapshot: StoreSnapshot): StoreAction {
  const analysis = analyze(snapshot);
  if (analysis.reason !== undefined)
    return { kind: "invalid", reason: analysis.reason, replayPosition: 0 };
  const { facts, replayPosition } = analysis;
  if (snapshot.account.role === "oversight")
    return { kind: "no-op", reason: "oversight-account", replayPosition };
  if (facts.length === 0) return { kind: "reserve", replayPosition };

  const attempt = facts[0]!.attempt;
  const has = (kind: StoreFactKind) => facts.some((fact) => fact.kind === kind);
  if (has("terminal"))
    return has("advanced")
      ? { kind: "no-op", reason: "settled", replayPosition }
      : { kind: "advance", attempt, replayPosition };
  if (!has("launch-intent")) return { kind: "launch", attempt, replayPosition };
  if (!has("started")) return { kind: "reconcile-launch", attempt, replayPosition };

  if (has("cancel-request")) {
    if (!has("cancel-intent")) return { kind: "cancel", attempt, replayPosition };
    if (!has("cancel-receipt"))
      return { kind: "reconcile-command", attempt, command: "cancel", replayPosition };
    return { kind: "no-op", reason: "awaiting-terminal", replayPosition };
  }

  const commands = facts.filter((fact) => fact.kind === "command-intent");
  for (const commandFact of commands) {
    const command = commandFact.command!;
    const delivered = facts.some((fact) => fact.kind === "delivery-receipt" && fact.command === command);
    if (delivered) continue;
    const deliveryIntent = facts.some((fact) => fact.kind === "delivery-intent" && fact.command === command);
    return deliveryIntent
      ? { kind: "reconcile-command", attempt, command, replayPosition }
      : { kind: "send", attempt, command, replayPosition };
  }
  return { kind: "no-op", reason: "nothing-pending", replayPosition };
}
