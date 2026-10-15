import { Database } from "bun:sqlite";
import { expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { eventsDoc, hookDocs } from "./settings.ts";
import { iterDocs as githubDocs, LIMIT } from "./github.ts";
import { epoch, iterDocs as threadsDocs } from "./threads.ts";
import { OWNER, REPOS } from "./_threads.ts";

const KEYS = ["at", "decays", "id", "ref", "source", "text", "title"];

function withLedger(check: (path: string) => void) {
  const dir = mkdtempSync(join(tmpdir(), "grounding-threads-"));
  const path = join(dir, "threads.db");
  const db = new Database(path);
  db.exec(`CREATE TABLE events(seq INTEGER PRIMARY KEY, ts TEXT, item TEXT, kind TEXT, actor TEXT, data TEXT);
    CREATE TABLE runs(item TEXT, ended TEXT, outcome TEXT, tier TEXT, report TEXT, note TEXT);
    CREATE TABLE gh_cache(repo TEXT, fetched REAL, issues TEXT);
    CREATE TABLE blocks(item TEXT, reason TEXT);
    CREATE TABLE claims(item TEXT, owner TEXT);
    CREATE TABLE items(ns TEXT, number INTEGER, title TEXT, body TEXT, state TEXT, created TEXT, closed TEXT, due TEXT);
    INSERT INTO items VALUES ('ops', 1, 'Codex routing trial', 'route overflow to codex', 'closed', '2026-10-13T12:00:00Z', '2026-10-13T13:00:00Z', NULL),
      ('ops', 2, 'Laptop availability', '', 'open', '2026-10-14T12:00:00Z', NULL, NULL);
    INSERT INTO events(ts, item, kind, actor, data) VALUES ('2026-10-14T13:00:00Z', 'ops#2', 'note', 'tick', '{"text": "laptop draining"}'),
      ('2026-10-12T10:00:00Z', 'north#7', 'claim', 'lead', '{}'), ('2026-10-12T10:05:00Z', 'north#9', 'claim', 'lead', '{}');
    INSERT INTO runs VALUES ('north#7', '2026-10-14T09:00:00.000Z', 'done', 'opus', 'Done: grant age fixed', NULL);
    INSERT INTO gh_cache VALUES ('north', 0, '{"7": {"title": "Grant age telemetry", "blockers": []}}');`);
  db.close();
  try {
    check(path);
  } finally {
    rmSync(dir, { recursive: true });
  }
}

test("threads: native items and issue threads become documents and since returns only later updates", () => {
  withLedger((path) => {
    const docs = Object.fromEntries([...threadsDocs(null, path)].map((d) => [d.id, d]));
    expect(Object.keys(docs).sort()).toEqual(["north#7", "north#9", "ops#1", "ops#2"]);
    for (const d of Object.values(docs)) expect(Object.keys(d).sort()).toEqual(KEYS);
    expect(docs["ops#1"].ref).toBe("threads ops#1");
    expect(docs["ops#1"].decays).toBe(true);
    expect(docs["ops#2"].decays).toBe(false);
    expect(docs["ops#2"].text).toContain("laptop draining");
    expect(docs["north#7"].title).toBe("Grant age telemetry");
    expect(docs["north#7"].text).toContain("Done: grant age fixed");
    expect(docs["north#7"].decays).toBe(false);
    expect(docs["north#9"].decays).toBe(true);
    const later = [...threadsDocs(epoch("2026-10-14T08:00:00Z"), path)].map((d) => d.id).sort();
    expect(later).toEqual(["north#7", "ops#2"]);
  });
});

test("threads: native items stay hidden where threads hides them", () => {
  withLedger((path) => {
    process.env.THREADS_NO_NATIVE = "1";
    try {
      expect([...threadsDocs(null, path)].map((d) => d.id).sort()).toEqual(["north#7", "north#9"]);
    } finally {
      delete process.env.THREADS_NO_NATIVE;
    }
  });
});

const node = (number: number, state: string, updatedAt: string, labels: string[] = []) => ({
  number, title: `Issue ${number}`, body: "body ".repeat(600), state, updatedAt, labels: { nodes: labels.map((name) => ({ name })) },
});

test("github: every tracked repo pages together in one query per page, incrementally, with trimmed text", async () => {
  const pages: Record<string, unknown> = {
    "": { pageInfo: { hasNextPage: true, endCursor: "c1" }, nodes: [node(1, "CLOSED", "2026-10-14T00:00:00Z", ["priority:now"])] },
    c1: { pageInfo: { hasNextPage: false, endCursor: null }, nodes: [node(2, "OPEN", "2026-10-15T00:00:00Z")] },
  };
  const calls: string[][] = [];
  const gh = async (args: string[]) => {
    calls.push(args);
    const fields = Object.fromEntries(args.filter((a) => a.includes("=") && !a.startsWith("query=")).map((a) => a.split("=", 2)));
    const repos = args.find((a) => a.startsWith("query="))!.split("repository(").length - 1;
    return JSON.stringify({ data: Object.fromEntries([...Array(repos).keys()].map((i) => [`r${i}`, { issues: pages[fields[`c${i}`] ?? ""] }])) });
  };
  const collect = async (since: number) => { const out = []; for await (const d of githubDocs(since, gh)) out.push(d); return out; };
  const docs = await collect(epoch("2026-10-13T00:00:00Z"));
  expect(calls.length).toBe(2);
  expect(docs.length).toBe(2 * REPOS.length);
  expect(calls.every((c) => c.includes("since=2026-10-13T00:00:00Z"))).toBe(true);
  expect(Object.keys(docs[0]).sort()).toEqual(KEYS);
  expect(docs[0].ref).toBe(`github ${OWNER}/${REPOS[0]}#1`);
  expect(docs[0].decays).toBe(true);
  expect(docs.at(-1)!.decays).toBe(false);
  expect(docs[0].text).toContain("labels: priority:now");
  expect(Math.max(...docs.map((d) => d.text.length))).toBeLessThanOrEqual(LIMIT);
  const later = await collect(epoch("2026-10-14T00:00:00Z"));
  expect(new Set(later.map((d) => d.id.split("#")[1]))).toEqual(new Set(["2"]));
});

test("commits: a commit landed inside a merged lane is indexed once with its subject and paths", async () => {
  const dir = mkdtempSync(join(tmpdir(), "grounding-commits-"));
  const run = (cwd: string, ...args: string[]) => {
    const out = Bun.spawnSync(["git", "-C", cwd, "-c", "user.name=t", "-c", "user.email=t@t", ...args], { stdout: "pipe", stderr: "pipe" });
    if (!out.success) throw new Error(out.stderr.toString());
    return out.stdout.toString().trim();
  };
  try {
    const main = join(dir, "proj", "main");
    mkdirSync(main, { recursive: true });
    run(main, "init", "-q", "-b", "main");
    run(main, "remote", "add", "origin", "https://github.com/tompassarelli/proj");
    writeFileSync(join(main, "a.txt"), "a");
    run(main, "add", "a.txt");
    run(main, "commit", "-q", "-m", "first on main");
    run(main, "checkout", "-q", "-b", "lane");
    writeFileSync(join(main, "bin-agent-contain"), "x");
    run(main, "add", "bin-agent-contain");
    run(main, "commit", "-q", "-m", "inside the lane");
    run(main, "checkout", "-q", "main");
    writeFileSync(join(main, "b.txt"), "b");
    run(main, "add", "b.txt");
    run(main, "commit", "-q", "-m", "diverge on main");
    run(main, "merge", "-q", "--no-ff", "-m", "merge lane", "lane");
    const lane = run(main, "rev-parse", "lane");
    const script = `import { iterDocs } from ${JSON.stringify(join(import.meta.dir, "commits.ts"))}; console.log(JSON.stringify([...iterDocs(null)]));`;
    const out = Bun.spawnSync([process.execPath, "-e", script], { stdout: "pipe", stderr: "pipe", env: { ...process.env, NORTH_GROUNDING_CODE: dir } });
    const docs = JSON.parse(out.stdout.toString()) as { id: string; ref: string; title: string; text: string }[];
    const doc = docs.find((d) => d.id === `proj@${lane}`)!;
    expect(doc.ref).toBe(`proj@${lane.slice(0, 8)}`);
    expect(doc.title).toContain("inside the lane");
    expect(doc.text).toContain("bin-agent-contain");
    expect(new Set(docs.map((d) => d.id)).size).toBe(docs.length);
    expect(docs.length).toBe(4);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("settings: every registered hook becomes one document carrying its event, matcher, file name and header comment", () => {
  const dir = mkdtempSync(join(tmpdir(), "grounding-settings-"));
  try {
    const hook = join(dir, "block-thing.sh");
    writeFileSync(hook, "#!/usr/bin/env bash\n# block-thing.sh: denies the thing\n# before it happens.\nset -e\n# not header\n");
    const binary = join(dir, "compiled");
    writeFileSync(binary, Buffer.from([0x7f, 0x45, 0, 0x4c]));
    const settings = join(dir, "settings.json");
    const entry = (command: string) => ({ type: "command", command });
    writeFileSync(settings, JSON.stringify({ hooks: {
      PreToolUse: [{ matcher: "Bash", hooks: [entry(`/usr/bin/env A=1 bash ${hook}`), entry(binary)] }],
      SessionStart: [{ hooks: [entry(`bash ${hook}`)] }],
    } }));
    const docs = [...hookDocs(settings, 5)];
    expect(docs.length).toBe(3);
    for (const d of docs) expect(Object.keys(d).sort()).toEqual(KEYS);
    const pre = docs.find((d) => d.id === "hook:PreToolUse:Bash:block-thing.sh")!;
    expect(pre.title).toContain("PreToolUse");
    expect(pre.text).toContain("matcher Bash");
    expect(pre.text).toContain("denies the thing\nbefore it happens.");
    expect(pre.text).not.toContain("not header");
    expect(docs.find((d) => d.id === "hook:SessionStart::block-thing.sh")!.text).toContain("every tool");
    expect(docs.find((d) => d.id.endsWith(":compiled"))!.text).not.toContain("\0");
  } finally {
    rmSync(dir, { recursive: true });
  }
});

test("settings: the events DB document names the ledger path and the event kinds read from durable_events.py without row data", () => {
  const doc = eventsDoc(join(import.meta.dir, "../../lib/durable_events.py"), 5)!;
  expect(doc.ref).toContain("threads.db");
  expect(doc.text).toContain("events.db");
  expect(doc.text).toContain("LAND-READY");
  expect(Object.keys(doc).sort()).toEqual(KEYS);
});
