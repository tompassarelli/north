import { Database } from "bun:sqlite";
import { expect, test } from "bun:test";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const BRIEFS = [
  "Delegation: role=worker budget=0\nCategory: bug\nSetup: load skills",
  "Category: bug-known-cause\nScope: module\nSurface: ts\nVerify: run the thing",
  "Read /home/tom/.local/state/agents/handoffs/foo-brief.md and follow it.",
  "read /home/tom/.local/state/some-evidence/brief.md and follow it",
  "You are the north lead. Your goal is to land the fix.",
  "<task-notification><task-id>abc</task-id>done</task-notification>",
  "[routine:tick] check the queue",
  "Another Claude session sent a message: please look at this",
  "Coach reply from north-lead, relay verbatim: use the lane",
  "<system-reminder>hook says stop</system-reminder>",
];

test("transcripts: brief-shaped user text is never indexed as Tom and Tom's own messages are, in proxy, lead and worker sessions", async () => {
  const dir = mkdtempSync(join(tmpdir(), "grounding-transcripts-"));
  const path = join(dir, "convo.db");
  const db = new Database(path);
  db.exec(`CREATE TABLE files(id INTEGER PRIMARY KEY, session_id TEXT, agent TEXT);
    CREATE TABLE body(id INTEGER PRIMARY KEY, text TEXT);
    CREATE TABLE msg(id INTEGER PRIMARY KEY, file_id INTEGER, line INTEGER, ts TEXT, role TEXT, body INTEGER, dup INTEGER DEFAULT 0);`);
  const sessions: [number, string, string][] = [
    [1, "aaaaaaaa", "Read /home/tom/.local/state/agents/handoffs/proxy-handoff-2026-10-13-c.md and take over as the proxy."],
    [2, "bbbbbbbb", "You are the north lead. Run the campaign."],
    [3, "cccccccc", "Delegation: role=worker budget=0\nCategory: bug\nScope: module"],
    [4, "dddddddd", "please fix the grounding recall"],
  ];
  let n = 0;
  const add = (file: number, role: string, text: string) => {
    n++;
    db.run("insert into body values (?, ?)", [n, text]);
    db.run("insert into msg(id, file_id, line, ts, role, body) values (?, ?, ?, ?, ?, ?)", [n, file, n, new Date(Date.now() - n * 1000).toISOString(), role, n]);
    return n;
  };
  const ids: Record<string, number> = {};
  for (const [file, session, opening] of sessions) {
    db.run("insert into files values (?, ?, NULL)", [file, session]);
    add(file, "user", opening);
    BRIEFS.forEach((b) => add(file, "user", b));
    ids[`tom${file}`] = add(file, "user", `tom typed this into session ${session} about retrieval`);
    ids[`reply${file}`] = add(file, "assistant", `assistant reply in session ${session} with several words`);
  }
  db.close();
  process.env.NORTH_GROUNDING_CONVO_DB = path;
  try {
    const { iterDocs } = await import("./transcripts.ts");
    const docs = [...iterDocs(null)];
    const byId = new Map(docs.map((d) => [d.id, d]));
    for (const d of docs) if (d.title.startsWith("Tom ")) expect(d.text).toMatch(/^(tom typed this|please fix the grounding)/);
    for (const file of [1, 2, 3, 4]) expect(byId.get(String(ids[`tom${file}`]))?.title).toStartWith("Tom ");
    expect(byId.get(String(ids.reply1))?.title).toStartWith("proxy ");
    expect(byId.get(String(ids.reply4))?.title).toStartWith("proxy ");
    expect(byId.has(String(ids.reply2))).toBe(false);
    expect(byId.has(String(ids.reply3))).toBe(false);
    expect(docs.filter((d) => d.title.startsWith("Tom ")).length).toBe(5);
  } finally {
    rmSync(dir, { recursive: true });
  }
});
