import { Database } from "bun:sqlite";
import { expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const TARGET = join(import.meta.dir, "distill.ts");

const SESSIONS: Record<string, [string, string][]> = {
  proxy: [
    ["user", "Read /h/.local/state/agents/handoffs/proxy-handoff-2026-10-13-c.md and take over as the proxy"],
    ["user", "its fine if we burn codex today, we have the windfall of 2 anthropic accounts.."],
    ["assistant", "Staffing now.\nDECIDED 06:20 codex lead takes the netcode box"],
    ["user", "Delegation: role=worker budget=0\nCategory: tooling. You must burn codex today."],
    ["user", "<task-notification>we need to land everything</task-notification>"],
    ["user", "you think all the songs should be 140bpm?"],
    ["user", "why did u do (Wallet) instead of (wallet)"],
    ["assistant", "Should I close waygate#1 with gh?"],
    ["user", "yes go ahead"],
    ["user", "I prefer anthropic to stay the dominant way: Claude first on every account, codex only for " +
      "tightly specified work so we burn usage evenly."],
    ["user", "we need a new rule change that everyone has to respond within 2-5 minutes"],
    ["user", "we need a new rule change that everyone has to respond within 2-5 minutes"],
    ["user", "you keep answering questions without checking skills and transcripts"],
    ["user", "can we update policy durably to stop making this class of errors"],
    ["user", "Can you have a worker set up Gemini on my computer free tier if that's available?"],
    ["user", "Why do we keep re-discovering that we need to batch and queue issues?"],
    ["user", "what happened to the build, did you restart it?"],
    ["user", "why do you keep losing the battle.net connection?"],
  ],
  lead: [
    ["user", "Read /h/.local/state/agents/handoffs/smashcraft-lead-brief.md and follow it."],
    ["user", "you keep asking; we need to land everything now"],
  ],
  oneshot: [["user", "Do NOT regenerate the token. We need to keep the scope."]],
};

function fixture() {
  const root = mkdtempSync(join(tmpdir(), "distill-"));
  const db = new Database(join(root, "index.db"));
  db.run("CREATE TABLE files(id INTEGER PRIMARY KEY, path TEXT, provider TEXT, session_id TEXT, agent TEXT)");
  db.run("CREATE TABLE msg(id INTEGER PRIMARY KEY, file_id INTEGER, line INTEGER, ts TEXT, role TEXT, tool TEXT, " +
    "body INTEGER, dup INTEGER NOT NULL DEFAULT 0)");
  db.run("CREATE TABLE body(id INTEGER PRIMARY KEY, text TEXT)");
  const start = Date.now() - 3 * 3_600_000;
  let n = 0;
  Object.entries(SESSIONS).forEach(([name, messages], f) => {
    db.run("INSERT INTO files VALUES (?,?,?,?,NULL)", [f + 1, `/t/${name}.jsonl`, "anthropic", `sess-${name}`]);
    messages.forEach(([role, text], line) => {
      n++;
      db.run("INSERT INTO body VALUES (?,?)", [n, text]);
      db.run("INSERT INTO msg VALUES (?,?,?,?,?,NULL,?,0)", [n, f + 1, line + 1, new Date(start + n * 60_000).toISOString(), role, n]);
    });
  });
  const handoffs = join(root, "handoffs");
  mkdirSync(handoffs);
  writeFileSync(join(handoffs, "galileo-lead-status.md"), "# status\nDECIDED 10:49 GAL44: grounding is a lead-local choice\n" +
    "APPROVED proxy 14:52Z: range lands\nnotes DECIDED inline is not a record\n");
  writeFileSync(join(handoffs, "brief.md"), "DECIDED 01:00 not a status file\n");
  const env = { ...process.env, NORTH_GROUNDING_CONVO_DB: join(root, "index.db"), NORTH_GROUNDING_STATE: join(root, "state"),
    NORTH_GROUNDING_HANDOFFS: handoffs } as Record<string, string>;
  delete env.NORTH_GROUNDING_LEDGER;
  const run = (...args: string[]) => {
    const p = Bun.spawnSync(["bun", TARGET, "run", "--no-refresh", ...args], { env });
    expect(p.exitCode).toBe(0);
    return JSON.parse(p.stdout.toString());
  };
  const rows = () => readFileSync(join(root, "state/ledger.jsonl"), "utf8").trim().split("\n").map((l) => JSON.parse(l));
  return { db, n, run, rows };
}

test("the ledger keeps Tom's decisions, links an override, takes lead DECIDED lines and adds only new rows", () => {
  const { db, n, run, rows } = fixture();
  const first = run("--since", "1d");
  const all = rows();
  for (const r of all) expect(Object.keys(r).sort()).toEqual(["at", "id", "kind", "quote", "source", "superseded_by", "topic"]);
  const tom = new Map(all.filter((r) => r.source.startsWith("tom convo:")).map((r) => [r.quote, r]));
  const prefer = "I prefer anthropic to stay the dominant way: Claude first on every account, codex only for tightly " +
    "specified work so we burn usage evenly.";
  const burn = "its fine if we burn codex today, we have the windfall of 2 anthropic accounts..";
  expect(Object.fromEntries([...tom].map(([q, r]) => [q, r.kind]))).toEqual({
    [burn]: "decision",
    "yes go ahead": "approval",
    [prefer]: "preference",
    "we need a new rule change that everyone has to respond within 2-5 minutes": "decision",
    "you keep answering questions without checking skills and transcripts": "correction",
    "can we update policy durably to stop making this class of errors": "decision",
    "Can you have a worker set up Gemini on my computer free tier if that's available?": "direction",
    "Why do we keep re-discovering that we need to batch and queue issues?": "correction",
    "why do you keep losing the battle.net connection?": "correction",
  });
  expect(tom.get("yes go ahead").topic).toContain("waygate");

  expect(tom.get(burn).superseded_by).toBe(tom.get(prefer).id);
  expect([...tom.values()].filter((r) => r.superseded_by).length).toBe(1);

  const others = all.filter((r) => !tom.has(r.quote)).map((r) => [r.source.split(" ")[0], r.kind, r.at.slice(11, 16)]).sort();
  expect(others.map(([b, k]) => [b, k])).toEqual([["galileo-lead", "approval"], ["galileo-lead", "decision"], ["proxy", "decision"]]);
  expect(others.find(([b, k]) => b === "galileo-lead" && k === "decision")![2]).toBe("10:49");

  db.run("INSERT INTO body VALUES (?,?)", [n + 1, "from now on only give me times in my local time"]);
  db.run("INSERT INTO msg VALUES (?,1,99,?,'user',NULL,?,0)", [n + 1, new Date().toISOString(), n + 1]);
  const second = run();
  expect([first.added, second.added, second.total]).toEqual([all.length, 1, all.length + 1]);
  expect(run().added).toBe(0);
});
