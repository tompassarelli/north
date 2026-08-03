import {
  chmodSync, closeSync, constants, fsyncSync, mkdirSync, openSync, readFileSync,
  readdirSync, linkSync, unlinkSync, writeSync, type Dirent,
} from "node:fs";
import { randomUUID } from "node:crypto";
import { join } from "node:path";
import { bridgeJournalRoot } from "./protocol";
import {
  LANE_LIFECYCLE_KINDS, scanJournalFile, type JournalRecord,
} from "./journal";

const SAFE_EXECUTION_ID = /^[A-Za-z0-9._-]+$/;

export interface PendingLane {
  executionId: string;
  terminal: JournalRecord;
  harvest?: JournalRecord;
}

interface ConsumptionMarker {
  version: 1;
  executionId: string;
  terminalSeq: number;
  consumedAt: string;
}

function executionId(value: string): string {
  if (!SAFE_EXECUTION_ID.test(value))
    throw new Error("bridge pending execution id contains unsupported characters");
  return value;
}

function markerDirectory(root: string): string {
  return join(root, "consumption");
}

function markerPath(root: string, id: string): string {
  return join(markerDirectory(root), `${executionId(id)}.json`);
}

function readMarker(root: string, id: string): ConsumptionMarker | undefined {
  let value: unknown;
  try { value = JSON.parse(readFileSync(markerPath(root, id), "utf8")); }
  catch (error) {
    if (error instanceof Error && "code" in error && error.code === "ENOENT") return undefined;
    throw error;
  }
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw new Error(`bridge consumption marker for ${id} is invalid`);
  const marker = value as Record<string, unknown>;
  if (marker.version !== 1 || marker.executionId !== id
      || !Number.isSafeInteger(marker.terminalSeq) || (marker.terminalSeq as number) < 1
      || typeof marker.consumedAt !== "string" || !marker.consumedAt)
    throw new Error(`bridge consumption marker for ${id} is invalid`);
  return marker as unknown as ConsumptionMarker;
}

function laneRecords(root: string, id: string): JournalRecord[] {
  return scanJournalFile(join(root, executionId(id), "events.log"), id).records;
}

function terminalLane(root: string, id: string): PendingLane {
  const records = laneRecords(root, id);
  const terminal = records.findLast((record) => record.kind === LANE_LIFECYCLE_KINDS.terminal);
  if (!terminal) throw new Error(`bridge lane ${id} has no terminal lifecycle event`);
  return {
    executionId: id,
    terminal,
    harvest: records.findLast((record) => record.kind === LANE_LIFECYCLE_KINDS.harvest),
  };
}

export function pendingLanes(root = bridgeJournalRoot()): PendingLane[] {
  let entries: Dirent[];
  try { entries = readdirSync(root, { withFileTypes: true }); }
  catch (error) {
    if (error instanceof Error && "code" in error && error.code === "ENOENT") return [];
    throw error;
  }
  const pending: PendingLane[] = [];
  for (const entry of entries.sort((left, right) => left.name.localeCompare(right.name))) {
    if (!entry.isDirectory()) continue;
    const id = entry.name;
    if (!SAFE_EXECUTION_ID.test(id) || id === "consumption" || readMarker(root, id)) continue;
    const records = laneRecords(root, id);
    const terminal = records.findLast(
      (record) => record.kind === LANE_LIFECYCLE_KINDS.terminal,
    );
    if (!terminal) continue;
    pending.push({
      executionId: id,
      terminal,
      harvest: records.findLast((record) => record.kind === LANE_LIFECYCLE_KINDS.harvest),
    });
  }
  return pending.sort((left, right) =>
    left.terminal.at.localeCompare(right.terminal.at)
      || left.executionId.localeCompare(right.executionId));
}

/** Returns true only when this call created the durable consumption marker. */
export function markLaneConsumed(
  id: string,
  root = bridgeJournalRoot(),
): boolean {
  const lane = terminalLane(root, executionId(id));
  const directory = markerDirectory(root);
  mkdirSync(directory, { recursive: true, mode: 0o700 });
  chmodSync(directory, 0o700);
  const marker: ConsumptionMarker = {
    version: 1,
    executionId: lane.executionId,
    terminalSeq: lane.terminal.seq,
    consumedAt: new Date().toISOString(),
  };
  const temporary = join(directory, `.${lane.executionId}.${randomUUID()}.tmp`);
  const fd = openSync(
    temporary,
    constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL,
    0o600,
  );
  try {
    const bytes = Buffer.from(`${JSON.stringify(marker)}\n`, "utf8");
    let offset = 0;
    while (offset < bytes.byteLength) offset += writeSync(fd, bytes, offset);
    fsyncSync(fd);
  } finally {
    closeSync(fd);
  }
  let created = false;
  try {
    linkSync(temporary, markerPath(root, lane.executionId));
    created = true;
  } catch (error) {
    if (!(error instanceof Error && "code" in error && error.code === "EEXIST")) throw error;
    const existing = readMarker(root, lane.executionId)!;
    if (existing.terminalSeq !== lane.terminal.seq)
      throw new Error(`bridge consumption marker for ${id} names another terminal`);
  } finally {
    unlinkSync(temporary);
  }
  const directoryFd = openSync(directory, constants.O_RDONLY);
  try { fsyncSync(directoryFd); }
  finally { closeSync(directoryFd); }
  return created;
}
