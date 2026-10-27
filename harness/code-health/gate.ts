import { classify, Config, configAtRev, moduleOf, thresholdsOf } from "./config";
import { git, lineCount, Snapshot } from "./files";
import { Fn, measure } from "./functions";
import { componentOf, layerViolation, sccs } from "./graph";
import { allows, churnAt } from "./history";
import { importsOf } from "./imports";
import { hotspotsOf, importGraph, inventory, resolverFor } from "./report";
import { readBaseline } from "./state";

export interface Violation { rule: string; path: string; line?: number; text: string }

export const FIX = {
  cycle: "move the import behind an interface the other file already owns, or merge the two files",
  layer: "move the import behind the allowed module's interface, or change may_use/paths in structure.toml in this change",
  file: "split the file, or raise thresholds.file_lines in structure.toml in this change",
  fn: "split the function, or raise thresholds.function_lines/function_complexity in structure.toml in this change",
  hotspot: "split complexity out of this hotspot, or add 'Code-health-allow: hotspot <path> — <reason>'",
  generated: "restore the producer, or remove the [[generated]] claim in structure.toml",
};

export function fileSize(path: string, head: number, base: number | undefined, limit: number): Violation | null {
  if (head <= limit || (base !== undefined && base > limit)) return null;
  const was = base === undefined ? "new" : `was ${base}`;
  return { rule: "file-size", path, text: `${head} lines > ${limit} (${was}) — ${FIX.file}` };
}

export function functionDebt(path: string, head: Fn[], base: Fn[], t: Config["thresholds"]): Violation[] {
  const before = new Map(base.map((f) => [f.name, f]));
  const out: Violation[] = [];
  for (const f of head) {
    const overL = f.lines > t.function_lines, overC = f.complexity > t.function_complexity;
    if (!overL && !overC) continue;
    const b = before.get(f.name);
    const wasOk = b && b.lines <= t.function_lines && b.complexity <= t.function_complexity;
    if (b && !wasOk) continue;
    const was = b ? `was ${b.lines} lines, complexity ${b.complexity}` : "new";
    out.push({ rule: "function", path, line: f.line, text: `${f.name} ${f.lines} lines (limit ${t.function_lines}), complexity ${f.complexity} (limit ${t.function_complexity}) (${was}) — ${FIX.fn}` });
  }
  return out;
}

export function formatViolation(v: Violation): string {
  return `${v.rule} ${v.path}${v.line ? `:${v.line}` : ""} ${v.text}`;
}

interface Change { status: string; path: string; base?: string }

function changes(repo: string, base: string, head: string): Change[] {
  const parts = git(repo, ["diff", "-M", "--name-status", "-z", base, head]).split("\0");
  const out: Change[] = [];
  for (let i = 0; i < parts.length - 1; ) {
    const status = parts[i++];
    if (status[0] === "R" || status[0] === "C") {
      const from = parts[i++], to = parts[i++];
      out.push({ status: status[0], path: to, base: status[0] === "R" ? from : undefined });
    } else {
      const p = parts[i++];
      if (status[0] !== "D") out.push({ status: status[0], path: p, base: status[0] === "A" ? undefined : p });
    }
  }
  return out;
}

export interface GateResult { violations: Violation[]; warning?: string }

export function gate(repo: string, base: string, head: string, override?: string): GateResult {
  const hs = new Snapshot(repo, head), bs = new Snapshot(repo, base);
  const cfg = configAtRev(hs, override);
  if (!cfg.present) return { violations: [], warning: "code-health: no structure.toml at HEAD; gate skipped" };
  const baseCfg = configAtRev(bs, override);
  const headKinds = inventory(hs, cfg);
  const changed = changes(repo, base, head).filter((c) => headKinds.get(c.path) === "hand");
  const v: Violation[] = [];

  for (const g of cfg.generated) {
    if (hs.has(g.producer)) continue;
    const claimedBefore = baseCfg.generated.some((b) => b.producer === g.producer && bs.has(g.producer));
    const isNew = !baseCfg.generated.some((b) => b.producer === g.producer);
    if (claimedBefore || isNew) v.push({ rule: "generated", path: g.producer, text: `producer missing at HEAD; ${g.paths.join(", ")} lose the generated exemption — ${FIX.generated}` });
  }
  if (changed.length === 0) return { violations: v };

  hs.load(changed.map((c) => c.path));
  bs.load(changed.flatMap((c) => (c.base ? [c.base] : [])));
  const headLines = new Map<string, number>(), baseLines = new Map<string, number | undefined>();
  for (const c of changed) {
    headLines.set(c.path, lineCount(hs.text(c.path)!));
    const bt = c.base ? bs.text(c.base) : undefined;
    baseLines.set(c.path, bt === undefined ? undefined : lineCount(bt));
    const fs = fileSize(c.path, headLines.get(c.path)!, baseLines.get(c.path), thresholdsOf(cfg, c.path).file_lines);
    if (fs) v.push(fs);
  }

  const fns = measure({
    h: changed.map((c) => ({ path: c.path, text: hs.text(c.path)! })),
    b: changed.flatMap((c) => (c.base && bs.has(c.base) ? [{ path: c.path, text: bs.text(c.base)! }] : [])),
  });
  const cx = (fs: Fn[] | undefined) => fs?.reduce((s, f) => s + f.complexity, 0);
  for (const c of changed) v.push(...functionDebt(c.path, fns.get(`h/${c.path}`)!, fns.get(`b/${c.path}`) ?? [], thresholdsOf(cfg, c.path)));
  const hand = [...headKinds].filter(([, k]) => k === "hand").map(([p]) => p);
  const renamed = new Map<string, string>();
  for (const c of changed) if (c.base && c.base !== c.path) renamed.set(c.base, c.path);
  const headResolver = resolverFor(hs);
  const added: [string, string][] = [];
  const baseImports = importsOf(changed.flatMap((c) => (c.base && bs.has(c.base) ? [{ path: c.base, text: bs.text(c.base)! }] : [])), resolverFor(bs));
  const headImports = importsOf(changed.map((c) => ({ path: c.path, text: hs.text(c.path)! })), headResolver);
  for (const c of changed) {
    const before = c.base && bs.has(c.base) ? new Set([...baseImports.get(c.base)!].map((x) => renamed.get(x) ?? x)) : new Set<string>();
    for (const to of headImports.get(c.path)!) {
      if (before.has(to) || headKinds.get(to) !== "hand") continue;
      added.push([c.path, to]);
      const lv = layerViolation(cfg, c.path, to);
      if (lv) v.push({ rule: "layer", path: c.path, text: `new import -> ${to}: module ${lv.fromModule} may not use ${lv.toModule} — ${FIX.layer}` });
    }
    if (!c.base && cfg.modules.length && !moduleOf(cfg, c.path)) v.push({ rule: "layer", path: c.path, text: `new hand source file in no module — add it to a [[module]] paths glob in structure.toml, or move it under one` });
  }

  if (added.length) {
    const comp = componentOf(sccs(importGraph(hs, hand, headResolver)));
    for (const [path, to] of added) {
      const k = comp.get(path);
      if (k !== undefined && k === comp.get(to)) v.push({ rule: "cycle", path, text: `new import -> ${to} closes an import cycle — ${FIX.cycle}` });
    }
  }

  const rising = changed.filter((c) => c.base && bs.has(c.base) && (cx(fns.get(`h/${c.path}`)) ?? 0) > (cx(fns.get(`b/${c.path}`)) ?? 0));
  if (rising.length) {
    const baseline = readBaseline<{ files?: Record<string, { complexity: number }> }>(repo);
    const measured = new Map(changed.flatMap((c) => (c.base ? [[c.base, cx(fns.get(`b/${c.path}`)) ?? 0] as const] : [])));
    const baseHand = inventory(bs, baseCfg);
    const complexity = (p: string) => (baseHand.get(p) !== "hand" ? undefined : baseline?.files ? (measured.get(p) ?? baseline.files[p]?.complexity) : 1);
    const ranked = hotspotsOf(churnAt(repo, base), complexity);
    for (const c of rising) {
      const limit = thresholdsOf(cfg, c.path).hotspot_top;
      if (!ranked.slice(0, limit).some((h) => h.path === c.base)) continue;
      v.push({ rule: "hotspot", path: c.path, text: `complexity ${cx(fns.get(`h/${c.path}`))} > ${cx(fns.get(`b/${c.path}`))} in a top-${limit} hotspot — ${FIX.hotspot}` });
    }
  }

  const allowed = allows(repo, [`${base}..${head}`]);
  const kept = v.filter((x) => !(["file-size", "function", "hotspot"].includes(x.rule) && allowed.some((a) => a.rule === x.rule && a.path === x.path)));
  return { violations: kept };
}
