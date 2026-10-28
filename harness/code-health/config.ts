import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { gitOk, isSource, Snapshot, ToolError } from "./files";

export interface Thresholds { file_lines: number; function_lines: number; function_complexity: number; hotspot_top: number }
export interface Review { commits: number; hand_lines_added: number; test_lines_changed: number; days: number; hotspot_score: number }
export interface Module { name: string; paths: string[]; responsibility: string; may_use: string[]; globs: Bun.Glob[]; thresholds: Thresholds }
export interface Generated { paths: string[]; producer: string; globs: Bun.Glob[] }

export interface Config {
  present: boolean;
  thresholds: Thresholds;
  review: Review;
  modules: Module[];
  generated: Generated[];
  ignore: Bun.Glob[];
  safePushGenerated: Set<string>;
}

const DEFAULT_THRESHOLDS: Thresholds = { file_lines: 500, function_lines: 80, function_complexity: 15, hotspot_top: 20 };
const DEFAULT_REVIEW: Review = { commits: 100, hand_lines_added: 5000, test_lines_changed: 3000, days: 7, hotspot_score: 0 };

function strings(v: unknown, where: string): string[] {
  if (v === undefined) return [];
  if (!Array.isArray(v) || v.some((s) => typeof s !== "string")) throw new ToolError(`structure.toml: ${where} must be a list of strings`);
  return v as string[];
}

function numbers<T extends object>(defaults: T, v: unknown, where: string): T {
  const out = { ...defaults } as Record<string, number>;
  if (v === undefined) return out as T;
  if (typeof v !== "object" || v === null) throw new ToolError(`structure.toml: [${where}] must be a table`);
  for (const [k, val] of Object.entries(v)) {
    if (!(k in out)) throw new ToolError(`structure.toml: unknown key ${where}.${k}`);
    if (typeof val !== "number" || val < 0) throw new ToolError(`structure.toml: ${where}.${k} must be a non-negative number`);
    out[k] = val;
  }
  return out as T;
}

export function parseConfig(text: string | null, safePushGenerated = new Set<string>()): Config {
  let raw: Record<string, unknown> = {};
  if (text !== null) {
    try {
      raw = Bun.TOML.parse(text) as Record<string, unknown>;
    } catch (e) {
      throw new ToolError(`structure.toml: ${(e as Error).message}`);
    }
  }
  const thresholds = numbers(DEFAULT_THRESHOLDS, raw.thresholds, "thresholds");
  const modules: Module[] = ((raw.module ?? []) as Record<string, unknown>[]).map((m, i) => {
    if (typeof m.name !== "string") throw new ToolError(`structure.toml: module ${i + 1} has no name`);
    const paths = strings(m.paths, `module ${m.name}.paths`);
    return {
      thresholds: numbers(thresholds, m.thresholds, `module ${m.name}.thresholds`),
      name: m.name, paths, responsibility: typeof m.responsibility === "string" ? m.responsibility : "",
      may_use: strings(m.may_use, `module ${m.name}.may_use`), globs: paths.map((p) => new Bun.Glob(p)),
    };
  });
  const names = new Set(modules.map((m) => m.name));
  if (names.size !== modules.length) throw new ToolError("structure.toml: duplicate module name");
  for (const m of modules) for (const u of m.may_use) if (!names.has(u)) throw new ToolError(`structure.toml: module ${m.name} may_use unknown module ${u}`);
  const state = new Map<string, number>();
  const visit = (name: string, path: string[]): void => {
    if (state.get(name) === 2) return;
    if (state.get(name) === 1) throw new ToolError(`structure.toml: may_use cycle ${[...path, name].join(" -> ")}`);
    state.set(name, 1);
    for (const u of modules.find((m) => m.name === name)!.may_use) visit(u, [...path, name]);
    state.set(name, 2);
  };
  for (const m of modules) visit(m.name, []);
  const generated: Generated[] = ((raw.generated ?? []) as Record<string, unknown>[]).map((g, i) => {
    if (typeof g.producer !== "string") throw new ToolError(`structure.toml: generated ${i + 1} has no producer`);
    const paths = strings(g.paths, `generated ${i + 1}.paths`);
    return { paths, producer: g.producer, globs: paths.map((p) => new Bun.Glob(p)) };
  });
  const ignore = strings((raw.ignore as Record<string, unknown> | undefined)?.paths, "ignore.paths").map((p) => new Bun.Glob(p));
  return {
    present: text !== null,
    thresholds,
    review: numbers(DEFAULT_REVIEW, raw.review, "review"),
    modules, generated, ignore, safePushGenerated,
  };
}

function safePushDecls(listing: string | null): Set<string> {
  const out = new Set<string>();
  if (!listing) return out;
  for (const entry of listing.split("\0")) {
    const key = entry.split("\n")[0];
    if (key.startsWith("generated.") && key.endsWith(".input")) out.add(key.slice("generated.".length, -".input".length));
  }
  return out;
}

export function configAtRev(snap: Snapshot, override?: string): Config {
  const decls = safePushDecls(snap.has(".safe-push") ? gitOk(snap.repo, ["config", "--blob", `${snap.rev}:.safe-push`, "--null", "--list"]) : null);
  if (override) return parseConfig(readFileSync(override, "utf8"), decls);
  return parseConfig(snap.text("structure.toml") ?? null, decls);
}

export function configInWorktree(repo: string, override?: string): Config {
  const sp = join(repo, ".safe-push");
  const decls = safePushDecls(existsSync(sp) ? gitOk(repo, ["config", "--file", sp, "--null", "--list"]) : null);
  const file = override ?? join(repo, "structure.toml");
  return parseConfig(existsSync(file) ? readFileSync(file, "utf8") : null, decls);
}

export type Kind = "hand" | "generated" | "ignored" | "other";

export function classify(cfg: Config, path: string, exists: (p: string) => boolean): Kind {
  if (!isSource(path)) return "other";
  if (cfg.ignore.some((g) => g.match(path))) return "ignored";
  if (cfg.safePushGenerated.has(path)) return "generated";
  if (cfg.generated.some((g) => g.globs.some((x) => x.match(path)) && exists(g.producer))) return "generated";
  return "hand";
}

const moduleCache = new WeakMap<Config, Map<string, Module | undefined>>();

export function moduleOf(cfg: Config, path: string): Module | undefined {
  let cache = moduleCache.get(cfg);
  if (!cache) moduleCache.set(cfg, (cache = new Map()));
  if (!cache.has(path)) cache.set(path, cfg.modules.find((m) => m.globs.some((g) => g.match(path))));
  return cache.get(path);
}

export function thresholdsOf(cfg: Config, path: string): Thresholds {
  return moduleOf(cfg, path)?.thresholds ?? cfg.thresholds;
}
