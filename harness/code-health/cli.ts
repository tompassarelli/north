import { resolve } from "node:path";
import { check } from "./check";
import { configAtRev } from "./config";
import { Snapshot, ToolError, toplevel } from "./files";
import { astGrepAvailable } from "./functions";
import { formatViolation, gate } from "./gate";
import { buildReport, formatReport, trendRow } from "./report";
import { reviewDone, signal } from "./signal";
import { save } from "./state";

const USAGE = `usage: code-health <command> [--repo DIR] [--config FILE] [--rev REV] [--json]
  report [--save]          full metrics for REV's tree (default HEAD); --save writes the baseline and appends the trend
  gate BASE HEAD           new structural debt between two commits (exit 0 pass, 1 debt, 2 error)
  check FILE               edit-time check of one worktree file against HEAD (prints nothing when clean)
  signal                   FIRE <reason> lines for REV (exit 0) or quiet (exit 1) for the recurring review
  review-done [--sha SHA]  record the review marker`;

interface Args { cmd?: string; pos: string[]; repo?: string; config?: string; json: boolean; save: boolean; sha?: string; rev: string }

function parse(argv: string[]): Args {
  const a: Args = { pos: [], json: false, save: false, rev: "HEAD" };
  for (let i = 0; i < argv.length; i++) {
    const x = argv[i];
    const val = () => { if (i + 1 >= argv.length) throw new ToolError(`${x} needs a value`); return argv[++i]; };
    if (x === "--repo") a.repo = val();
    else if (x === "--config") a.config = resolve(val());
    else if (x === "--sha") a.sha = val();
    else if (x === "--rev") a.rev = val();
    else if (x === "--json") a.json = true;
    else if (x === "--save") a.save = true;
    else if (x === "-h" || x === "--help") a.cmd = "help";
    else if (x.startsWith("--")) throw new ToolError(`unknown flag ${x}`);
    else if (!a.cmd) a.cmd = x;
    else a.pos.push(x);
  }
  return a;
}

function run(a: Args): number {
  if (!a.cmd || a.cmd === "help") { console.log(USAGE); return a.cmd ? 0 : 2; }
  const repo = toplevel(a.repo ? resolve(a.repo) : process.cwd());
  switch (a.cmd) {
    case "report": {
      if (!astGrepAvailable()) throw new ToolError("ast-grep is not on PATH");
      const r = buildReport(repo, a.rev, configAtRev(new Snapshot(repo, a.rev), a.config));
      if (a.save) save(repo, r, trendRow(r));
      console.log(a.json ? JSON.stringify(r) : formatReport(r));
      return 0;
    }
    case "gate": {
      if (a.pos.length !== 2) throw new ToolError("gate needs BASE HEAD");
      if (!astGrepAvailable()) { console.error("code-health: ast-grep is not on PATH; gate skipped"); return 0; }
      const r = gate(repo, a.pos[0], a.pos[1], a.config);
      if (r.warning) console.error(r.warning);
      if (a.json) console.log(JSON.stringify(r.violations));
      else for (const v of r.violations) console.log(formatViolation(v));
      return r.violations.length ? 1 : 0;
    }
    case "check": {
      if (a.pos.length !== 1) throw new ToolError("check needs FILE");
      if (!astGrepAvailable()) return 0;
      const vs = check(repo, a.pos[0], a.config);
      if (a.json) { if (vs.length) console.log(JSON.stringify(vs)); }
      else for (const v of vs) console.log(formatViolation(v));
      return vs.length ? 1 : 0;
    }
    case "signal": {
      const reasons = signal(repo, configAtRev(new Snapshot(repo, a.rev), a.config), Date.now(), a.rev);
      if (a.json) console.log(JSON.stringify({ fire: reasons.length > 0, reasons }));
      else console.log(reasons.length ? reasons.map((r) => `FIRE ${r}`).join("\n") : "quiet");
      return reasons.length ? 0 : 1;
    }
    case "review-done": {
      reviewDone(repo, a.sha ?? a.rev, a.config ? configAtRev(new Snapshot(repo, a.sha ?? a.rev), a.config) : undefined);
      return 0;
    }
    default:
      throw new ToolError(`unknown command ${a.cmd}\n${USAGE}`);
  }
}

try {
  process.exitCode = run(parse(Bun.argv.slice(2)));
} catch (e) {
  console.error(`code-health: ${e instanceof ToolError ? e.message : (e as Error).stack}`);
  process.exitCode = 2;
}
