import { classify, Config, Kind, moduleOf, thresholdsOf } from "./config";
import { git, lineCount, Snapshot } from "./files";
import { Fn, measure } from "./functions";
import { fan, Graph, layerViolation, LayerViolation, moduleGraph, sccs } from "./graph";
import { allows, Allow, coChange, history, Pair, WINDOW_COMMITS, WINDOW_DAYS } from "./history";
import { importsOf, manifestsOf, Resolver } from "./imports";
import { repoName } from "./state";

export function inventory(snap: Snapshot, cfg: Config): Map<string, Kind> {
  const kinds = new Map<string, Kind>();
  for (const p of snap.tree.keys()) {
    const k = classify(cfg, p, (x) => snap.has(x));
    if (k !== "other") kinds.set(p, k);
  }
  return kinds;
}

export function resolverFor(snap: Snapshot): Resolver {
  return { exists: { tree: [...snap.tree.keys()] }, manifests: manifestsOf(snap.tree.keys(), (p) => snap.text(p)) };
}

export function importGraph(snap: Snapshot, hand: string[], resolver = resolverFor(snap)): Graph {
  snap.load(hand);
  const handSet = new Set(hand);
  const found = importsOf(hand.map((path) => ({ path, text: snap.text(path)! })), resolver);
  return new Map(hand.map((p) => [p, new Set([...found.get(p)!].filter((t) => handSet.has(t)))]));
}

export interface Dist { p50: number; p90: number; p99: number; max: number; n: number }

function dist(values: number[]): Dist {
  const v = [...values].sort((a, b) => a - b);
  const at = (q: number) => (v.length ? v[Math.min(v.length - 1, Math.ceil(q * v.length) - 1)] : 0);
  return { p50: at(0.5), p90: at(0.9), p99: at(0.99), max: v.length ? v[v.length - 1] : 0, n: v.length };
}

export interface FileRow { lines: number; complexity: number; functions: Fn[]; module?: string }
export interface Hotspot { path: string; commits: number; complexity: number; score: number }

export interface Report {
  repo: string; sha: string; at: string;
  thresholds: Config["thresholds"];
  split: Record<"hand" | "generated" | "ignored", { files: number; lines: number }>;
  distributions: { file_lines: Dist; function_lines: Dist; function_complexity: Dist };
  over: { files: number; function_lines: number; function_complexity: number };
  files: Record<string, FileRow>;
  unmapped: string[];
  cycles: string[][];
  layer_violations: LayerViolation[];
  module_graph: Record<string, string[]>;
  fan_in: [string, number][]; fan_out: [string, number][];
  module_fan_in: [string, number][]; module_fan_out: [string, number][];
  co_change: Pair[];
  hotspots: Hotspot[];
  history: { commits: number; days: number; max_commits: number };
  allowed_debt: Allow[];
}

const top = <T>(xs: T[], n: number) => xs.slice(0, n);
const ranked = (m: Map<string, number>) => [...m].filter(([, n]) => n > 0).sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));

export function hotspotsOf(churn: Map<string, number>, complexity: (p: string) => number | undefined): Hotspot[] {
  const out: Hotspot[] = [];
  for (const [path, commits] of churn) {
    const c = complexity(path);
    if (c !== undefined) out.push({ path, commits, complexity: c, score: commits * c });
  }
  return out.sort((a, b) => b.score - a.score || a.path.localeCompare(b.path));
}

export function buildReport(repo: string, rev: string, cfg: Config): Report {
  const snap = new Snapshot(repo, rev);
  const sha = git(repo, ["rev-parse", rev]).trim();
  const kinds = inventory(snap, cfg);
  snap.load([...kinds.keys()]);
  const split = { hand: { files: 0, lines: 0 }, generated: { files: 0, lines: 0 }, ignored: { files: 0, lines: 0 } };
  const hand: string[] = [];
  const lines = new Map<string, number>();
  for (const [p, k] of kinds) {
    const n = lineCount(snap.text(p)!);
    lines.set(p, n);
    split[k as keyof typeof split].files++;
    split[k as keyof typeof split].lines += n;
    if (k === "hand") hand.push(p);
  }
  const fns = measure({ h: hand.map((p) => ({ path: p, text: snap.text(p)! })) });
  const files: Record<string, FileRow> = {};
  for (const p of hand) {
    const f = fns.get(`h/${p}`) ?? [];
    files[p] = { lines: lines.get(p)!, complexity: f.reduce((s, x) => s + x.complexity, 0), functions: f, module: moduleOf(cfg, p)?.name };
  }
  const g = importGraph(snap, hand);
  const layer: LayerViolation[] = [];
  for (const [v, ws] of g) for (const w of ws) { const lv = layerViolation(cfg, v, w); if (lv) layer.push(lv); }
  const mg = moduleGraph(cfg, g);
  const { fanIn, fanOut } = fan(g);
  const mf = fan(mg);
  const h = history(repo, rev);
  const handSet = new Set(hand);
  const allFns = hand.flatMap((p) => files[p].functions);
  const t = cfg.thresholds;
  return {
    repo, sha, at: new Date().toISOString(), thresholds: t, split,
    distributions: {
      file_lines: dist(hand.map((p) => files[p].lines)),
      function_lines: dist(allFns.map((f) => f.lines)),
      function_complexity: dist(allFns.map((f) => f.complexity)),
    },
    over: {
      files: hand.filter((p) => files[p].lines > thresholdsOf(cfg, p).file_lines).length,
      function_lines: hand.reduce((n, p) => n + files[p].functions.filter((f) => f.lines > thresholdsOf(cfg, p).function_lines).length, 0),
      function_complexity: hand.reduce((n, p) => n + files[p].functions.filter((f) => f.complexity > thresholdsOf(cfg, p).function_complexity).length, 0),
    },
    files,
    unmapped: cfg.modules.length ? hand.filter((p) => !files[p].module) : [],
    cycles: sccs(g),
    layer_violations: layer,
    module_graph: Object.fromEntries([...mg].map(([k, v]) => [k, [...v].sort()])),
    fan_in: top(ranked(fanIn), 10), fan_out: top(ranked(fanOut), 10),
    module_fan_in: ranked(mf.fanIn), module_fan_out: ranked(mf.fanOut),
    co_change: top(coChange(h, (p) => handSet.has(p), (p) => (cfg.modules.length ? moduleOf(cfg, p)?.name : undefined)), 10),
    hotspots: top(hotspotsOf(h.churn, (p) => files[p]?.complexity), Math.max(10, t.hotspot_top, ...cfg.modules.map((m) => m.thresholds.hotspot_top))),
    history: { commits: h.commits, days: WINDOW_DAYS, max_commits: WINDOW_COMMITS },
    allowed_debt: allows(repo, ["-n", String(WINDOW_COMMITS), rev]),
  };
}

export function trendRow(r: Report) {
  return {
    at: r.at, sha: r.sha, hand_files: r.split.hand.files, hand_lines: r.split.hand.lines, generated_lines: r.split.generated.lines,
    over: r.over, cycles: r.cycles.length, layer_violations: r.layer_violations.length,
    top_hotspot: r.hotspots[0]?.score ?? 0, allowed_debt: r.allowed_debt.length,
  };
}

export function formatReport(r: Report): string {
  const out: string[] = [];
  const d = (x: Dist) => `p50 ${x.p50}  p90 ${x.p90}  p99 ${x.p99}  max ${x.max}  (n=${x.n})`;
  out.push(`code-health report ${repoName(r.repo)} @ ${r.sha.slice(0, 12)}`);
  out.push("", "Split (files / lines)");
  for (const k of ["hand", "generated", "ignored"] as const) out.push(`  ${k.padEnd(10)} ${String(r.split[k].files).padStart(6)} ${String(r.split[k].lines).padStart(9)}`);
  out.push("", "Distributions");
  out.push(`  file lines        ${d(r.distributions.file_lines)}`);
  out.push(`  function lines    ${d(r.distributions.function_lines)}`);
  out.push(`  complexity        ${d(r.distributions.function_complexity)}`);
  out.push(`  over effective limits  files: ${r.over.files}  function lines: ${r.over.function_lines}  complexity: ${r.over.function_complexity}`);
  out.push("", "Top 10 files by size");
  const files = Object.entries(r.files);
  for (const [p, f] of files.sort((a, b) => b[1].lines - a[1].lines).slice(0, 10)) out.push(`  ${String(f.lines).padStart(6)}  ${p}`);
  out.push("", "Top 10 functions by complexity (complexity, lines)");
  const fns = files.flatMap(([p, f]) => f.functions.map((x) => ({ p, ...x })));
  for (const f of fns.sort((a, b) => b.complexity - a.complexity || b.lines - a.lines).slice(0, 10)) out.push(`  ${String(f.complexity).padStart(4)} ${String(f.lines).padStart(5)}  ${f.p}:${f.line} ${f.name}`);
  out.push("", `Cycles: ${r.cycles.length}`);
  for (const c of r.cycles.slice(0, 10)) out.push(`  [${c.length}] ${c.slice(0, 6).join(" ")}${c.length > 6 ? " ..." : ""}`);
  out.push("", `Layer violations: ${r.layer_violations.length}${r.unmapped.length ? `   unmapped hand files: ${r.unmapped.length}` : ""}`);
  for (const v of r.layer_violations.slice(0, 10)) out.push(`  ${v.fromModule} -> ${v.toModule}  ${v.from} -> ${v.to}`);
  for (const p of r.unmapped.slice(0, 5)) out.push(`  unmapped ${p}`);
  out.push("", "Top fan-in (files)");
  for (const [p, n] of r.fan_in) out.push(`  ${String(n).padStart(4)}  ${p}`);
  out.push("", "Top fan-out (files)");
  for (const [p, n] of r.fan_out) out.push(`  ${String(n).padStart(4)}  ${p}`);
  if (r.module_fan_in.length || r.module_fan_out.length) {
    out.push("", "Module fan-in / fan-out");
    out.push(`  in:  ${r.module_fan_in.map(([m, n]) => `${m}=${n}`).join(" ")}`);
    out.push(`  out: ${r.module_fan_out.map(([m, n]) => `${m}=${n}`).join(" ")}`);
  }
  out.push("", `Top co-change pairs (${r.history.commits} commits, support / confidence)`);
  for (const c of r.co_change) out.push(`  ${String(c.support).padStart(4)} ${c.confidence.toFixed(2)}  ${c.a} <-> ${c.b}`);
  out.push("", "Top 10 hotspots (commits x complexity)");
  for (const h of r.hotspots.slice(0, 10)) out.push(`  ${String(h.score).padStart(7)}  ${String(h.commits).padStart(4)} x ${String(h.complexity).padStart(5)}  ${h.path}`);
  if (r.allowed_debt.length) {
    out.push("", "Allowed debt (Code-health-allow)");
    for (const a of r.allowed_debt) out.push(`  ${a.rule} ${a.path} ${a.commit.slice(0, 10)} ${a.reason}`);
  }
  return out.join("\n");
}
