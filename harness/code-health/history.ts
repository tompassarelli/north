import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { stateDir } from "./state";
import { git } from "./files";

export const WINDOW_DAYS = 90;
export const WINDOW_COMMITS = 2000;
const BULK_COMMIT_FILES = 50;

export interface History { commits: number; churn: Map<string, number>; changes: string[][] }

export function history(repo: string, rev: string): History {
  const at = Number(git(repo, ["log", "-1", "--format=%ct", rev]).trim());
  const since = new Date((at - WINDOW_DAYS * 86400) * 1000).toISOString();
  const out = git(repo, ["log", `-n${WINDOW_COMMITS}`, `--since=${since}`, "--no-renames", "--no-merges", "--format=%x1e", "--name-only", rev]);
  const churn = new Map<string, number>();
  const changes: string[][] = [];
  for (const block of out.split("\x1e").slice(1)) {
    const files = block.split("\n").filter(Boolean);
    changes.push(files);
    for (const f of files) churn.set(f, (churn.get(f) ?? 0) + 1);
  }
  return { commits: changes.length, churn, changes };
}

export interface Pair { a: string; b: string; support: number; confidence: number }

export function coChange(h: History, keep: (path: string) => boolean, group: (path: string) => string | undefined): Pair[] {
  const counts = new Map<string, number>();
  for (const files of h.changes) {
    const fs = files.filter(keep);
    if (fs.length < 2 || fs.length > BULK_COMMIT_FILES) continue;
    fs.sort();
    for (let i = 0; i < fs.length; i++)
      for (let j = i + 1; j < fs.length; j++) {
        const ga = group(fs[i]), gb = group(fs[j]);
        if (ga !== undefined && ga === gb) continue;
        const k = `${fs[i]}\0${fs[j]}`;
        counts.set(k, (counts.get(k) ?? 0) + 1);
      }
  }
  const pairs: Pair[] = [];
  for (const [k, support] of counts) {
    if (support < 3) continue;
    const [a, b] = k.split("\0");
    pairs.push({ a, b, support, confidence: support / Math.min(h.churn.get(a)!, h.churn.get(b)!) });
  }
  return pairs.sort((x, y) => y.support - x.support || y.confidence - x.confidence);
}

export interface Allow { rule: string; path: string; reason: string; commit: string }

export function allows(repo: string, range: string[]): Allow[] {
  const out = git(repo, ["log", "--format=%x1e%H%x1f%(trailers:key=Code-health-allow,valueonly,separator=%x1f)", ...range]);
  const result: Allow[] = [];
  for (const block of out.split("\x1e").slice(1)) {
    const [commit, ...values] = block.trim().split("\x1f");
    for (const v of values) {
      const m = v.trim().match(/^(\S+)\s+(\S+)\s*(?:—|--?)?\s*(.*)$/);
      if (m) result.push({ rule: m[1], path: m[2], reason: m[3], commit });
    }
  }
  return result;
}

export function churnAt(repo: string, rev: string): Map<string, number> {
  const sha = git(repo, ["rev-parse", `${rev}^{commit}`]).trim();
  const dir = join(stateDir(repo), "churn");
  const path = join(dir, `${sha}-${WINDOW_DAYS}-${WINDOW_COMMITS}.json`);
  try {
    return new Map(JSON.parse(readFileSync(path, "utf8")));
  } catch {}
  const churn = history(repo, sha).churn;
  mkdirSync(dir, { recursive: true });
  writeFileSync(path, JSON.stringify([...churn]));
  return churn;
}
