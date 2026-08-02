import { afterEach, expect, test } from "bun:test";
import { appendFileSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  ExecutionJournal, JournalTornTailError, scanJournalFile,
} from "../src/bridge/journal";

const roots: string[] = [];
afterEach(() => {
  for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});

function root(): string {
  const path = mkdtempSync(join(tmpdir(), "north-bridge-journal-"));
  roots.push(path);
  return path;
}

test("execution journal appends durable length-prefixed records and replays by sequence", () => {
  const journalRoot = root();
  const journal = new ExecutionJournal(journalRoot, "execution-1");
  const accepted = journal.append("execution.accepted", { prompt: "hello" });
  const text = journal.append("provider.assistant", { text: "world" });
  journal.close();

  const scan = scanJournalFile(join(journalRoot, "execution-1", "events.log"), "execution-1");
  expect(scan.tornTail).toBeUndefined();
  expect(scan.records).toEqual([accepted, text]);
  expect(scan.records.map((record) => record.seq)).toEqual([1, 2]);
  expect(scan.committedBytes).toBeGreaterThan(8);
});

test("execution journal reports a torn body after the committed prefix and never rewrites it", () => {
  const journalRoot = root();
  const journal = new ExecutionJournal(journalRoot, "execution-torn");
  const committed = journal.append("execution.accepted", { prompt: "keep me" });
  journal.close();
  const path = join(journalRoot, "execution-torn", "events.log");
  const torn = Buffer.alloc(7);
  torn.writeUInt32BE(20, 0);
  torn.write("bad", 4, "utf8");
  appendFileSync(path, torn);

  const scan = scanJournalFile(path, "execution-torn");
  expect(scan.records).toEqual([committed]);
  expect(scan.tornTail).toEqual({
    offset: scan.committedBytes,
    availableBytes: 7,
    requiredBytes: 24,
  });

  const reopened = new ExecutionJournal(journalRoot, "execution-torn");
  expect(() => reopened.append("execution.completed"))
    .toThrow(JournalTornTailError);
  reopened.close();
  expect(scanJournalFile(path, "execution-torn")).toEqual(scan);
});

test("execution journal reports a partial length prefix as a torn tail", () => {
  const journalRoot = root();
  const journal = new ExecutionJournal(journalRoot, "execution-prefix");
  journal.append("execution.accepted");
  journal.close();
  const path = join(journalRoot, "execution-prefix", "events.log");
  appendFileSync(path, Buffer.from([0, 1]));

  const scan = scanJournalFile(path, "execution-prefix");
  expect(scan.records).toHaveLength(1);
  expect(scan.tornTail).toEqual({
    offset: scan.committedBytes,
    availableBytes: 2,
    requiredBytes: 4,
  });
});
