import { existsSync, readFileSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { classify, configInWorktree, moduleOf } from "./config";
import { lineCount, Snapshot } from "./files";
import { measure } from "./functions";
import { layerViolation } from "./graph";
import { FIX, fileSize, functionDebt, Violation } from "./gate";
import { cratesOf, importsOf, Resolver } from "./imports";

const CYCLE_SEARCH_LIMIT = 3000;

export function check(repo: string, file: string, override?: string): Violation[] {
  const rel = relative(repo, resolve(file));
  const cfg = configInWorktree(repo, override);
  if (!cfg.present || !existsSync(join(repo, rel))) return [];
  const exists = (p: string) => existsSync(join(repo, p));
  if (classify(cfg, rel, exists) !== "hand") return [];
  const head = new Snapshot(repo, "HEAD");
  const work = readFileSync(join(repo, rel), "utf8");
  const before = head.text(rel);
  const t = cfg.thresholds;
  const v: Violation[] = [];
  const fs = fileSize(rel, lineCount(work), before === undefined ? undefined : lineCount(before), t.file_lines);
  if (fs) v.push(fs);
  const fns = measure({ h: [{ path: rel, text: work }], b: before === undefined ? [] : [{ path: rel, text: before }] });
  v.push(...functionDebt(rel, fns.get(`h/${rel}`)!, fns.get(`b/${rel}`) ?? [], t));

  const read = (p: string) => (exists(p) ? readFileSync(join(repo, p), "utf8") : undefined);
  const r: Resolver = { exists, crates: cratesOf(head.tree.keys(), read), all: () => head.tree.keys() };
  const old = before === undefined ? new Set<string>() : importsOf(rel, before, r);
  const isHand = (p: string) => classify(cfg, p, exists) === "hand";
  for (const to of importsOf(rel, work, r)) {
    if (old.has(to) || !isHand(to)) continue;
    const lv = layerViolation(cfg, rel, to);
    if (lv) v.push({ rule: "layer", path: rel, text: `new import -> ${to}: module ${lv.fromModule} may not use ${lv.toModule} — ${FIX.layer}` });
    const seen = new Set([to]);
    const queue = [to];
    while (queue.length && seen.size < CYCLE_SEARCH_LIMIT) {
      const cur = queue.pop()!;
      if (cur === rel) {
        v.push({ rule: "cycle", path: rel, text: `new import -> ${to} closes an import cycle — ${FIX.cycle}` });
        break;
      }
      for (const n of importsOf(cur, read(cur) ?? "", r)) if (!seen.has(n) && isHand(n)) { seen.add(n); queue.push(n); }
    }
  }
  if (before === undefined && cfg.modules.length && !moduleOf(cfg, rel)) v.push({ rule: "layer", path: rel, text: "new hand source file in no module — add it to a [[module]] paths glob in structure.toml, or move it under one" });
  return v;
}
