import { closeSync, openSync, readFileSync, readSync, statSync } from "node:fs";
import { join } from "node:path";
import { HOME } from "./store";
import { display } from "./sources/policy";
import { stamp } from "./sources/_convo";

const PACE = process.env.NORTH_GROUNDING_PACE ?? join(HOME, ".local/state/agents/pace-control.json");
const ORG = process.env.AGENTS_ORG_FILE ?? join(HOME, ".local/state/agents/org.json");

export const USAGE = /\b(status report|executive|usage|pace|pacing|burn|quota|limits?|accounts?|claude ?[abc]|claude:[abc]|codex|5h|weekly|week|resets?|capacity|percent)\b|\d+ ?%/i;
export const ORGQ = /\b(leads?|org|status report|executive|who is|staff|staffed|proxy)\b/i;

type Window = { window?: string; used_pct?: number; resets_at?: string; as_of?: string };
type Account = { windows?: Window[]; held?: boolean; allowed_workers?: number };

export function paceLines(): string[] {
  let data: { accounts?: Record<string, Account> };
  try {
    data = JSON.parse(readFileSync(PACE, "utf8"));
  } catch {
    return [];
  }
  const lines = [`Usage, source of truth ${display(PACE)} (written by north harness/lib/pace_control.py; never quote a lead's status line):`];
  for (const [name, account] of Object.entries(data.accounts ?? {}).sort()) {
    const parts = (account.windows ?? []).map((w) =>
      `${w.window} ${Math.round(w.used_pct ?? 0)}% (resets ${(w.resets_at ?? "").slice(11, 16)}Z, as of ${(w.as_of ?? "").slice(11, 16)}Z)`);
    lines.push(`- ${name}: ${parts.join("; ") || "no windows"}; allowed_workers ${account.allowed_workers}${account.held ? " held" : ""}`);
  }
  return lines;
}

function lastStatus(path: string): [number, string] {
  try {
    const size = statSync(path).size;
    const fd = openSync(path, "r");
    const buf = Buffer.alloc(Math.min(4096, size));
    readSync(fd, buf, 0, buf.length, Math.max(0, size - buf.length));
    closeSync(fd);
    const tail = buf.toString().trim().split("\n");
    return [statSync(path).mtimeMs / 1000, (tail[tail.length - 1] ?? "").slice(0, 160)];
  } catch {
    return [0, ""];
  }
}

type Node = { id: string; domain?: string; provider?: string; state?: string; status_file?: string };

export function orgLines(message: string): string[] {
  let data: { nodes?: Node[] };
  try {
    data = JSON.parse(readFileSync(ORG, "utf8"));
  } catch {
    return [];
  }
  const nodes = (data.nodes ?? []).filter((n) => n.state === "active");
  const low = message.toLowerCase();
  const named = nodes.filter((n) => low.includes(n.id.toLowerCase()) || low.includes(n.id.replace(/-lead$/, "").replaceAll("-", " ")));
  const lines = [`Org (same as \`agents org show\`, from ${display(ORG)}): ${nodes.length} active leads: ` +
    nodes.map((n) => `${n.id} (${n.domain}, ${n.provider})`).join(", ").slice(0, 700)];
  const chosen = named.length ? named : [...nodes].sort((a, b) => lastStatus(b.status_file ?? "")[0] - lastStatus(a.status_file ?? "")[0]).slice(0, 3);
  for (const node of chosen) {
    const [mtime, line] = lastStatus(node.status_file ?? "");
    if (mtime) lines.push(`- ${node.id} status ${display(node.status_file!)} @ ${stamp(mtime)}: ${line}`);
  }
  let budget = 600;
  return lines.flatMap((line) => {
    const compact = line.slice(0, Math.max(0, budget - 1));
    budget -= compact.length + 1;
    return compact ? [compact] : [];
  });
}
