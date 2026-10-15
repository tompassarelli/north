import { Database } from "bun:sqlite";
import { existsSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { NATIVE, visibleNative } from "./_threads.ts";

import type { Doc } from "../store";

export const COMPLETE = true;
export const REFRESH_SECONDS = 900;
export const LIMIT = 2000;

export function ledgerPath(env: Record<string, string | undefined> = process.env): string {
  if (env.THREADS_DB) return env.THREADS_DB;
  const state = join(env.XDG_STATE_HOME || join(homedir(), ".local/state"), "threads");
  return join(state, existsSync(join(homedir(), ".config/threads/remote")) ? "snapshot.db" : "threads.db");
}

export function epoch(text: string | null | undefined): number {
  if (!text) return 0;
  const stamp = /[zZ]|[+-]\d\d:\d\d$/.test(text) ? text : `${text}Z`;
  return Date.parse(stamp) / 1000;
}

function trim(lines: string[]): string {
  const text = lines.filter(Boolean).join("\n");
  return text.length <= LIMIT ? text : `${text.slice(0, LIMIT - 1)}…`;
}

function eventLine(ts: string, kind: string, actor: string, data: Record<string, any>): string {
  const at = ts.slice(0, 16);
  if (kind === "handoff") return `${at} handoff ${data.from ?? ""} -> ${data.to ?? ""}`;
  if (["claim", "release", "capture", "close", "reopen"].includes(kind)) return `${at} ${kind} ${actor}`.trimEnd();
  if (kind === "need" || kind === "unneed") return `${at} ${kind} ${data.needs ?? ""}`;
  const said = data.text || data.block || data.body;
  return said ? `${at} ${kind} ${actor}: ${said}` : "";
}

function open(path: string): Database {
  const snapshot = path.endsWith("/snapshot.db");
  const db = new Database(path, { readonly: true });
  db.exec("PRAGMA busy_timeout = 2000");
  if (snapshot) {
    const check = db.query("PRAGMA quick_check").values();
    if (check.length !== 1 || check[0][0] !== "ok") {
      db.close();
      throw new Error(`threads snapshot ${path} failed its integrity check`);
    }
  }
  return db;
}

export function* iterDocs(since: number | null, path: string = ledgerPath()): Iterable<Doc> {
  const native = visibleNative();
  const db = open(path);
  const openTitles = new Map<string, string>();
  const trail = new Map<string, [number, string][]>();
  const latest = new Map<string, number>();
  const note = (item: string, at: number, line: string) => {
    if (line) {
      if (!trail.has(item)) trail.set(item, []);
      trail.get(item)!.push([at, line]);
    }
    latest.set(item, Math.max(latest.get(item) ?? 0, at));
  };
  let items: any[];
  const blocks = new Map<string, string>();
  const claims = new Map<string, string>();
  try {
    for (const [repo, issues] of db.query("SELECT repo, issues FROM gh_cache").values() as [string, string][]) {
      for (const [number, issue] of Object.entries(JSON.parse(issues) as Record<string, { title: string }>)) {
        openTitles.set(`${repo}#${number}`, issue.title);
      }
    }
    for (const [ts, item, kind, actor, data] of db
      .query("SELECT ts, item, kind, actor, data FROM events WHERE item LIKE '%#%' ORDER BY seq").values() as string[][]) {
      note(item, epoch(ts), eventLine(ts, kind, actor, JSON.parse(data)));
    }
    for (const [item, ended, outcome, tier, report, runNote] of db
      .query("SELECT item, ended, outcome, tier, report, note FROM runs WHERE item LIKE '%#%'").values() as string[][]) {
      note(item, epoch(ended), `${ended.slice(0, 16)} run ${tier} ${outcome}: ${report}${runNote ? ` (note ${runNote})` : ""}`);
    }
    for (const [item, reason] of db.query("SELECT item, reason FROM blocks").values() as string[][]) blocks.set(item, reason);
    for (const [item, owner] of db.query("SELECT item, owner FROM claims").values() as string[][]) claims.set(item, owner);
    items = db.query("SELECT ns, number, title, body, state, created, closed, due FROM items").all();
  } finally {
    db.close();
  }

  const recent = (item: string) => [...(trail.get(item) ?? [])].sort((a, b) => b[0] - a[0]).map(([, line]) => line);
  const status = (item: string) => [
    claims.has(item) ? `owner: ${claims.get(item)}` : "",
    blocks.has(item) ? `blocked: ${blocks.get(item)}` : "",
  ];
  const seen = new Set<string>();
  for (const row of items) {
    const item = `${row.ns}#${row.number}`;
    seen.add(item);
    if (!native.includes(row.ns)) continue;
    const at = Math.max(epoch(row.created), epoch(row.closed), latest.get(item) ?? 0);
    if (since !== null && at <= since) continue;
    const head = [row.title, `state: ${row.state}${row.due ? `, due ${row.due}` : ""}`, row.body, ...status(item)];
    yield { source: "threads", id: item, ref: `threads ${item}`, title: row.title, text: trim([...head, ...recent(item)]),
      at, decays: row.state === "closed" };
  }
  for (const item of [...latest.keys()].sort()) {
    const at = latest.get(item) ?? 0;
    if (seen.has(item) || NATIVE.includes(item.split("#")[0]) || (since !== null && at <= since)) continue;
    const title = openTitles.get(item) ?? item;
    const head = [title, `state: ${openTitles.has(item) ? "open" : "not open"}`, ...status(item)];
    yield { source: "threads", id: item, ref: `threads ${item}`, title, text: trim([...head, ...recent(item)]), at,
      decays: !openTitles.has(item) };
  }
}
