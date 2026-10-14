export const REPOS = ["smashcraft", "wisp", "north", "firn", "south", "padbridge", "smashcraft-client", "proton-triage"] as const;
export type Repo = (typeof REPOS)[number];

export type Gate = { workflow: string; name: string; budget_s: number | null; budget_source: string | null; debt_to_s?: number };

export const GATES: Record<Repo, Gate[]> = {
  smashcraft: [{ workflow: "farm-test.yml", name: "farm-test", budget_s: 240, budget_source: "T1" }],
  wisp: [{ workflow: "farm-test.yml", name: "farm-test", budget_s: null, budget_source: null }],
  north: [
    { workflow: "cargo.yml", name: "cargo", budget_s: null, budget_source: null },
    { workflow: "harness.yml", name: "harness", budget_s: null, budget_source: null },
  ],
  firn: [{ workflow: "tests.yml", name: "Tests", budget_s: 345, budget_source: "T2" }],
  south: [],
  "padbridge": [{ workflow: "ci.yml", name: "ci", budget_s: null, budget_source: null }],
  "smashcraft-client": [{ workflow: "ci.yml", name: "ci", budget_s: null, budget_source: null }],
  "proton-triage": [],
};

export type Link = { value: number | string | null; href: string | null; source?: string };
export type Job = { startedAt: string | null; completedAt: string | null; conclusion: string | null };
export type Run = { id: number; url: string; createdAt: string; headSha: string; conclusion: string; jobs: Job[] };

export function runWall(jobs: Job[]): number | null {
  const ran = jobs.filter((j) => j.startedAt && j.completedAt && j.conclusion !== "skipped" && j.startedAt <= j.completedAt!);
  if (ran.length === 0) return null;
  const start = Math.min(...ran.map((j) => Date.parse(j.startedAt!)));
  const end = Math.max(...ran.map((j) => Date.parse(j.completedAt!)));
  return Math.round((end - start) / 1000);
}

export function finished(runs: Run[]): Run[] {
  return runs
    .filter((r) => r.conclusion === "success" || r.conclusion === "failure")
    .sort((a, b) => (a.createdAt < b.createdAt ? 1 : a.createdAt > b.createdAt ? -1 : b.id - a.id));
}

export function days(now: string, n = 30): string[] {
  const end = Date.parse(now.slice(0, 10) + "T00:00:00Z");
  const out: string[] = [];
  for (let i = n - 1; i >= 0; i--) out.push(new Date(end - i * 86400000).toISOString().slice(0, 10));
  return out;
}

export function lastPerDay(runs: Run[], dayList: string[]): Map<string, Run> {
  const map = new Map<string, Run>();
  for (const r of finished(runs).slice().reverse()) {
    const day = r.createdAt.slice(0, 10);
    if (dayList.includes(day)) map.set(day, r);
  }
  return map;
}

const TEST_PATH = /(^|\/)(tests?\/.*\.rs|test_[^/]*\.py|[^/]*[._-]tests?\.(ts|tsx|mjs|js|sh|py|bjs|rs)|[^/]*_test\.(bjs|rs|py|go))$/;
export function isTestPath(path: string): boolean {
  return TEST_PATH.test(path) && !path.includes("/fixtures/") && !path.includes("node_modules/");
}

export type CensusRow = { repo: string; path: string; test: string; kind: string; cases: number; evidence: string; action: string; source: string };

export function parseTsv(text: string, source: string): CensusRow[] {
  const rows: CensusRow[] = [];
  for (const line of text.split("\n")) {
    const f = line.split("\t");
    if (f.length < 6 || f[0] === "repo:path") continue;
    const colon = f[0].indexOf(":");
    const m = /^\d+/.exec(f[3].trim());
    rows.push({ repo: f[0].slice(0, colon), path: f[0].slice(colon + 1), test: f[1], kind: f[2].trim(), cases: m ? Number(m[0]) : 0, evidence: f[4], action: f[5].trim(), source });
  }
  return rows;
}

export function active(row: CensusRow): boolean {
  return /^(keep|pending)/.test(row.action) || /^rejected.*kept/.test(row.action);
}

const UNIQUE = /unique|only here|killed only|only killer|sole |mutant|\bkills?\b|^[a-z0-9]+(-[a-z0-9]+)+[;:]/i;
export function citesUniqueKill(row: CensusRow): boolean {
  return /^[1-4]$/.test(row.kind) && row.action.startsWith("keep") && UNIQUE.test(row.evidence);
}

export type Census = {
  status: "census" | "no census";
  tests: Link;
  five_kind: Link;
  kinds: Record<string, number> | null;
  unique_kill: Link;
  sources: string[];
};

const share = (a: number, b: number) => (b === 0 ? null : Math.round((1000 * a) / b) / 10);

export function tsvCensus(rows: CensusRow[], repo: string): Census | null {
  const byKey = new Map<string, CensusRow>();
  for (const r of rows) if (r.repo === repo) byKey.set(`${r.path}\t${r.test}`, r);
  const mine = [...byKey.values()];
  if (mine.length === 0) return null;
  const live = mine.filter(active);
  const total = live.reduce((s, r) => s + r.cases, 0);
  const kinds: Record<string, number> = { "1": 0, "2": 0, "3": 0, "4": 0, "5": 0, none: 0 };
  for (const r of live) kinds[/^[1-5]$/.test(r.kind) ? r.kind : "none"] += r.cases;
  const five = total - kinds.none;
  const kept = live.filter((r) => r.action.startsWith("keep") || r.action.startsWith("rejected"));
  const keptCases = kept.reduce((s, r) => s + r.cases, 0);
  const unique = kept.filter(citesUniqueKill).reduce((s, r) => s + r.cases, 0);
  const sources = [...new Set(mine.map((r) => r.source))].sort();
  const source = `${sources.join(" + ")}: ${mine.length} rows, ${live.length} active, ${kept.length} kept`;
  return {
    status: "census",
    tests: { value: total, href: null, source },
    five_kind: { value: share(five, total), href: null, source: `${source}, ${five} of ${total} cases in kinds 1-5` },
    kinds,
    unique_kill: { value: share(unique, keptCases), href: null, source: `${source}, ${unique} of ${keptCases} kept cases cite a unique mutant` },
    sources,
  };
}

export function parseWispTables(comments: { url: string; body: string }[]): Census | null {
  const files = new Map<string, { cases: number; kinds: Record<string, number> }>();
  const sources: string[] = [];
  for (const c of comments) {
    if (!/^## T5 /m.test(c.body)) continue;
    sources.push(c.url);
    let header: string[] | null = null;
    for (const line of c.body.split("\n")) {
      if (!line.startsWith("|")) { header = null; continue; }
      const cells = line.split("|").slice(1, -1).map((s) => s.trim());
      if (!header) { header = cells.map((h) => h.toLowerCase()); continue; }
      if (cells.every((x) => /^-+$/.test(x))) continue;
      const fi = header.findIndex((h) => h === "file"), ci = header.findIndex((h) => h.startsWith("cases")), ki = header.indexOf("kinds");
      if (fi < 0 || ci < 0 || ki < 0) continue;
      const cases = Number((/(\d+)\s*$/.exec(cells[ci]) || [, "0"])[1]);
      const kinds: Record<string, number> = {};
      for (const m of cells[ki].matchAll(/([1-5S])×(\d+)/g)) kinds[m[1] === "S" ? "none" : m[1]] = (kinds[m[1] === "S" ? "none" : m[1]] || 0) + Number(m[2]);
      files.set(cells[fi], { cases, kinds });
    }
  }
  if (files.size === 0) return null;
  const kinds: Record<string, number> = { "1": 0, "2": 0, "3": 0, "4": 0, "5": 0, none: 0 };
  let total = 0;
  for (const f of files.values()) {
    total += f.cases;
    for (const [k, n] of Object.entries(f.kinds)) kinds[k] += n;
  }
  const labelled = Object.values(kinds).reduce((a, b) => a + b, 0);
  const href = sources[0];
  return {
    status: "census",
    tests: { value: total, href },
    five_kind: { value: share(labelled - kinds.none, labelled), href },
    kinds,
    unique_kill: { value: null, href: null },
    sources,
  };
}

export function parseProton(md: string, source: string): Census | null {
  const m = /Test count before \d+, after (\d+)/.exec(md);
  if (!m) return null;
  const src = `${source}: "${m[0]}"`;
  return { status: "census", tests: { value: Number(m[1]), href: null, source: src }, five_kind: { value: null, href: null }, kinds: null, unique_kill: { value: null, href: null }, sources: [source] };
}

export const NO_CENSUS: Census = { status: "no census", tests: { value: null, href: null }, five_kind: { value: null, href: null }, kinds: null, unique_kill: { value: null, href: null }, sources: [] };

export type Verdict = { test: string; verdict: string; reason: string };
export type SpotCheck = { source: string; sampled: number; would_delete: number; verdicts: Verdict[] } | null;

export function spotCheck(json: any, source: string): SpotCheck {
  if (!json) return null;
  const verdicts: Verdict[] = (json.verdicts || []).map((v: any) => ({ test: String(v.test ?? v.path ?? ""), verdict: String(v.verdict ?? ""), reason: String(v.reason ?? "") }));
  return {
    source,
    sampled: Number(json.sampled ?? verdicts.length),
    would_delete: Number(json.would_delete ?? verdicts.filter((v) => /DELETE|REWRITE/i.test(v.verdict)).length),
    verdicts,
  };
}

export type CiRow = { gate: string; budget_s: number | null; budget: Link; wall: Link; sha: string | null; over: boolean | null; debt_to_s?: number };
export type TrendPoint = { day: string; test_files: Link; ci_wall: Link };
export type RepoRow = { repo: Repo; ci: CiRow[]; census: Census; spot_check: SpotCheck; trend: TrendPoint[]; green: boolean; reasons: string[]; debts: string[] };

export function judge(row: Omit<RepoRow, "green" | "reasons" | "debts">): { green: boolean; reasons: string[]; debts: string[] } {
  const reasons: string[] = [];
  const debts: string[] = [];
  if (row.ci.length === 0) reasons.push("no CI gate");
  for (const c of row.ci) {
    if (c.budget_s === null) reasons.push(`${c.gate}: no budget`);
    if (c.wall.value === null) reasons.push(`${c.gate}: no run`);
    if (c.over) (c.debt_to_s !== undefined && (c.wall.value as number) <= c.debt_to_s ? debts : reasons).push(`${c.gate}: ${c.wall.value} s over ${c.budget_s} s`);
  }
  if (row.census.status === "no census") reasons.push("no census");
  else {
    if (row.census.five_kind.value !== null && row.census.five_kind.value < 100) reasons.push(`five-kind ${row.census.five_kind.value}%`);
    if (row.census.unique_kill.value === null && (row.census.tests.value ?? 0) > 0) reasons.push("unique-kill: no census");
  }
  if (!row.spot_check) reasons.push("spot check pending");
  else if (row.spot_check.would_delete > 1) debts.push(`spot check: ${row.spot_check.would_delete} of ${row.spot_check.sampled} would delete`);
  return { green: reasons.length === 0, reasons, debts };
}

export function ciRow(gate: Gate, run: { wall_s: number | null; url: string | null; sha: string; source?: string } | null, budgetSource: string | null): CiRow {
  const wall = run?.wall_s ?? null;
  return {
    gate: gate.name,
    budget_s: gate.budget_s,
    budget: { value: gate.budget_s, href: null, source: budgetSource ?? gate.budget_source ?? undefined },
    wall: { value: wall, href: run?.url ?? null, source: run?.source },
    sha: run?.sha ?? null,
    over: wall === null || gate.budget_s === null ? null : wall > gate.budget_s,
    ...(gate.debt_to_s !== undefined ? { debt_to_s: gate.debt_to_s } : {}),
  };
}

const esc = (s: unknown) => String(s ?? "").replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]!);
const link = (l: Link, text: string) =>
  (l.href ? `<a href="${esc(l.href)}">${esc(text)}</a>` : esc(text)) + (l.source ? `<small class="src" title="${esc(l.source)}"> [${esc(l.source)}]</small>` : "");

function spark(points: (number | null)[], w = 240, h = 36): string {
  const vals = points.filter((v): v is number => v !== null);
  if (vals.length < 2) return `<span class="bad">no history</span>`;
  const lo = Math.min(...vals), hi = Math.max(...vals), span = hi - lo || 1;
  const step = w / (points.length - 1);
  let d = "", pen = false;
  points.forEach((v, i) => {
    if (v === null) { pen = false; return; }
    const y = h - 2 - ((v - lo) / span) * (h - 4);
    d += `${pen ? "L" : "M"}${(i * step).toFixed(1)},${y.toFixed(1)}`;
    pen = true;
  });
  return `<svg viewBox="0 0 ${w} ${h}" width="${w}" height="${h}" role="img"><path d="${d}" fill="none" stroke="currentColor" stroke-width="1.5"/></svg><span class="range">${lo}–${hi}</span>`;
}

export function renderHtml(doc: { generated_at: string; repos: RepoRow[] }): string {
  const rows = doc.repos.map((r) => {
    const ci = r.ci.length
      ? r.ci.map((c) => {
          const cls = c.over === false ? "ok" : c.over && c.debt_to_s !== undefined && (c.wall.value as number) <= c.debt_to_s ? "debt" : "bad";
          const budget = c.budget_s === null ? `<span class="bad">no budget</span>` : link(c.budget, `${c.budget_s} s`);
          return `<div class="${cls}">${esc(c.gate)}: ${link(c.wall, c.wall.value === null ? "no run" : `${c.wall.value} s`)} / ${budget}</div>`;
        }).join("")
      : `<div class="bad">no CI gate</div>`;
    const c = r.census;
    const census = c.status === "no census"
      ? `<span class="bad">no census</span>`
      : `<div>${link(c.tests, `${c.tests.value} tests`)}</div><div class="${c.five_kind.value === 100 ? "ok" : c.five_kind.value === null ? "" : "bad"}">five-kind ${link(c.five_kind, c.five_kind.value === null ? "n/a" : `${c.five_kind.value}%`)}</div>` +
        (c.kinds ? `<div class="kinds">${Object.entries(c.kinds).map(([k, n]) => `K${k}:${n}`).join(" ")}</div>` : "");
    const uk = c.unique_kill.value === null
      ? (c.status === "census" && (c.tests.value ?? 0) === 0 ? "n/a" : `<span class="bad">no census</span>`)
      : link(c.unique_kill, `${c.unique_kill.value}%`);
    const sc = r.spot_check
      ? `<details><summary class="${r.spot_check.would_delete <= 1 ? "ok" : "debt"}">${r.spot_check.would_delete} of ${r.spot_check.sampled} would delete</summary><ul>${r.spot_check.verdicts.map((v) => `<li><b>${esc(v.verdict)}</b> ${esc(v.test)}: ${esc(v.reason)}</li>`).join("")}</ul><small class="src">${esc(r.spot_check.source)}</small></details>`
      : `<span class="bad">pending</span>`;
    const trend = `<details><summary>30 days</summary><div class="spark">tests files ${spark(r.trend.map((t) => t.test_files.value as number | null))}</div><div class="spark">CI wall s ${spark(r.trend.map((t) => t.ci_wall.value as number | null))}</div><table class="days">${r.trend.map((t) => `<tr><td>${t.day}</td><td>${link(t.test_files, String(t.test_files.value ?? "–"))}</td><td>${link(t.ci_wall, t.ci_wall.value === null ? "–" : `${t.ci_wall.value} s`)}</td></tr>`).join("")}</table></details>`;
    return `<section class="repo ${r.green ? "green" : "red"}"><h2>${esc(r.repo)} <span class="${r.green ? "ok" : "bad"}">${r.green ? "green" : "red"}</span></h2>
<div class="grid"><div><h3>CI wall / budget</h3>${ci}</div><div><h3>Tests</h3>${census}</div><div><h3>Unique kill</h3>${uk}</div><div><h3>Spot check</h3>${sc}</div></div>
${r.reasons.length ? `<p class="why">${r.reasons.map(esc).join(" · ")}</p>` : ""}${r.debts.length ? `<p class="why debt">debt: ${r.debts.map(esc).join(" · ")}</p>` : ""}${trend}</section>`;
  }).join("\n");
  return `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Test Health</title>
<style>
:root{--bg:#fbfaf7;--fg:#1d1d1b;--muted:#6b6a65;--card:#fff;--line:#e4e1d9;--ok:#1f7a3a;--bad:#b42318;--debt:#9a6700}
@media (prefers-color-scheme:dark){:root:not([data-theme="light"]){--bg:#151514;--fg:#ecebe6;--muted:#9c9a92;--card:#1f1f1d;--line:#33322e;--ok:#5cc27a;--bad:#ff7b6b;--debt:#e3b341}}
:root[data-theme="dark"]{--bg:#151514;--fg:#ecebe6;--muted:#9c9a92;--card:#1f1f1d;--line:#33322e;--ok:#5cc27a;--bad:#ff7b6b;--debt:#e3b341}
*{box-sizing:border-box}body{margin:0;padding:16px;background:var(--bg);color:var(--fg);font:15px/1.45 system-ui,sans-serif}
main{max-width:980px;margin:0 auto}h1{font-size:22px;margin:0 0 4px}.meta{color:var(--muted);margin:0 0 16px}
.repo{background:var(--card);border:1px solid var(--line);border-left:4px solid var(--bad);border-radius:8px;padding:12px;margin:0 0 12px}.repo.green{border-left-color:var(--ok)}
h2{font-size:17px;margin:0 0 8px}h3{font-size:12px;text-transform:uppercase;letter-spacing:.04em;color:var(--muted);margin:0 0 4px}
.grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(170px,1fr));gap:10px}.ok{color:var(--ok)}.bad{color:var(--bad)}.debt{color:var(--debt)}
a{color:inherit}.kinds,.range,.why{color:var(--muted);font-size:13px}.range{margin-left:6px}details{margin-top:6px}summary{cursor:pointer}
.spark{display:flex;align-items:center;gap:8px;flex-wrap:wrap;font-size:13px}svg{max-width:100%;height:auto}.src{display:block;color:var(--muted);font-size:11px;overflow-wrap:anywhere}.days .src,.days small{display:inline}.days{font-size:12px;border-collapse:collapse}.days td{padding:1px 8px 1px 0}
</style></head><body><main><h1>Test health</h1><p class="meta">Generated ${esc(doc.generated_at)}. Red means over budget past any debt ceiling, no budget, no census, under 100% five-kind, or spot check pending. Debt, such as more than 1 of 10 spot-checked tests that would delete, is a tracked miss that blocks nothing.</p>
${rows}</main></body></html>
`;
}

export function budgetExcerpt(md: string, box: string | null, path: string): string | null {
  if (!box) return null;
  const line = md.split("\n").find((l) => l.startsWith(`- ${box} `));
  return line ? `${path}: "${line.slice(2, 220)}${line.length > 222 ? "…" : ""}"` : null;
}

export function censusFiles(names: string[], repo: string): string[] {
  return names.filter((f) => f === `test-lean-t5-${repo}.tsv` || f === `test-lean-t5-${repo}-telemetry.tsv`).sort();
}
