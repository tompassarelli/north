import { closeSync, openSync, readdirSync, readSync, statSync } from "node:fs";
import { join } from "node:path";
import type { Database } from "bun:sqlite";
import { HOME, match } from "./store";
import { display } from "./sources/policy";
import { stamp } from "./sources/_convo";

const HANDOFFS = process.env.NORTH_GROUNDING_HANDOFFS ?? join(HOME, ".local/state/agents/handoffs");
const BUDGET_MS = 55_000;

export function rephrasings(ranked: string[]): string[][] {
  const out: string[][] = [];
  for (const item of [ranked.slice(0, 3), ranked.slice(0, 5), ranked.slice(1, 4)])
    if (item.length && !out.some((o) => o.join(" ") === item.join(" "))) out.push(item);
  return out.slice(0, 3);
}

type ConvoRow = { id?: number; role?: string; ts?: string; session?: string; snippet?: string; partial?: boolean };

export async function tierB(words: string[], db: Database | null): Promise<{ lines: string[]; searched: string[] }> {
  const deadline = Date.now() + BUDGET_MS;
  const searched = ["skills/policy/ledger/issues index", "14-day history"];
  const df = (w: string) => {
    try {
      return (db?.query("select count(*) n from docs where docs match ?").get(match(w)) as { n: number }).n || 1e9;
    } catch {
      return 1e9;
    }
  };
  const ranked = [...words].sort((a, b) => df(a) - df(b));
  const lines: string[] = [];
  const seen = new Set<string>();
  for (const [i, phrase] of rephrasings(ranked).entries()) {
    if (Date.now() > deadline) break;
    const cmd = ["convo", ...(i ? ["-u"] : []), "-m", "-n", "6", "--json", "-r", "user,assistant", ...phrase];
    let rows: ConvoRow[] = [];
    try {
      const proc = Bun.spawn(cmd, { stdout: "pipe", stderr: "ignore" });
      const timer = setTimeout(() => proc.kill(), Math.max(5000, deadline - Date.now()));
      const out = await new Response(proc.stdout).text();
      clearTimeout(timer);
      rows = JSON.parse(out || "[]");
    } catch {
      continue;
    }
    searched.push(`convo '${phrase.join(" ")}'`);
    for (const row of rows) {
      const key = `convo:${row.id}`;
      if (seen.has(key) || row.partial) continue;
      seen.add(key);
      lines.push(`- ${key} (${row.role} ${(row.ts ?? "").slice(0, 16)}Z session ${(row.session ?? "").slice(0, 8)}): "${(row.snippet ?? "").split(/\s+/).join(" ").slice(0, 180)}"`);
    }
  }
  searched.push(`handoff names/headings in ${display(HANDOFFS)}`);
  lines.push(...handoffHits(ranked, deadline));
  return { lines, searched };
}

export function handoffHits(words: string[], deadline: number): string[] {
  if (!words.length) return [];
  let names: string[] = [];
  try {
    names = readdirSync(HANDOFFS);
  } catch {
    return [];
  }
  const scored: [number, number, string][] = [];
  const need = words.length > 2 ? 2 : 1;
  for (const name of names) {
    const path = join(HANDOFFS, name);
    let score = words.filter((w) => name.toLowerCase().includes(w)).length;
    if (name.endsWith(".md") && Date.now() < deadline) {
      try {
        const fd = openSync(path, "r");
        const buf = Buffer.alloc(4096);
        const n = readSync(fd, buf, 0, 4096, 0);
        closeSync(fd);
        const heads = buf.toString("utf8", 0, n).split("\n").filter((l) => l.startsWith("#")).join(" ").toLowerCase();
        score += 0.5 * words.filter((w) => heads.includes(w)).length;
      } catch {
        // unreadable entries are skipped
      }
    }
    if (score >= need) {
      try {
        scored.push([score, statSync(path).mtimeMs / 1000, path]);
      } catch {
        continue;
      }
    }
  }
  scored.sort((a, b) => b[0] - a[0] || b[1] - a[1]);
  return scored.slice(0, 6).map(([, m, p]) => `- ${display(p)} (handoff, ${stamp(m)})`);
}
