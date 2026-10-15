#!/usr/bin/env bun

import { Database } from "bun:sqlite";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readdirSync, readFileSync, renameSync, rmdirSync, statSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { stateDir } from "./store";

const USAGE = `distill: distill Tom's decisions into a long-term ledger that does not decay.

Each run reads what is new since its watermark and appends one JSON row per record to
~/.local/state/north/grounding/ledger.jsonl:

  - Tom's own typed messages in top-level sessions he started (a session whose first prompt is
    an agent brief, or that has a single prompt, is an agent's; brief-shaped prompts and
    questions about what already happened inside Tom's sessions are skipped; requests count as
    directions), kept when deterministic rules read them as a
    decision, direction, preference, correction or approval;
  - DECIDED and APPROVED lines the proxy wrote in Tom's sessions;
  - DECIDED and APPROVED lines in handoff status files.

Sources are convo's SQLite index (read-only) and *status*.md files under the handoffs directory.
Extraction is rules only; no model is called. A newer Tom correction, decision or preference
with an override marker supersedes the latest older Tom decision, direction or preference from
the three days before it that names at least three of the same specific subjects; both rows
are kept.

Each row is {id, at, kind, topic, quote, source, superseded_by}; source starts with who said it
(\`tom\`, \`proxy\` or the status file's lead) and names the convo message and session or the file
and line.

  distill.ts run [--since 14d] [--no-refresh]
  distill.ts status
`;

const HOME = homedir();
const env = process.env;
const DB = env.NORTH_GROUNDING_CONVO_DB ?? join(HOME, ".local/state/convo/index-v4.db");
const STATE = stateDir();
const HANDOFFS = env.NORTH_GROUNDING_HANDOFFS ?? join(HOME, ".local/state/agents/handoffs");
const LEDGER = env.NORTH_GROUNDING_LEDGER ?? join(STATE, "ledger.jsonl");
const WATERMARK = join(STATE, "distill-state.json");
const MINUTE = 60_000;
const DAY = 24 * 60 * MINUTE;
const OVERLAP = 30 * MINUTE;
const SUPERSEDE_WINDOW = 3 * DAY;
const QUOTE_MAX = 500;
const GENERIC = new Set(["issue", "policy", "skill", "hook", "lead", "worker", "test", "handoff", "landing", "review",
  "memory", "cpu", "terminal", "proxy"]);

export type Kind = "decision" | "direction" | "preference" | "correction" | "approval";
export type Row = { id: string; at: string; kind: Kind; topic: string; quote: string; source: string; superseded_by: string | null };

const BRIEF_SESSION = new RegExp(
  String.raw`^\s*(Delegation:|Category:|Parent:|You are (the|a|an) [^\n]{0,60}\b(lead|worker|agent|reviewer)\b` +
  String.raw`|Read \S*handoffs/(?![^\s]*proxy)\S+ and (follow|take over))`, "i");
const SKIP_MESSAGE = new RegExp(
  String.raw`^\s*(<(?!pasted_content)|/[a-z-]+\s*$|/compact|Delegation:|Category:|Parent:|Caveat:|\[Request interrupted` +
  String.raw`|This session is being continued|Base directory for this skill|Read \S*handoffs/\S*brief)`, "i");
const PASTED = /<pasted_content[^>]*>[\s\S]*?(<\/pasted_content>|$)/g;
const LINE_RECORD = /^\s*(?:[-*]\s+)?(DECIDED|APPROVED)\b[:\s]*(.*)$/;
const SECRET = new RegExp(
  String.raw`\b(sk-[A-Za-z0-9_-]{16,}|gh[pousr]_[A-Za-z0-9]{20,}|xox[abp]-[A-Za-z0-9-]{10,}|AKIA[A-Z0-9]{16}` +
  String.raw`|(?![0-9a-f]{40,}\b)[A-Za-z0-9+/_-]{40,})\b|\b(password|passwd|token|secret)\s*[:=]\s*\S+`, "gi");
const STATUS_TIME = /^(\d{1,2}):(\d{2})Z?\b/;
const ABOUT_THE_PAST = new RegExp(
  String.raw`^\W*(why (did|do|does|would|are|were|is) (you|u)\b(?! keep)|why'?d (you|u)\b|how come|what happened|what did (you|u)\b` +
  String.raw`|did (you|u)\b)`, "i");
const REQUEST = /^\W*((can|could|would|will) (you|we|u)\b|let'?s\b|please\b|why don'?t (we|you|u)\b)/i;

const RULES: [Kind, RegExp][] = [
  ["correction", new RegExp(
    String.raw`\b(we already (have|did|talked)|i (already )?told you|i'?ve already|i have already|didn'?t i (ask|say|tell)` +
    String.raw`|you keep\b|that'?s (wrong|not (it|right|what))|not what i (asked|said|meant|want)|why didn'?t you` +
    String.raw`|we did this (recently|already|before)|do we not have|why do we keep|you'?re approaching|i said\b|as i said` +
    String.raw`|stop (doing|asking|saying)|you were wrong|no,? (that|it)'?s not)`, "i")],
  ["decision", new RegExp(
    String.raw`\b(we('ll| will)? go with|let'?s go with|i'?ve decided|i decided|we decided|decision is|it'?s fine if|its fine if` +
    String.raw`|from now on|durably|new rule|rule change|going forward|instead of|i'?m choosing|we'?re (switching|moving) to)`, "i")],
  ["preference", new RegExp(
    String.raw`\b(i prefer|i'?d prefer|i'?d rather|i don'?t want|i dont want|i want (you|it|them|this|everything|all|every)` +
    String.raw`|only give me|i like (it|when|the)|i hate|i never want|always (use|give|check|answer|respond)` +
    String.raw`|never (use|give|ask|answer|start))`, "i")],
  ["direction", new RegExp(
    String.raw`\b(we need to|we have to|we need (a|an)|need you to|make sure|get it done|be ambitious|focus on|prioriti[sz]e` +
    String.raw`|should (be|have|always|never|only|just)|needs to be|must\b|keep (it|them|the)|do not\b|don'?t (ever|let|touch|use|start|close)` +
    String.raw`|remove (the|these|this|that)|close (my|the|all)|burn (it|codex|claude|down))`, "i")],
];
const APPROVAL = new RegExp(
  String.raw`^\s*(approved?|yes|yep|yeah|go ahead|do it|do (all|both|everything)|ship it|land it|sounds good` +
  String.raw`|lgtm|proceed|go for it|let'?s do (it|that))\b[\s.!,]*([^\n?]{0,80})$`, "i");
const ASKED = /\?|\b(approve|approval|should i|want me to|ok to|shall i|your call|confirm|choose|options?)\b/i;
const OVERRIDE = /\b(instead|actually|rather|prefer|no longer|not anymore|stop|don'?t|change|reverse|scratch that|forget|wrong)\b/i;

const SUBJECT_TABLE: Record<string, string> = {
  codex: "codex", sol: "codex", gpt: "codex", openai: "codex",
  claude: "claude", anthropic: "anthropic", opus: "opus", sonnet: "sonnet", haiku: "haiku",
  nexus: "nexus", laptop: "laptop", cloud: "cloud", farm: "farm", vast: "vast",
  smashcraft: "smashcraft", wisp: "wisp", wc3: "wc3", warcraft: "wc3", bnet: "bnet",
  north: "north", south: "south", firn: "firn", nixos: "firn", galileo: "galileo",
  telemetry: "telemetry", paperclip: "paperclip", proton: "proton", convo: "convo",
  proxy: "proxy", lead: "lead", leads: "lead", worker: "worker", workers: "worker",
  usage: "usage", burn: "usage", quota: "usage", pace: "pace", account: "account",
  accounts: "account", cpu: "cpu", load: "cpu", landing: "landing", land: "landing",
  autoland: "landing", "safe-push": "landing", review: "review", skill: "skill", skills: "skill",
  hook: "hook", hooks: "hook", rule: "policy", rules: "policy", policy: "policy",
  netcode: "netcode", rollback: "netcode", latency: "latency", test: "test", tests: "test",
  issue: "issue", issues: "issue", threads: "issue", transcripts: "memory", memory: "memory",
  retention: "memory", handoff: "handoff", handoffs: "handoff", timezone: "timezone",
  ghostty: "terminal", terminal: "terminal", terminals: "terminal",
};
const SUBJECTS = new Map(Object.entries(SUBJECT_TABLE));
const STOP = new Set(`a about above after again against all also am an and any are aren't as at be because been before being
below between both but by can can't cannot could couldn't did didn't do does doesn't doing don't down during each
few for from further get got had hasn't have haven't having he her here hers him his how i i'd i'll i'm i've if in
into is isn't it it's its itself just let let's like make me more most much my myself need needs no nor not now of
off on once only or other our ours out over own really right same shall she should shouldn't so some such than that
that's the their them then there these they this those through to too under until up us very was wasn't way we
we're were weren't what when where which while who whom why will with won't would wouldn't yeah yes you you're your
yours okay ok gonna want think know thing things kind basically actually something someone even still lot im dont
thats youre its isnt cant wont didnt doesnt ive whats theres`.split(/\s+/));

const iso = (ms: number) => new Date(ms).toISOString().slice(0, 19) + "Z";
const parseTs = (s: string) => Date.parse(s);
const squash = (s: string) => s.split(/\s+/).filter(Boolean).join(" ");

export function parseSince(s: string, now = Date.now()): number {
  const m = /^(\d+)([hdw])$/.exec(s);
  if (m) return now - Number(m[1]) * { h: 3_600_000, d: DAY, w: 7 * DAY }[m[2] as "h" | "d" | "w"];
  return parseTs(s.includes("T") ? s : s + "T00:00:00Z");
}

const rid = (key: string) => "L" + createHash("sha1").update(key).digest("hex").slice(0, 10);
const words = (text: string) => text.toLowerCase().match(/[a-z0-9][a-z0-9'-]*[a-z0-9]|[a-z0-9]/g) ?? [];

export function subjects(text: string): string[] {
  const seen: string[] = [];
  for (const w of words(text)) {
    const s = SUBJECTS.get(w);
    if (s && !seen.includes(s)) seen.push(s);
  }
  return seen;
}

export function topic(text: string): string {
  const found = subjects(text);
  const lower = text.toLowerCase();
  const counts = new Map<string, number>();
  for (const w of words(text)) {
    if (w.length >= 4 && !STOP.has(w) && !SUBJECTS.has(w) && !/^\d+$/.test(w)) counts.set(w, (counts.get(w) ?? 0) + 1);
  }
  const extra = [...counts.keys()].sort((a, b) => counts.get(b)! - counts.get(a)! || lower.indexOf(a) - lower.indexOf(b));
  return [...found, ...extra].slice(0, 6).join(" ");
}

const typedText = (text: string) => squash(text.replace(PASTED, " "));
const redact = (text: string) => text.replace(SECRET, "[redacted]");
const sentences = (text: string) => text.split(/(?<=[.!?])\s+/);

export function trimQuote(raw: string, pattern: RegExp | null): string {
  const text = redact(raw);
  if (text.length <= QUOTE_MAX) return text;
  const all = sentences(text);
  const picked = all.filter((s) => pattern?.test(s));
  let out = "";
  for (const s of picked.length ? picked : all.slice(0, 1)) {
    const piece = out ? " … " + s : s;
    if (out.length + piece.length > QUOTE_MAX) break;
    out += piece;
  }
  return out || text.slice(0, QUOTE_MAX);
}

const CORRECTION = RULES[0][1];
const counts = (s: string) =>
  !ABOUT_THE_PAST.test(s) && (!s.trimEnd().endsWith("?") || REQUEST.test(s) || CORRECTION.test(s));

export function classify(text: string, previousAssistant: string | null): [Kind | null, RegExp | null] {
  if (APPROVAL.test(text) && text.length <= 120 && previousAssistant && ASKED.test(previousAssistant.slice(-600))) {
    return ["approval", null];
  }
  if (/^\s*approved?\b/i.test(text)) return ["approval", null];
  const statements = sentences(text).filter(counts);
  for (const [kind, pattern] of RULES) {
    if (statements.some((s) => pattern.test(s))) return [kind, pattern];
  }
  if (statements.some((s) => REQUEST.test(s))) return ["direction", REQUEST];
  return [null, null];
}

const isTomSession = (first: string | undefined, prompts: number) =>
  first !== undefined && prompts >= 2 && !BRIEF_SESSION.test(first);

function refreshIndex() {
  try {
    Bun.spawnSync(["convo", "index"], { stdout: "ignore", stderr: "ignore", timeout: 600_000 });
  } catch {}
}

type Message = { msg: number; path: string; line: number; ts: string; role: string; text: string; session: string; previous: string | null };

function readConvo(since: number): Message[] {
  if (!existsSync(DB)) return [];
  const db = new Database(DB, { readonly: true });
  db.run("PRAGMA busy_timeout = 30000");
  db.run("PRAGMA query_only = 1");
  const rows = db.query(
    "SELECT m.id, m.file_id, m.line, m.ts, m.role, b.text, f.session_id, f.path FROM msg m JOIN files f ON f.id = m.file_id " +
    "JOIN body b ON b.id = m.body WHERE m.ts > ? AND m.dup = 0 AND f.agent IS NULL AND m.tool IS NULL " +
    "AND (m.role = 'user' OR (m.role = 'assistant' AND (b.text LIKE '%DECIDED%' OR b.text LIKE '%APPROVED%'))) " +
    "ORDER BY m.ts, m.id").values(new Date(since).toISOString()) as [number, number, number, string, string, string, string, string][];
  const fileIds = [...new Set(rows.map((r) => r[1]))].sort((a, b) => a - b);
  const first = new Map<number, [string, number]>();
  for (let i = 0; i < fileIds.length; i += 500) {
    const chunk = fileIds.slice(i, i + 500);
    const marks = chunk.map(() => "?").join(",");
    const got = db.query(
      `SELECT u.file_id, b.text, u.n FROM (SELECT file_id, min(id) AS first, count(*) AS n FROM msg ` +
      `WHERE role = 'user' AND file_id IN (${marks}) GROUP BY file_id) u ` +
      `JOIN msg m ON m.id = u.first JOIN body b ON b.id = m.body`).values(...chunk) as [number, string, number][];
    for (const [fid, text, n] of got) first.set(fid, [text, n]);
  }
  const tomFiles = new Set(fileIds.filter((fid) => isTomSession(first.get(fid)?.[0], first.get(fid)?.[1] ?? 0)));
  const previous = db.query(
    "SELECT b.text FROM msg m JOIN body b ON b.id = m.body WHERE m.file_id = ? AND m.line < ? " +
    "AND m.role = 'assistant' ORDER BY m.line DESC LIMIT 1");
  const out: Message[] = [];
  for (const [msg, fid, line, ts, role, text, session, path] of rows) {
    if (!tomFiles.has(fid)) continue;
    const prev = role === "user" ? (previous.values(fid, line)[0]?.[0] as string | undefined) ?? null : null;
    out.push({ msg, path, line, ts, role, text, session, previous: prev });
  }
  db.close();
  return out;
}

const row = (id: string, at: string, kind: Kind, textForTopic: string, quote: string, source: string): Row =>
  ({ id, at, kind, topic: topic(textForTopic), quote, source, superseded_by: null });

export const speaker = (r: Row) => r.source.split(" ", 1)[0];
const sessionOf = (r: Row) => /\bsession:(\S+)/.exec(r.source)?.[1] ?? null;

function convoRecords(messages: Message[]): Row[] {
  const out: Row[] = [];
  for (const m of messages) {
    const where = `convo:${m.msg} session:${m.session}`;
    const at = iso(parseTs(m.ts));
    const key = `convo:${m.session}:${m.path}:${m.line}`;
    if (m.role === "assistant") {
      m.text.split("\n").forEach((ln, n) => {
        const hit = LINE_RECORD.exec(ln);
        if (hit) out.push(row(rid(`${key}:${n}`), at, hit[1] === "APPROVED" ? "approval" : "decision", hit[2].trim(),
          ln.trim(), `proxy ${where}`));
      });
      continue;
    }
    if (SKIP_MESSAGE.test(m.text)) continue;
    const text = typedText(m.text);
    if (text.length < 6) continue;
    const [kind, pattern] = classify(text, m.previous);
    if (!kind) continue;
    const asked = kind === "approval" && m.previous ? m.previous.slice(-600) : "";
    out.push(row(rid(key), at, kind, text + " " + asked, trimQuote(text, pattern), `tom ${where}`));
  }
  return out;
}

function statusRecords(since: number): Row[] {
  const out: Row[] = [];
  if (!existsSync(HANDOFFS)) return out;
  for (const name of readdirSync(HANDOFFS).filter((n) => /status.*\.md$/.test(n)).sort()) {
    const path = join(HANDOFFS, name);
    let mtime: number;
    let lines: string[];
    try {
      mtime = statSync(path).mtimeMs;
      if (mtime <= since) continue;
      lines = readFileSync(path, "utf8").split("\n");
    } catch {
      continue;
    }
    const lead = name.replace(/\.md$/, "").replace("-status", "");
    lines.forEach((ln, i) => {
      const hit = LINE_RECORD.exec(ln);
      if (!hit) return;
      const body = hit[2].trim();
      let at = mtime;
      const clock = STATUS_TIME.exec(body);
      if (clock && Number(clock[1]) < 24) {
        const d = new Date(mtime);
        d.setUTCHours(Number(clock[1]), Number(clock[2]), 0, 0);
        at = d.getTime() > mtime ? d.getTime() - DAY : d.getTime();
      }
      const quote = squash(ln);
      const key = `status:${name}:${createHash("sha1").update(quote).digest("hex")}`;
      out.push(row(rid(key), iso(at), hit[1] === "APPROVED" ? "approval" : "decision", body, trimQuote(quote, null),
        `${lead} status:${path}:${i + 1}`));
    });
  }
  return out;
}

export function linkSupersession(rows: Row[], newIds: Set<string>): number {
  const tom = rows.filter((r) => speaker(r) === "tom").sort((a, b) => (a.at < b.at ? -1 : a.at > b.at ? 1 : 0));
  let links = 0;
  tom.forEach((newer, i) => {
    if (!newIds.has(newer.id) || !["correction", "decision", "preference"].includes(newer.kind)) return;
    if (!OVERRIDE.test(newer.quote)) return;
    const mine = new Set(subjects(newer.quote).filter((s) => !GENERIC.has(s)));
    const newerAt = parseTs(newer.at);
    for (let j = i - 1; j >= 0; j--) {
      const older = tom[j];
      if (newerAt - parseTs(older.at) > SUPERSEDE_WINDOW) break;
      if (older.superseded_by || !["decision", "direction", "preference"].includes(older.kind)) continue;
      if (older.quote !== newer.quote && subjects(older.quote).filter((s) => mine.has(s)).length >= 3) {
        older.superseded_by = newer.id;
        links++;
        break;
      }
    }
  });
  return links;
}

const loadLedger = (): Row[] =>
  existsSync(LEDGER) ? readFileSync(LEDGER, "utf8").split("\n").filter((l) => l.trim()).map((l) => JSON.parse(l)) : [];

function writeLedger(rows: Row[]) {
  const sorted = [...rows].sort((a, b) => (a.at + a.id < b.at + b.id ? -1 : 1));
  const tmp = LEDGER + ".tmp";
  writeFileSync(tmp, sorted.map((r) => JSON.stringify(r) + "\n").join(""));
  renameSync(tmp, LEDGER);
}

function withLock<T>(fn: () => T): T {
  const lock = join(STATE, "distill.lock.d");
  for (let tries = 0; ; tries++) {
    try {
      mkdirSync(lock);
      break;
    } catch {
      if (Date.now() - statSync(lock).mtimeMs > 10 * MINUTE) rmdirSync(lock);
      else if (tries > 600) throw new Error("distill: another run holds " + lock);
      else Bun.sleepSync(1000);
    }
  }
  try {
    return fn();
  } finally {
    rmdirSync(lock);
  }
}

function run(args: { since?: string; noRefresh: boolean }) {
  mkdirSync(STATE, { recursive: true });
  const result = withLock(() => {
    const state = existsSync(WATERMARK) ? JSON.parse(readFileSync(WATERMARK, "utf8")) : {};
    const since = args.since ? parseSince(args.since) : state.through ? parseTs(state.through) - OVERLAP : parseSince("14d");
    const started = Date.now();
    if (!args.noRefresh) refreshIndex();
    const rows = loadLedger();
    const known = new Set(rows.map((r) => r.id));
    const said = new Set(rows.filter((r) => speaker(r) === "tom").map((r) => `${sessionOf(r)}\0${r.quote}`));
    const unique = new Map<string, Row>();
    for (const r of [...convoRecords(readConvo(since)), ...statusRecords(since)]) {
      const repeat = `${sessionOf(r)}\0${r.quote}`;
      if (known.has(r.id) || unique.has(r.id) || (speaker(r) === "tom" && said.has(repeat))) continue;
      said.add(repeat);
      unique.set(r.id, r);
    }
    rows.push(...unique.values());
    const links = linkSupersession(rows, new Set(unique.keys()));
    writeLedger(rows);
    writeFileSync(WATERMARK, JSON.stringify({ through: iso(started), rows: rows.length }) + "\n");
    const kinds: Record<string, number> = {};
    for (const r of unique.values()) kinds[r.kind] = (kinds[r.kind] ?? 0) + 1;
    return { added: unique.size, kinds, superseded: links, total: rows.length, since: iso(since), ledger: LEDGER };
  });
  console.log(JSON.stringify(result));
}

function status() {
  const rows = loadLedger();
  const state = existsSync(WATERMARK) ? JSON.parse(readFileSync(WATERMARK, "utf8")) : {};
  console.log(JSON.stringify({ total: rows.length, superseded: rows.filter((r) => r.superseded_by).length,
    through: state.through ?? null, ledger: LEDGER }));
}

if (import.meta.main) {
  const [cmd, ...rest] = process.argv.slice(2);
  const sinceAt = rest.indexOf("--since");
  if (cmd === "run") run({ since: sinceAt >= 0 ? rest[sinceAt + 1] : undefined, noRefresh: rest.includes("--no-refresh") });
  else if (cmd === "status") status();
  else {
    console.error(USAGE);
    process.exit(2);
  }
}
