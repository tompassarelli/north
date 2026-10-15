// Source contract: sources/<name>.ts exports iterDocs(since) plus optional COMPLETE, RETAIN_DAYS, REFRESH_SECONDS.
import { Database } from "bun:sqlite";
import { closeSync, existsSync, mkdirSync, openSync, readdirSync, readFileSync, rmSync, statSync, utimesSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

export type Doc = { source: string; id: string; ref: string; title: string; text: string; at: number; decays: boolean };
const SECRET = new RegExp(
  String.raw`\b(sk-[A-Za-z0-9_-]{16,}|gh[pousr]_[A-Za-z0-9]{20,}|xox[abp]-[A-Za-z0-9-]{10,}|AKIA[A-Z0-9]{16}` +
  String.raw`|(?![0-9a-f]{40,}\b)(?=[A-Za-z0-9+_-]*\d)(?=[A-Za-z0-9+_-]*[A-Za-z])[A-Za-z0-9+_-]{40,})\b` +
  String.raw`|\b(password|passwd|token|secret)\s*[:=]\s*\S+`, "gi");
export const redact = (text: string) => text.replace(SECRET, "[redacted]");
export type Source = {
  iterDocs(since: number | null): Iterable<Doc> | AsyncIterable<Doc>;
  COMPLETE?: boolean;
  RETAIN_DAYS?: number;
  REFRESH_SECONDS?: number;
};
export type Hit = Doc & { cover: number; score: number; matched: string[] };

export const HERE = import.meta.dir;
export const SOURCES = process.env.NORTH_GROUNDING_SOURCES ?? join(HERE, "sources");
export const HOME = homedir();
const DEFAULT_REFRESH = 900;
const TEXT_LIMIT = 4000;
const HALF_LIFE_DAYS = 7;

export const stateDir = () => process.env.NORTH_GROUNDING_STATE ?? join(HOME, ".local/state/north/grounding");
export const dbPath = () => join(stateDir(), "index.db");
export const mainRunner = () => {
  const main = join(process.env.NORTH_GROUNDING_NORTH ?? join(HOME, "code/north/main"), "harness/grounding/grounding.ts");
  return existsSync(main) ? main : join(HERE, "grounding.ts");
};

const SCHEMA = `
CREATE VIRTUAL TABLE IF NOT EXISTS docs USING fts5(
  source UNINDEXED, id UNINDEXED, ref UNINDEXED, title, text, at UNINDEXED, decays UNINDEXED,
  tokenize='porter unicode61 remove_diacritics 2');
CREATE TABLE IF NOT EXISTS watermarks(source TEXT PRIMARY KEY, at REAL, refreshed REAL, rows INTEGER);`;

export function open(readonly = false, path = dbPath()): Database {
  if (readonly) {
    if (!existsSync(path)) throw new Error(`no index at ${path}`);
    const db = new Database(path, { readonly: true });
    return db;
  }
  mkdirSync(stateDir(), { recursive: true });
  const db = new Database(path, { create: true });
  db.exec("PRAGMA journal_mode=DELETE; PRAGMA busy_timeout=30000;");
  db.exec(SCHEMA);
  return db;
}

export async function sourceModules(dir = SOURCES): Promise<Map<string, Source>> {
  const out = new Map<string, Source>();
  for (const file of readdirSync(dir).sort()) {
    if (!file.endsWith(".ts") || file.includes(".test.") || file.startsWith("_")) continue;
    try {
      const mod = (await import(join(dir, file))) as Source;
      if (typeof mod.iterDocs === "function") out.set(file.slice(0, -3), mod);
    } catch (error) {
      console.error(`grounding: source ${file} failed to load: ${error}`);
    }
  }
  return out;
}

function row(doc: Doc, name: string): (string | number)[] {
  for (const key of ["source", "id", "ref", "title", "text", "at", "decays"] as const)
    if (!(key in doc)) throw new Error(`source ${name} doc lacks ${key}`);
  return [String(doc.source), String(doc.id), String(doc.ref), String(doc.title),
    String(doc.text).slice(0, TEXT_LIMIT), Number(doc.at) || 0, doc.decays ? 1 : 0];
}

function lock(): number | null {
  mkdirSync(stateDir(), { recursive: true });
  const path = join(stateDir(), "refresh.lock");
  try {
    const fd = openSync(path, "wx");
    writeFileSync(fd, String(process.pid));
    return fd;
  } catch {
    try {
      const pid = Number(readFileSync(path, "utf8"));
      if (pid && pid !== process.pid) process.kill(pid, 0);
      else throw new Error("stale");
      return null;
    } catch {
      rmSync(path, { force: true });
      return lock();
    }
  }
}

export async function refresh(only: string[] | null = null, force = false, wait = true, dir = SOURCES): Promise<Record<string, number | string | boolean>> {
  let fd = lock();
  while (fd === null && wait) {
    await Bun.sleep(500);
    fd = lock();
  }
  if (fd === null) return { busy: true };
  try {
    const db = open();
    const modules = await sourceModules(dir);
    const seen = new Map(db.query("select source, refreshed from watermarks").values().map(([s, r]) => [String(s), Number(r)]));
    const counts: Record<string, number | string> = {};
    const kept = db.query("select rows from watermarks where source=?");
    for (const [name, mod] of modules) {
      if (only && !only.includes(name)) continue;
      const started = Date.now() / 1000;
      if (!force && started - (seen.get(name) ?? 0) <= (mod.REFRESH_SECONDS ?? DEFAULT_REFRESH)) {
        counts[name] = `fresh ${(kept.get(name) as { rows: number } | null)?.rows ?? 0}`;
        continue;
      }
      const mark = db.query("select at from watermarks where source=?").get(name) as { at: number } | null;
      const since = mod.COMPLETE || force || !mark ? null : mark.at;
      const rows: (string | number)[][] = [];
      try {
        for await (const doc of mod.iterDocs(since)) rows.push(row(doc, name));
      } catch (error) {
        console.error(`grounding: source ${name} failed: ${error}`);
        continue;
      }
      const del = db.prepare("delete from docs where source=? and id=?");
      const ins = db.prepare("insert into docs(source,id,ref,title,text,at,decays) values (?,?,?,?,?,?,?)");
      db.transaction(() => {
        if (since === null) db.run("delete from docs where source=?", [name]);
        else for (const r of rows) del.run(r[0], r[1]);
        for (const [source, id, ref, title, text, at, decays] of rows) ins.run(source, id, ref, redact(String(title)), redact(String(text)), at, decays);
        if (mod.RETAIN_DAYS) db.run("delete from docs where source=? and at < ?", [name, started - mod.RETAIN_DAYS * 86400]);
        const newest = Math.max(mark?.at ?? 0, ...rows.map((r) => Number(r[5])));
        const total = (db.query("select count(*) n from docs where source=?").get(name) as { n: number }).n;
        db.run("insert or replace into watermarks values (?,?,?,?)", [name, newest, started, total]);
      })();
      counts[name] = rows.length;
    }
    db.close();
    return counts;
  } finally {
    closeSync(fd);
    rmSync(join(stateDir(), "refresh.lock"), { force: true });
  }
}

export function spawnRefreshIfStale(): void {
  try {
    const marker = join(stateDir(), "refresh.started");
    if (existsSync(marker) && Date.now() - statSync(marker).mtimeMs < 120_000) return;
    if (existsSync(dbPath())) {
      const db = open(true);
      const oldest = (db.query("select min(refreshed) m from watermarks").get() as { m: number | null }).m ?? 0;
      db.close();
      if (Date.now() / 1000 - oldest < 300) return;
    }
    mkdirSync(stateDir(), { recursive: true });
    writeFileSync(marker, "");
    utimesSync(marker, new Date(), new Date());
    const log = openSync(join(stateDir(), "refresh.log"), "a");
    const runner = mainRunner();
    const distill = join(runner, "..", "distill.ts");
    const ledger = join(stateDir(), "ledger.jsonl");
    const ledgerStale = !existsSync(ledger) || Date.now() - statSync(ledger).mtimeMs > 900_000;
    const steps = [...(ledgerStale && existsSync(distill) ? [`"${process.execPath}" "${distill}" run --no-refresh --if-idle`] : []),
      `"${process.execPath}" "${runner}" refresh --if-idle`];
    const child = Bun.spawn(["nice", "-n", "19", "ionice", "-c", "3", "bash", "-c", steps.join("; ")], {
      stdin: "ignore", stdout: log, stderr: log, detached: true,
    } as Parameters<typeof Bun.spawn>[1]);
    child.unref();
  } catch {
    return;
  }
}

const STOP = new Set(`
a about above after again all also am an and any are aren't as at be because been before being below between
both but by can can't could couldn't did didn't do does doesn't doing don don't down during each few for from further
had hadn't has hasn't have haven't having he her here hers herself him himself his how i i'm if in into is isn't
it it's its itself just let's like me more most my myself no nor not now of off on once only or other ought our
ours ourselves out over own same she should shouldn shouldn't so some such than that that's the their theirs them
themselves then there there's these they this those through to too under until up very was wasn't we we're
were weren't what what's when where which while who whom why why's with won't would wouldn't you you're your
yours yourself yourselves ok okay yes yeah yep no nope thanks thank please sure right well um uh oh hey hi
mean know think want need get got going go gonna kind sort thing things really actually basically just like
lot also still even ever way make made use used using one two see say said tell told ask asked didn
im dont didnt doesnt isnt youre thats whats ive weve theyre cant wont shouldnt wouldnt couldnt
point question something anything everything someone stuff guess maybe probably pretty bit
already again thought tonight today yesterday earlier recently idea fucking honestly literally`.split(/\s+/));
const TOKEN = /[a-z0-9][a-z0-9_.\-/#:]*[a-z0-9]|[a-z0-9]/g;
const DISTINCT = /[0-9]|[:_/#.-]/;
const CLAUSE = /(?<=[.?!;:\n])\s+|\s+(?:but|and then|so that|because|although|which|however)\s+/i;
const RRF_K = 40;
const HISTORY = ["transcripts", "builds", "ledger", "commits", "threads", "github"];
const FAMILIES = ["catalog", "config", "handoffs", "projects", "transcripts", "ledger", "commits/builds", "issues"];
const FAMILY = `case
  when source='policy' and title like 'handoff %' then 'handoffs'
  when source='settings' or (source='policy' and ref like '%.toml' and title not like 'rule %' and title not like 'hook %') then 'config'
  when source='policy' then 'catalog'
  when source in ('commits', 'builds') then 'commits/builds'
  when source in ('threads', 'github') then 'issues'
  else source end`;
const GENERIC_NAME = new Set(["skill", "main", "index", "notes", "readme", "agents", "test", "lib", "references", "brief", "status"]);

export function rawTerms(text: string): [string, boolean][] {
  const out: [string, boolean][] = [];
  const seen = new Set<string>();
  for (const raw of text.toLowerCase().match(TOKEN) ?? []) {
    const distinct = DISTINCT.test(raw) && !/^\d+$/.test(raw);
    for (const word of raw.split(/[^a-z0-9]+/)) {
      if (!word || seen.has(word) || STOP.has(word) || (word.length < 2 && !/^\d+$/.test(word))) continue;
      seen.add(word);
      out.push([word, distinct]);
    }
  }
  return out;
}

export const terms = (text: string) => rawTerms(text).map(([w]) => w);
export const match = (term: string) => `"${term.replaceAll('"', "")}"`;

export function clauses(text: string): string[] {
  const merged: string[] = [];
  for (const part of text.split(CLAUSE)) {
    if (!part || part.trim().split(/\s+/).length < 2) continue;
    if (merged.length && merged[merged.length - 1].split(/\s+/).length < 6) merged[merged.length - 1] += " " + part;
    else merged.push(part);
  }
  return merged.slice(0, 10);
}

function distance(a: string, b: string): number {
  let row = Array.from({ length: b.length + 1 }, (_, i) => i);
  for (let i = 0; i < a.length; i++) {
    const next = [i + 1];
    for (let j = 0; j < b.length; j++) next.push(Math.min(next[j] + 1, row[j + 1] + 1, row[j] + Number(a[i] !== b[j])));
    row = next;
  }
  return row[b.length];
}

function vocabulary(db: Database, words: string[]) {
  db.exec(`CREATE VIRTUAL TABLE IF NOT EXISTS temp.grounding_vocab USING fts5vocab(main, docs, row);
    CREATE VIRTUAL TABLE IF NOT EXISTS temp.grounding_instances USING fts5vocab(main, docs, instance);
    CREATE VIRTUAL TABLE IF NOT EXISTS temp.grounding_tokens USING fts5(text, tokenize='porter unicode61 remove_diacritics 2');
    CREATE VIRTUAL TABLE IF NOT EXISTS temp.grounding_stems USING fts5vocab(temp, grounding_tokens, instance);
    DELETE FROM temp.grounding_tokens;`);
  db.run("INSERT INTO temp.grounding_tokens(text) VALUES (?)", [words.join(" ")]);
  const stems = new Map((db.query("select offset, term from temp.grounding_stems").values() as [number, string][]).map(([i, stem]) => [words[i], stem]));
  const stats = (tokens: string[]) => new Map((db.query(`select term, doc from temp.grounding_vocab where term in (${tokens.map(() => "?").join(",")})`)
    .values(...tokens) as [string, number][]).map(([term, df]) => [term, df]));
  let df = stats([...stems.values()]);
  const repaired = new Map<string, string>();
  for (const word of words) {
    const stem = stems.get(word)!;
    if (df.has(stem) || word.length < 6) continue;
    const candidates = db.query("select term, doc from temp.grounding_vocab where length(term) between ? and ? and (substr(term,1,2)=? or substr(term,-2)=?) order by doc desc limit 2000")
      .values(stem.length - 2, stem.length + 2, stem.slice(0, 2), stem.slice(-2)) as [string, number][];
    let best = 3;
    for (const [term] of candidates) {
      const edits = distance(stem, term);
      if (edits < best) { best = edits; repaired.set(word, term); }
    }
  }
  for (const [word, term] of repaired) stems.set(word, term);
  df = stats([...stems.values()]);
  const tokens = [...new Set(stems.values())];
  const domain = new Map((db.query(`select v.term, count(distinct case when d.source!='projects' then v.doc end), count(distinct case when d.source='projects' then v.doc end)
    from temp.grounding_instances v join docs d on d.rowid=v.doc
    where v.term in (${tokens.map(() => "?").join(",")}) and (d.source in ('settings','projects') or (d.source='policy' and d.title not like 'handoff %')) group by v.term`)
    .values(...tokens) as [string, number, number][]).map(([term, catalog, projects]) => [term, { catalog, projects }]));
  return { stems, df, domain, repaired };
}

export function search(query: string, db: Database, now = Date.now() / 1000, limit = 80, asOf = Number(process.env.NORTH_GROUNDING_AS_OF) || 0): { hits: Hit[]; words: string[] } {
  if (asOf) now = Math.min(now, asOf);
  const seen = asOf ? ` and not ((decays = 1 or source in (${HISTORY.map((h) => `'${h}'`).join(",")})) and at > ${asOf})` : "";
  const candidatesTerms = rawTerms(query).slice(0, 60);
  if (!candidatesTerms.length) return { hits: [], words: [] };
  const total = (db.query("select count(*) n from docs").get() as { n: number }).n || 1;
  const vocab = vocabulary(db, candidatesTerms.map(([word]) => word));
  const long = query.trim().split(/\s+/).length > 40;
  const weight = new Map<string, number>();
  for (const [raw, distinct] of candidatesTerms) {
    const stem = vocab.stems.get(raw)!;
    const word = vocab.repaired.get(raw) ?? raw;
    const df = vocab.df.get(stem) ?? 0;
    if (!df || (total > 1000 && df > total * 0.2)) continue;
    const { catalog, projects } = vocab.domain.get(stem) ?? { catalog: 0, projects: 0 };
    if ((word.length === 2 && !catalog) || (long && !catalog && !projects)) continue;
    let idf = Math.log(1 + total / df);
    if (catalog + projects < 3 && !distinct) idf *= 0.5;
    weight.set(word, idf * (distinct ? 1.5 : 1) * (1 + Math.log1p(catalog) / Math.log1p(df)));
  }
  if (!weight.size) return { hits: [], words: [] };
  const queries: string[][] = [[...weight.keys()]];
  for (const clause of clauses(query)) {
    const words = rawTerms(clause).map(([w]) => vocab.repaired.get(w) ?? w).filter((w) => weight.has(w));
    if (words.length >= 2 && words.join(" ") !== queries[0].join(" ")) queries.push(words);
  }
  const fused = new Map<number, number>();
  const families = new Map<number, string>();
  const ranked = db.query(`select rowid, ${FAMILY} family from docs where docs match ? and rank match 'bm25(0, 0, 0, 4.0, 1.0, 0, 0)'${seen} order by rank`);
  for (const words of queries) {
    const top = [...words].sort((a, b) => weight.get(b)! - weight.get(a)!).slice(0, 16);
    const query = top.map(match).join(" OR ");
    const counts = new Map<string, number>();
    for (const row of ranked.all(query) as { rowid: number; family: string }[]) {
      const rank = counts.get(row.family) ?? 0;
      if (rank >= 80 || !FAMILIES.includes(row.family)) continue;
      counts.set(row.family, rank + 1);
      families.set(row.rowid, row.family);
      fused.set(row.rowid, (fused.get(row.rowid) ?? 0) + 1 / (RRF_K + rank));
      if (FAMILIES.every((family) => counts.get(family) === 80)) break;
    }
  }
  const names = db.query(`select rowid, ref, ${FAMILY} from docs where docs match ? and source in ('policy', 'projects')${seen} limit 400`);
  const qwords = [...weight.keys()];
  for (const [rowid, ref, family] of names.values(`title : (${qwords.map(match).join(" OR ")})`) as [number, string, string][]) {
    const base = ref.split("#")[0].split("/").pop()!.replace(/\.[a-z]+$/, "").toLowerCase();
    const parts = base.split(/[^a-z0-9]+/).filter((p) => p.length > 2 && !GENERIC_NAME.has(p));
    const name = parts.length ? parts : ref.split("/").slice(-2, -1)[0]?.toLowerCase().split(/[^a-z0-9]+/) ?? [];
    if (name.length && name.every((p) => qwords.some((w) => p.startsWith(w) || w.startsWith(p)))) {
      families.set(rowid, family);
      fused.set(rowid, (fused.get(rowid) ?? 0) + 2 / RRF_K);
    }
  }
  const rankedIds = [...fused.keys()].sort((a, b) => fused.get(b)! - fused.get(a)!);
  const byFamily = FAMILIES.map((family) => rankedIds.filter((id) => families.get(id) === family));
  const candidates: number[] = [];
  for (let rank = 0; candidates.length < 500; rank++) {
    const next = byFamily.flatMap((ids) => ids[rank] === undefined ? [] : [ids[rank]]);
    if (!next.length) break;
    candidates.push(...next.slice(0, 500 - candidates.length));
  }
  if (!candidates.length) return { hits: [], words: [...weight.keys()] };
  const marks = candidates.map(() => "?").join(",");
  const topFused = Math.max(...fused.values());
  const out: Hit[] = [];
  const rows = db.query(`select rowid, source, id, ref, title, text, at, decays from docs where rowid in (${marks})`).all(...candidates) as
    { rowid: number; source: string; id: string; ref: string; title: string; text: string; at: number; decays: number }[];
  const stems = [...weight.keys()].map((w) => [w, w.length > 5 ? w.slice(0, w.length - 2) : w] as const);
  for (const r of rows) {
    const hay = `${r.title}\n${r.text}`.toLowerCase();
    const matched = new Set(stems.filter(([, stem]) => hay.includes(stem)).map(([w]) => w));
    let cover = 0;
    for (const words of queries) {
      const mass = words.reduce((s, w) => s + weight.get(w)!, 0);
      cover = Math.max(cover, words.reduce((s, w) => s + (matched.has(w) ? weight.get(w)! : 0), 0) / mass);
    }
    const age = Math.max(0, (now - Number(r.at || 0)) / 86400);
    const decay = Number(r.decays) ? 0.5 ** (age / HALF_LIFE_DAYS) : 1;
    out.push({ source: r.source, id: r.id, ref: r.ref, title: r.title, text: r.text, at: Number(r.at || 0),
      decays: Boolean(Number(r.decays)), cover, score: (fused.get(r.rowid)! / topFused + cover) * (0.5 + 0.5 * decay),
      matched: [...matched].sort() });
  }
  out.sort((a, b) => b.score - a.score);
  const hitFamilies = new Map(rows.map((r) => [`${r.source}\0${r.id}`, families.get(r.rowid)!]));
  const retained = new Set<Hit>();
  const share = Math.floor(limit / FAMILIES.length);
  for (const family of FAMILIES) {
    for (const hit of out.filter((hit) => hitFamilies.get(`${hit.source}\0${hit.id}`) === family).slice(0, share)) retained.add(hit);
  }
  for (const hit of out) {
    if (retained.size >= limit) break;
    retained.add(hit);
  }
  return { hits: out.filter((hit) => retained.has(hit)), words: [...weight.keys()].sort((a, b) => weight.get(b)! - weight.get(a)!) };
}
