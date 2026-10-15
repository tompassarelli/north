#!/usr/bin/env bun
// Grounding block for one of Tom's messages: tier A reads the precomputed index; tier B adds convo rephrasings and handoffs.
import type { Database } from "bun:sqlite";
import { closeSync, openSync, readSync, statSync } from "node:fs";
import { refresh, open, search, spawnRefreshIfStale, terms, mainRunner, type Hit } from "./store";
import { ORGQ, USAGE, orgLines, paceLines } from "./live";
import { tierB } from "./tierb";
import { display } from "./sources/policy";
import { stamp } from "./sources/_convo";

export const LIMIT_CHARS = 6000;
export const CONFIDENT = 0.5;
const FLOOR = 0.2;
const SLOTS: [string, number][] = [["Decisions", 3], ["Skills", 4], ["Policy", 7], ["Handoffs", 2], ["Tom said", 4], ["Built before", 3], ["Projects", 3], ["Issues", 2]];
const PAST = /\b(already|again|before|earlier|last time|previously|recently|yesterday|remind|told you|we (did|have|had|talked|discussed|agreed|decided|built|made|used)|did(n'?t| not) (we|i|you)|don'?t we|do we (not )?have|have we|existing|remember|as i said|i said|i asked|(the|our|a|any) (skill|hook|rule|routine|policy|script|tool|protocol|system|index|ledger|guard)s?|why (was|is|did|didn'?t|don'?t)|how (does|do|did) (we|the|our|it))\b/i;

export function category(hit: Hit): string {
  if (hit.source === "policy" && hit.title.startsWith("handoff ")) return "Handoffs";
  if (hit.source === "policy") return hit.title.startsWith("skill ") || hit.ref.includes("/skills/") ? "Skills" : "Policy";
  if (hit.source === "settings") return "Policy";
  if (hit.source === "builds" || hit.source === "commits") return "Built before";
  if (hit.source === "threads" || hit.source === "github") return "Issues";
  return ({ ledger: "Decisions", transcripts: "Tom said", projects: "Projects" } as Record<string, string>)[hit.source] ?? hit.source;
}

function weight(hit: Hit): number {
  if (hit.source !== "ledger") return hit.score;
  const [speaker, kind] = hit.title.split(" ");
  return hit.score * (kind === "direction" ? 0.6 : 1) * (speaker === "tom" ? 1 : 0.8);
}

const flat = (text: string) => text.split(/\s+/).join(" ").trim();

function handoffWords(hit: Hit): Set<string> {
  return new Set(flat(hit.text).toLowerCase().match(/[a-z][a-z0-9_-]*/g) ?? []);
}

function sameHandoff(words: Set<string>, seen: Set<string>): boolean {
  if (!words.size || !seen.size) return false;
  let overlap = 0;
  for (const word of words) if (seen.has(word)) overlap++;
  return overlap / Math.min(words.size, seen.size) >= 0.9 && overlap / Math.max(words.size, seen.size) >= 0.8;
}

function sameMessage(hit: Hit, message: string): boolean {
  if (hit.source !== "transcripts" && hit.source !== "ledger") return false;
  const norm = flat(message).slice(0, 160);
  return Boolean(norm) && flat(hit.text).slice(0, 160) === norm;
}

function snippet(text: string, words: string[], width = 120): string {
  const f = flat(text);
  const low = f.toLowerCase();
  const pos = Math.min(...words.map((w) => low.indexOf(w)).filter((p) => p >= 0), Infinity);
  const start = Number.isFinite(pos) ? Math.max(0, pos - 30) : 0;
  return (start ? "…" : "") + f.slice(start, start + width) + (start + width < f.length ? "…" : "");
}

function outcome(hit: Hit): string {
  if (hit.source === "commits") return "landed";
  const shas = [...new Set((hit.text.match(/\b[0-9a-f]{8,40}\b/g) ?? []).slice(0, 3).map((s) => s.slice(0, 8)))];
  const head = hit.text.slice(0, 200).toLowerCase();
  const state = /^\W*(not done|blocked)/.test(head) ? "not finished" : /\b(landed|merged|pushed|queued)\b/i.test(hit.text) ? "landed" : "outcome unknown";
  return shas.length ? `${state} ${shas.join(" ")}` : state;
}

function hitLine(hit: Hit): string {
  if (hit.source === "transcripts") return `- ${hit.ref} (${hit.title}): ${snippet(hit.text, hit.matched, 80)}`;
  if (hit.source === "builds") return `- ${hit.ref} ${hit.title.slice(0, 80)} [${outcome(hit)}]`;
  if (hit.source === "commits") return `- ${hit.ref} ${hit.title.slice(0, 80)} [landed]`;
  const why = hit.text && !hit.title.startsWith("skill ") ? snippet(hit.text, hit.matched, 80) : "";
  const title = hit.title.length > 80 ? `${hit.title.slice(0, 80)}…` : hit.title;
  const head = title.includes(hit.ref) ? title : `${hit.ref} — ${title}`;
  return why ? `- ${head}: ${why}` : `- ${head}`;
}

function mentions(hits: Hit[]): string[] {
  const counts = new Map<string, number>();
  const placements = new Set<string>();
  for (const hit of hits) {
    const refs = new Set<string>();
    for (const placement of hit.title.matchAll(/\b(hook|skill):([a-z0-9_-]+)/g))
      placements.add(placement[1] === "hook" ? `harness/hooks/${placement[2]}.sh` : `harness/skills/${placement[2]}/SKILL.md`);
    const text = `${hit.ref}\n${hit.title}\n${hit.text}`;
    for (const found of text.matchAll(/(?:~\/|\/)?(?:[\w.@-]+\/)+[\w.@-]+(?:#[\w:-]+)?|\b(?:convo:\d+|rule:[\w-]+|[a-z][\w-]*@[0-9a-f]{8,40}|[0-9a-f]{8,40})\b/g)) {
      const ref = found[0].replace(/[.,]+$/, "");
      if (/^\d+$/.test(ref)) continue;
      if (ref.includes("/") && !/[.#]/.test(ref) && !/^(?:[~/]|harness\/|agents\/|lib\/|bin\/|docs\/|skills\/)/.test(ref)) continue;
      refs.add(ref);
    }
    for (const ref of refs) counts.set(ref, (counts.get(ref) ?? 0) + 1);
  }
  return [...placements, ...[...counts.keys()].filter((ref) => !placements.has(ref))
    .sort((a, b) => counts.get(b)! - counts.get(a)!).slice(0, Math.max(0, 30 - placements.size))];
}

export function select(hits: Hit[], message: string): { groups: Map<string, Hit[]>; rest: Hit[] } {
  const groups = new Map<string, Hit[]>();
  const rest: Hit[] = [];
  const seen = new Set<string>();
  const handoffs: Set<string>[] = [];
  const limits = new Map(SLOTS);
  for (const hit of [...hits].sort((a, b) => weight(b) - weight(a))) {
    if (hit.cover < FLOOR || seen.has(hit.ref) || sameMessage(hit, message)) continue;
    const name = category(hit);
    if (name === "Handoffs") {
      const words = handoffWords(hit);
      if (handoffs.some((prior) => sameHandoff(words, prior))) continue;
      handoffs.push(words);
    }
    seen.add(hit.ref);
    const bucket = groups.get(name) ?? [];
    groups.set(name, bucket);
    if (bucket.length < (limits.get(name) ?? 2)) bucket.push(hit);
    else rest.push(hit);
  }
  return { groups, rest };
}

export function compose(message: string, hits: Hit[], words: string[], live: string[], extra: string[]): string {
  const lines = [`Grounding (auto-retrieved for this message; terms: ${words.slice(0, 12).join(" ")}). ` +
    "Read the named files before answering; say what already exists before proposing anything new.", ...live];
  const { groups, rest } = select(hits, message);
  const selected = [...groups.values()].flat();
  const entities = mentions(selected).join(", ");
  if (entities) lines.push(`Mentions: ${entities.slice(0, 1600)}`);
  for (const [name] of SLOTS) {
    const items = groups.get(name);
    if (items?.length) lines.push(`${name}:`, ...items.map(hitLine));
  }
  lines.push(...extra);
  const more = rest.filter((h) => h.source !== "builds" && h.source !== "commits").map((h) => h.ref);
  if (more.length) lines.push(`Also matching: ${more.slice(0, 20).join(", ")}`);
  const out: string[] = [];
  let size = 0;
  for (let line of lines) {
    if (size + line.length + 1 > LIMIT_CHARS) line = line.slice(0, Math.max(0, LIMIT_CHARS - size - 1));
    if (!line) break;
    out.push(line);
    size += line.length + 1;
  }
  return out.join("\n");
}

export const tierBCommand = (message: string) =>
  `bun ${display(mainRunner())} --tier B --query '${message.replaceAll("'", " ").slice(0, 300)}'`;

function index(): Database | null {
  try {
    return open(true);
  } catch {
    return null;
  }
}

export function retrieve(message: string, db: Database, now: number, limit = 80, asOf = Number(process.env.NORTH_GROUNDING_AS_OF) || 0, prior = "") {
  const result = search(message, db, now, limit, asOf);
  if (prior) {
    const current = new Set(terms(message));
    const context = terms(prior).filter((word) => !current.has(word)).slice(0, 16).join(" ");
    if (context) {
      const known = new Set(result.hits.map((hit) => `${hit.source}:${hit.id}`));
      result.hits.push(...search(context, db, now, limit, asOf).hits.filter((hit) => !known.has(`${hit.source}:${hit.id}`))
        .map((hit) => ({ ...hit, score: hit.score * 0.4 })));
    }
  }
  result.hits.sort((a, b) => b.score - a.score);
  return result;
}

export async function block(message: string, tier = "A", now = Date.now() / 1000, prior = ""): Promise<string> {
  const usage = USAGE.test(message);
  const org = ORGQ.test(message);
  const count = terms(message).length;
  if (count < 2 && (!prior || !count) && !usage && !org) return "";
  const db = index();
  let hits: Hit[] = [];
  let words = terms(message);
  try {
    if (db) ({ hits, words } = retrieve(message, db, now, 80, undefined, prior));
  } catch {
    hits = [];
  }
  if (!words.length) words = terms(message);
  const live = [...(usage ? paceLines() : []), ...(org ? orgLines(message) : [])];
  const top = hits.reduce((m, h) => Math.max(m, h.cover), 0);
  const confident = top >= CONFIDENT;
  const extra: string[] = [];
  if (tier === "B") {
    const b = await tierB(words, db);
    if (b.lines.length) extra.push("Tier B (convo rephrasings, handoffs):", ...b.lines.slice(0, 14));
    if (!confident && !b.lines.length) {
      const closest = hits[0];
      const near = closest ? `closest: ${closest.ref}${closest.at ? ` on ${stamp(closest.at)}` : ""}` : "closest: none";
      extra.push(`Ask Tom in this form: "searched ${b.searched.join(", ")} for '${words.slice(0, 6).join(" ")}'; ${near}; this one?"`);
    }
  } else if (!confident && PAST.test(message)) {
    extra.push(`No confident tier-A hit and this refers to past work or existing infrastructure: before answering run tier B (<=60 s): \`${tierBCommand(message)}\``);
  }
  db?.close();
  if (!hits.some((h) => h.cover >= FLOOR) && !live.length && !extra.length) return "";
  return compose(message, hits, words, live, extra);
}

function priorReply(path: string): string {
  let fd: number | undefined;
  try {
    const size = statSync(path).size;
    fd = openSync(path, "r");
    const buffer = Buffer.alloc(Math.min(size, 1048576));
    readSync(fd, buffer, 0, buffer.length, Math.max(0, size - buffer.length));
    const rows = buffer.toString().split("\n");
    for (let i = rows.length - 1; i >= 0; i--) {
      let row;
      try { row = JSON.parse(rows[i]); } catch { continue; }
      if (row.type !== "assistant" || row.message?.role !== "assistant") continue;
      const content = row.message.content;
      const text = typeof content === "string" ? content : Array.isArray(content)
        ? content.filter((part: { type: string }) => part.type === "text").map((part: { text: string }) => part.text).join("\n") : "";
      if (text) return text.slice(0, 4000);
    }
  } catch {
    return "";
  } finally {
    if (fd !== undefined) closeSync(fd);
  }
  return "";
}

async function hookMessage(): Promise<{ message: string; prior: string } | null> {
  const raw = await Bun.stdin.text();
  if (raw.length > 1048576) return { message: "", prior: "" };
  let data: Record<string, unknown>;
  try {
    data = JSON.parse(raw);
  } catch {
    return null;
  }
  if (!data || typeof data !== "object" || "agent_id" in data || "agent_type" in data) return { message: "", prior: "" };
  if ((data.hook_event_name ?? "UserPromptSubmit") !== "UserPromptSubmit") return { message: "", prior: "" };
  return { message: String(data.prompt ?? ""), prior: typeof data.transcript_path === "string" ? priorReply(data.transcript_path) : "" };
}

function flag(args: string[], name: string): string | undefined {
  const i = args.indexOf(name);
  return i >= 0 ? args[i + 1] : undefined;
}

export async function main(args: string[]): Promise<number> {
  if (args.includes("--help") || args.includes("-h")) {
    console.log("usage: grounding.ts [--query TEXT] [--tier A|B] | refresh [--force] [--if-idle] [--source NAME]... | hook");
    return 0;
  }
  if (args[0] === "refresh") {
    const only = args.flatMap((a, i) => (a === "--source" ? [args[i + 1]] : []));
    console.log(JSON.stringify(await refresh(only.length ? only : null, args.includes("--force"), !args.includes("--if-idle"))));
    return 0;
  }
  if (args[0] === "hook") {
    const payload = await hookMessage();
    if (payload === null) return 65;
    spawnRefreshIfStale();
    const text = payload.message ? await block(payload.message, "A", Date.now() / 1000, payload.prior) : "";
    if (text) console.log(JSON.stringify({ hookSpecificOutput: { hookEventName: "UserPromptSubmit", additionalContext: text } }));
    return 0;
  }
  const text = await block(flag(args, "--query") ?? flag(args, "-q") ?? "", flag(args, "--tier") ?? "A", Date.now() / 1000, process.env.NORTH_GROUNDING_PRIOR ?? "");
  if (text) console.log(text);
  return 0;
}

if (import.meta.main) process.exit(await main(process.argv.slice(2)));
