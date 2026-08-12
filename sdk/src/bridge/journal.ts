import {
  chmodSync, constants, closeSync, fsyncSync, mkdirSync, openSync, readFileSync, writeSync,
} from "node:fs";
import { join } from "node:path";
import {
  openWireJsonlWriter, readWireJsonl,
  type WireJsonlOptions, type WireJsonlReplay, type WireJsonlWriter,
} from "../wire/jsonl";
import type { WireEvent } from "../wire/events";

const LENGTH_BYTES = 4;
const MAX_RECORD_BYTES = 8 * 1024 * 1024;
const SAFE_EXECUTION_ID = /^[A-Za-z0-9._-]+$/;

export const BRIDGE_WIRE_FILE = "wire.jsonl";

export const LANE_LIFECYCLE_KINDS = {
  spawnStart: "lane.spawn-start",
  identityAdmitted: "lane.identity-admitted",
  turnBoundary: "lane.turn-boundary",
  terminal: "lane.terminal",
  harvest: "lane.harvest",
} as const;

export interface JournalRecord {
  version: 1;
  executionId: string;
  seq: number;
  at: string;
  kind: string;
  data: Record<string, unknown>;
}

export interface TornTail {
  offset: number;
  availableBytes: number;
  requiredBytes: number;
}

export interface JournalScan {
  records: JournalRecord[];
  committedBytes: number;
  tornTail?: TornTail;
}

export class JournalTornTailError extends Error {
  constructor(readonly tornTail: TornTail) {
    super(`journal has a torn tail at byte ${tornTail.offset}`);
    this.name = "JournalTornTailError";
  }
}

function validateRecord(value: unknown, expectedExecutionId?: string): JournalRecord {
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw new Error("journal record must be an object");
  const record = value as Record<string, unknown>;
  if (record.version !== 1 || typeof record.executionId !== "string" || !record.executionId
      || !Number.isSafeInteger(record.seq) || (record.seq as number) < 1
      || typeof record.at !== "string" || !record.at
      || typeof record.kind !== "string" || !record.kind
      || !record.data || typeof record.data !== "object" || Array.isArray(record.data))
    throw new Error("journal record has an invalid v1 shape");
  if (expectedExecutionId && record.executionId !== expectedExecutionId)
    throw new Error("journal record belongs to another execution");
  return value as JournalRecord;
}

export function scanJournalFile(path: string, expectedExecutionId?: string): JournalScan {
  let bytes: Buffer;
  try { bytes = readFileSync(path); }
  catch (error) {
    if (error instanceof Error && "code" in error && error.code === "ENOENT")
      return { records: [], committedBytes: 0 };
    throw error;
  }
  const records: JournalRecord[] = [];
  let offset = 0;
  while (offset < bytes.byteLength) {
    const available = bytes.byteLength - offset;
    if (available < LENGTH_BYTES) {
      return {
        records, committedBytes: offset,
        tornTail: { offset, availableBytes: available, requiredBytes: LENGTH_BYTES },
      };
    }
    const payloadBytes = bytes.readUInt32BE(offset);
    if (payloadBytes === 0 || payloadBytes > MAX_RECORD_BYTES)
      throw new Error(`journal record at byte ${offset} has invalid length ${payloadBytes}`);
    const frameBytes = LENGTH_BYTES + payloadBytes;
    if (available < frameBytes) {
      return {
        records, committedBytes: offset,
        tornTail: { offset, availableBytes: available, requiredBytes: frameBytes },
      };
    }
    let parsed: unknown;
    try {
      parsed = JSON.parse(bytes.subarray(offset + LENGTH_BYTES, offset + frameBytes).toString("utf8"));
    } catch (cause) {
      throw new Error(`journal record at byte ${offset} is invalid JSON`, { cause });
    }
    const record = validateRecord(parsed, expectedExecutionId);
    const expectedSeq = records.length + 1;
    if (record.seq !== expectedSeq)
      throw new Error(`journal sequence is ${record.seq} at byte ${offset}; expected ${expectedSeq}`);
    records.push(record);
    offset += frameBytes;
  }
  return { records, committedBytes: offset };
}

function writeAll(fd: number, bytes: Buffer): void {
  let offset = 0;
  while (offset < bytes.byteLength) offset += writeSync(fd, bytes, offset);
}

export class ExecutionJournal {
  readonly path: string;
  #fd: number;
  #nextSeq: number;
  #tornTail?: TornTail;
  #closed = false;

  constructor(readonly root: string, readonly executionId: string) {
    const directory = join(root, executionId);
    mkdirSync(directory, { recursive: true, mode: 0o700 });
    chmodSync(directory, 0o700);
    this.path = join(directory, "events.log");
    const scan = scanJournalFile(this.path, executionId);
    this.#nextSeq = scan.records.length + 1;
    this.#tornTail = scan.tornTail;
    this.#fd = openSync(
      this.path,
      constants.O_WRONLY | constants.O_CREAT | constants.O_APPEND,
      0o600,
    );
    chmodSync(this.path, 0o600);
  }

  scan(): JournalScan {
    return scanJournalFile(this.path, this.executionId);
  }

  append(kind: string, data: Record<string, unknown> = {}): JournalRecord {
    if (this.#closed) throw new Error("journal is closed");
    if (this.#tornTail) throw new JournalTornTailError(this.#tornTail);
    const record: JournalRecord = {
      version: 1,
      executionId: this.executionId,
      seq: this.#nextSeq,
      at: new Date().toISOString(),
      kind,
      data,
    };
    const payload = Buffer.from(JSON.stringify(record), "utf8");
    if (payload.byteLength > MAX_RECORD_BYTES)
      throw new Error(`journal record exceeds ${MAX_RECORD_BYTES} bytes`);
    const frame = Buffer.allocUnsafe(LENGTH_BYTES + payload.byteLength);
    frame.writeUInt32BE(payload.byteLength, 0);
    payload.copy(frame, LENGTH_BYTES);
    writeAll(this.#fd, frame);
    fsyncSync(this.#fd);
    this.#nextSeq += 1;
    return record;
  }

  close(): void {
    if (this.#closed) return;
    this.#closed = true;
    closeSync(this.#fd);
  }
}

function bridgeWireDirectory(root: string, executionId: string): string {
  if (!SAFE_EXECUTION_ID.test(executionId)) {
    throw new Error("bridge wire execution id contains unsupported characters");
  }
  return join(root, executionId);
}

export function bridgeWirePath(root: string, executionId: string): string {
  return join(bridgeWireDirectory(root, executionId), BRIDGE_WIRE_FILE);
}

/**
 * Durable Bridge replay projection of the canonical wire stream. Control and
 * lane records remain in ExecutionJournal and never consume wire sequence.
 */
export class BridgeWireJournal {
  readonly path: string;
  #writer: WireJsonlWriter;

  constructor(writer: WireJsonlWriter) {
    this.path = writer.filePath;
    this.#writer = writer;
  }

  static async open(root: string, executionId: string): Promise<BridgeWireJournal> {
    const directory = bridgeWireDirectory(root, executionId);
    mkdirSync(directory, { recursive: true, mode: 0o700 });
    chmodSync(directory, 0o700);
    return new BridgeWireJournal(await openWireJsonlWriter(
      bridgeWirePath(root, executionId),
      { recoverDeadOwnerLock: true },
    ));
  }

  replay(): WireJsonlReplay {
    return this.#writer.replay();
  }

  async append(event: WireEvent): Promise<WireEvent> {
    return this.#writer.append(event);
  }

  async close(): Promise<void> {
    await this.#writer.close();
  }
}

export async function readBridgeWireJournal(
  root: string,
  executionId: string,
  options: WireJsonlOptions = {},
): Promise<WireJsonlReplay> {
  return readWireJsonl(bridgeWirePath(root, executionId), options);
}
