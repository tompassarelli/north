#!/usr/bin/env bun
// Eval scorer: needed refs match as substrings of the block; latency is the real CLI process, as the hook runs it.
import { copyFileSync, existsSync, mkdtempSync, readFileSync, rmSync, rmdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { LIMIT_CHARS, select, retrieve } from "./grounding";
import { dbPath, open, search, type Hit } from "./store";
import { convo } from "./sources/_convo";

type Item = { id: string; tier?: string; question: string; asked_at?: string; session?: string; convo_id?: number; needed: { ref: string }[] };
type MissRow = { at?: string; query: string; should_have_found: string[] };
type Row = { id: string; tier: string; needed: number; a: number; ab: number; missesA: string[]; missesAB: string[]; chars: number };
type Cut = "limit" | "floor" | "category cap" | "dedup" | "char budget" | "render";
type Miss = { id: string; tier: string; ref: string; class: "a" | "b" | "c"; rank?: number | null; cut?: Cut; doc?: string };
type Query = { id: string; tier: string; question: string; refs: string[]; block: string; asOf: number; prior?: string };

const CLI = join(import.meta.dir, "grounding.ts");
const CANDIDATES = 500;
const REAL_LIMIT = 80;
const FLOOR = 0.2;
const CUT_ORDER: Cut[] = ["limit", "floor", "dedup", "category cap", "render", "char budget"];

const asOfOf = (at: string | undefined, leaky: boolean) => (leaky || !at || Number.isNaN(Date.parse(at)) ? 0 : Date.parse(at) / 1000);

async function run(question: string, tier: string, asOf = 0, prior = ""): Promise<[string, number]> {
  const started = performance.now();
  const env = { ...process.env, NORTH_GROUNDING_AS_OF: asOf ? String(asOf) : "", NORTH_GROUNDING_PRIOR: prior };
  const proc = Bun.spawn([process.execPath, CLI, "--tier", tier, "--query", question], { stdout: "pipe", stderr: "ignore", env });
  const out = await new Response(proc.stdout).text();
  await proc.exited;
  return [out, (performance.now() - started) / 1000];
}

const pct = (values: number[], q: number) => {
  const s = [...values].sort((a, b) => a - b);
  return s[Math.min(s.length - 1, Math.round(q * (s.length - 1)))];
};

const readRows = <T>(path: string): T[] => readFileSync(path, "utf8").split("\n").filter((l) => l.trim()).map((l) => JSON.parse(l));
const docKey = (d: { source: string; id: string }) => `${d.source}\u0000${d.id}`;
const label = (d: { source: string; ref: string }) => `${d.source}:${d.ref}`;

function containing(refs: Set<string>, db: ReturnType<typeof open>): Map<string, Set<string>> {
  const found = new Map<string, Set<string>>([...refs].map((r) => [r, new Set()]));
  for (const d of db.query("select source, id, ref, title, text from docs").iterate() as Iterable<
    { source: string; id: string; ref: string; title: string; text: string }>) {
    const hay = `${d.ref}\n${d.title}\n${d.text}`;
    for (const [ref, keys] of found) if (hay.includes(ref)) keys.add(docKey(d));
  }
  return found;
}

function diagnose(queries: Query[], db: ReturnType<typeof open>, now: number): Miss[] {
  const missed = queries.map((q) => ({ q, misses: q.refs.filter((r) => !q.block.includes(r)) })).filter((x) => x.misses.length);
  const where = containing(new Set(missed.flatMap((x) => x.misses)), db);
  const bm25 = (asOf: number) => db.query(`select source, id from docs where docs match ?${asOf ? ` and not ((decays = 1 or source in ('transcripts','builds','ledger','commits','threads','github')) and at > ${asOf})` : ""} order by bm25(docs, 0, 0, 0, 4.0, 1.0, 0, 0)`);
  const out: Miss[] = [];
  for (const { q, misses } of missed) {
    const { hits: cands, words } = retrieve(q.question, db, now, CANDIDATES, q.asOf, q.prior);
    const { groups, rest } = select(cands.slice(0, REAL_LIMIT), q.question);
    const selected = new Set([...groups.values()].flat());
    const restSet = new Set(rest);
    const truncated = q.block.length >= LIMIT_CHARS - 1;
    let global: string[] | null = null;
    for (const ref of misses) {
      const keys = where.get(ref)!;
      if (!keys.size) {
        out.push({ id: q.id, tier: q.tier, ref, class: "a" });
        continue;
      }
      let best: { cut: Cut; hit: Hit } | null = null;
      cands.forEach((hit: Hit, rank: number) => {
        if (!keys.has(docKey(hit))) return;
        const cut: Cut = rank >= REAL_LIMIT ? "limit" : hit.cover < FLOOR ? "floor" : selected.has(hit) ? (truncated ? "char budget" : "render")
          : restSet.has(hit) ? "category cap" : "dedup";
        if (!best || CUT_ORDER.indexOf(cut) > CUT_ORDER.indexOf(best.cut)) best = { cut, hit };
      });
      if (best) {
        const { cut, hit } = best as { cut: Cut; hit: Hit };
        out.push({ id: q.id, tier: q.tier, ref, class: "c", cut, doc: label(hit) });
        continue;
      }
      if (!global) {
        const top = words.slice(0, 16).map((w) => `"${w.replaceAll('"', "")}"`).join(" OR ");
        global = words.length ? (bm25(q.asOf).values(top) as string[][]).map(([s, i]) => docKey({ source: s, id: i })) : [];
      }
      const at = global.findIndex((k) => keys.has(k));
      out.push({ id: q.id, tier: q.tier, ref, class: "b", rank: at >= 0 ? at + 1 : null });
    }
  }
  return out;
}

const counts = (misses: Miss[]) => {
  const total = { a: 0, b: 0, c: 0 };
  const cuts: Record<string, number> = {};
  for (const m of misses) {
    total[m.class]++;
    if (m.cut) cuts[m.cut] = (cuts[m.cut] ?? 0) + 1;
  }
  return { ...total, total: misses.length, cuts };
};

type Options = { withB: boolean; runs: number; holdoutPath?: string; leaky?: boolean; excludeSessions?: string[] };

function excludeSessionTree(sessions: string[]): { dir: string; removed: Record<string, number> } {
  const dir = mkdtempSync(join(tmpdir(), "grounding-score-"));
  copyFileSync(dbPath(), join(dir, "index.db"));
  const db = convo();
  const ids = db ? sessions.flatMap((s) => (db.query("select m.id from msg m join files f on f.id = m.file_id where f.session_id like ?").values(`${s}%`) as number[][]).map(([id]) => String(id))) : [];
  db?.close();
  const index = open(false, join(dir, "index.db"));
  const del = index.prepare("delete from docs where source = ? and id = ?");
  const removed: Record<string, number> = {};
  for (const source of ["transcripts", "builds"])
    for (const id of ids) removed[source] = (removed[source] ?? 0) + del.run(source, id).changes;
  index.close();
  return { dir, removed };
}

export async function score(path: string, o: Options) {
  const items: Item[] = readRows<Item>(path);
  const real = process.env.NORTH_GROUNDING_STATE;
  const excluded = o.excludeSessions?.length && !o.leaky ? excludeSessionTree(o.excludeSessions) : null;
  if (excluded) process.env.NORTH_GROUNDING_STATE = excluded.dir;
  try {
    return await scoreIn(items, path, o, excluded?.removed ?? null);
  } finally {
    if (excluded) {
      process.env.NORTH_GROUNDING_STATE = real;
      if (real === undefined) delete process.env.NORTH_GROUNDING_STATE;
      rmSync(join(excluded.dir, "index.db"), { force: true });
      rmdirSync(excluded.dir);
    }
  }
}

async function scoreIn(items: Item[], path: string, o: Options, removed: Record<string, number> | null) {
  const latencies: number[] = [];
  const rows: Row[] = [];
  const queries: Query[] = [];
  for (const item of items) {
    const refs = item.needed.map((n) => n.ref);
    const asOf = asOfOf(item.asked_at, !!o.leaky);
    let prior = "";
    if (item.session && item.convo_id) {
      const history = convo();
      try {
        const row = history?.query("select b.text from msg m join body b on b.id=m.body join files f on f.id=m.file_id join msg current on current.id=? where f.session_id like ? and m.file_id=current.file_id and m.line<current.line and m.role='assistant' and m.ts<=current.ts and m.dup=0 order by m.line desc limit 1")
          .get(item.convo_id, `${item.session}%`) as { text: string } | null;
        prior = row?.text.slice(0, 4000) ?? "";
      } finally { history?.close(); }
    }
    let blockA = "";
    for (let i = 0; i < Math.max(1, o.runs); i++) {
      const [out, s] = await run(item.question, "A", asOf, prior);
      blockA = out;
      latencies.push(s);
    }
    queries.push({ id: item.id, tier: item.tier ?? "A", question: item.question, refs, block: blockA, asOf, prior });
    const foundA = refs.filter((r) => blockA.includes(r));
    let foundAB = foundA;
    if (o.withB && foundA.length < refs.length) {
      const [blockB] = await run(item.question, "B", asOf, prior);
      foundAB = refs.filter((r) => blockA.includes(r) || blockB.includes(r));
    }
    rows.push({ id: item.id, tier: item.tier ?? "A", needed: refs.length, a: foundA.length, ab: foundAB.length,
      missesA: refs.filter((r) => !foundA.includes(r)), missesAB: refs.filter((r) => !foundAB.includes(r)), chars: blockA.length });
  }
  const holdFile = o.holdoutPath ?? join(dirname(path), "misses.jsonl");
  const hold = existsSync(holdFile) ? readRows<MissRow>(holdFile) : [];
  const holdQueries: Query[] = [];
  for (const [i, row] of hold.entries()) {
    const asOf = asOfOf(row.at, !!o.leaky);
    const [block] = await run(row.query, "A", asOf);
    holdQueries.push({ id: `holdout-${i}`, tier: "A", question: row.query, refs: row.should_have_found, block, asOf });
  }
  const db = open(true);
  const now = Date.now() / 1000;
  const misses = diagnose(queries, db, now);
  const holdMisses = diagnose(holdQueries, db, now);
  db.close();
  const sum = (rs: Row[], k: "a" | "ab" | "needed") => rs.reduce((s, r) => s + r[k], 0);
  const tierA = rows.filter((r) => r.tier === "A");
  const holdNeeded = holdQueries.reduce((s, q) => s + q.refs.length, 0);
  return {
    rows,
    misses,
    summary: {
      mode: o.leaky ? "leaky" : "as-of",
      excludedDocs: removed,
      questions: rows.length,
      tierA: { questions: tierA.length, found: sum(tierA, "a"), needed: sum(tierA, "needed") },
      recallTierARows: sum(tierA, "a") / Math.max(1, sum(tierA, "needed")),
      recallAAllRows: sum(rows, "a") / Math.max(1, sum(rows, "needed")),
      recallABAllRows: o.withB ? sum(rows, "ab") / Math.max(1, sum(rows, "needed")) : null,
      classesTierA: counts(misses.filter((m) => m.tier === "A")),
      classesAll: counts(misses),
      p50: pct(latencies, 0.5), p95: pct(latencies, 0.95), samples: latencies.length,
    },
    holdout: {
      file: holdFile, rows: hold.length, found: holdNeeded - holdMisses.length, needed: holdNeeded,
      recall: holdNeeded ? (holdNeeded - holdMisses.length) / holdNeeded : null,
      classes: counts(holdMisses), misses: holdMisses,
    },
  };
}

const fmt = (m: Miss) =>
  `  ${m.id} [${m.tier}] ${m.ref}: ${m.class}` +
  (m.class === "b" ? ` best-rank ${m.rank ?? "no query term hits it"}` : m.class === "c" ? ` ${m.cut} via ${m.doc}` : " no indexed doc contains it");
const cls = (c: ReturnType<typeof counts>) => `a ${c.a} / b ${c.b} / c ${c.c} = ${c.total} misses; cuts ${JSON.stringify(c.cuts)}`;

if (import.meta.main) {
  const args = process.argv.slice(2);
  const valued = new Set(["--runs", "--holdout", "--exclude-session"]);
  const path = args.find((a, i) => !a.startsWith("--") && !valued.has(args[i - 1]));
  if (!path) {
    console.error("usage: score.ts EVAL.jsonl [--no-b] [--runs N] [--holdout MISSES.jsonl] [--leaky] [--exclude-session ID]... [--json]");
    process.exit(2);
  }
  const value = (name: string) => (args.includes(name) ? args[args.indexOf(name) + 1] : undefined);
  const result = await score(path, {
    withB: !args.includes("--no-b"), runs: value("--runs") ? Number(value("--runs")) : 1, holdoutPath: value("--holdout"),
    leaky: args.includes("--leaky"), excludeSessions: args.flatMap((a, i) => (a === "--exclude-session" ? [args[i + 1]] : [])),
  });
  if (args.includes("--json")) console.log(JSON.stringify(result, null, 1));
  else {
    for (const r of result.rows)
      console.log(`${r.id} [${r.tier}]: A ${r.a}/${r.needed} A+B ${r.ab}/${r.needed} chars ${r.chars} misses A ${JSON.stringify(r.missesA)} A+B ${JSON.stringify(r.missesAB)}`);
    console.log("misses (tier A retrieval):");
    for (const m of result.misses) console.log(fmt(m));
    const s = result.summary;
    console.log(`mode ${s.mode}; session-tree docs removed from the index copy: ${JSON.stringify(s.excludedDocs)}`);
    console.log(`tier-A recall: ${s.tierA.found}/${s.tierA.needed} = ${s.recallTierARows.toFixed(3)} over ${s.tierA.questions} tier-A questions`);
    console.log(`miss classes, tier-A questions: ${cls(s.classesTierA)}`);
    console.log(`miss classes, all questions: ${cls(s.classesAll)}`);
    const h = result.holdout;
    for (const m of h.misses) console.log(fmt(m));
    console.log(`holdout recall: ${h.found}/${h.needed}${h.recall === null ? "" : ` = ${h.recall.toFixed(3)}`} over ${h.rows} rows of ${h.file} scored on their own query text; ${cls(h.classes)}`);
    console.log(JSON.stringify({ ...s, classesTierA: undefined, classesAll: undefined }));
  }
}
