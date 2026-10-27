import { existsSync, readFileSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { classify, configInWorktree, moduleOf, thresholdsOf } from "./config";
import { lineCount, Snapshot } from "./files";
import { measure } from "./functions";
import { layerViolation } from "./graph";
import { FIX, fileSize, functionDebt, Violation } from "./gate";
import { importsOf, manifestsOf, Resolver } from "./imports";

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
  const t = thresholdsOf(cfg, rel);
  const v: Violation[] = [];
  const fs = fileSize(rel, lineCount(work), before === undefined ? undefined : lineCount(before), t.file_lines);
  if (fs) v.push(fs);
  const fns = measure({ h: [{ path: rel, text: work }], b: before === undefined ? [] : [{ path: rel, text: before }] });
  v.push(...functionDebt(rel, fns.get(`h/${rel}`)!, fns.get(`b/${rel}`) ?? [], t));

  const read = (p: string) => (exists(p) ? readFileSync(join(repo, p), "utf8") : undefined);
  const r: Resolver = { exists: { root: repo }, manifests: manifestsOf(head.tree.keys(), read) };
  const imports = (files: string[], text = (p: string) => read(p) ?? "") => importsOf(files.map((path) => ({ path, text: text(path) })), r);
  const old = before === undefined ? new Set<string>() : imports([rel], () => before).get(rel)!;
  const isHand = (p: string) => classify(cfg, p, exists) === "hand";
  for (const to of imports([rel], () => work).get(rel)!) {
    if (old.has(to) || !isHand(to)) continue;
    const lv = layerViolation(cfg, rel, to);
    if (lv) v.push({ rule: "layer", path: rel, text: `new import -> ${to}: module ${lv.fromModule} may not use ${lv.toModule} — ${FIX.layer}` });
    const seen = new Set([to]);
    let frontier = [to];
    while (frontier.length && seen.size < CYCLE_SEARCH_LIMIT) {
      if (frontier.includes(rel)) {
        v.push({ rule: "cycle", path: rel, text: `new import -> ${to} closes an import cycle — ${FIX.cycle}` });
        break;
      }
      frontier = [...imports(frontier).values()].flatMap((ns) => [...ns]).filter((n) => !seen.has(n) && isHand(n) && seen.add(n));
    }
  }
  if (before === undefined && cfg.modules.length && !moduleOf(cfg, rel)) v.push({ rule: "layer", path: rel, text: "new hand source file in no module — add it to a [[module]] paths glob in structure.toml, or move it under one" });
  return v;
}
