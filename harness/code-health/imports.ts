import { dirname, join, normalize } from "node:path";
import { ext } from "./files";

const TS_FROM = /(?:^|[\s}])from\s*['"]([^'"]+)['"]/g;
const TS_BARE = /^\s*import\s*['"]([^'"]+)['"]/gm;
const TS_CALL = /\b(?:require|import)\s*\(\s*['"]([^'"]+)['"]\s*\)/g;
const RS_USE = /^\s*(?:pub(?:\([^)]*\))?\s+)?use\s+([^;]+);/gm;
const C_INCLUDE = /^\s*#\s*include\s*"([^"]+)"/gm;

const TS_EXTS = ["", ".ts", ".tsx", ".mts", ".cts", ".js", ".mjs", ".cjs", ".jsx", "/index.ts", "/index.tsx", "/index.js", "/index.mjs"];

export interface Crate { name: string; dir: string }

export interface Resolver { exists(path: string): boolean; crates: Crate[]; all: () => Iterable<string> }

export function cratesOf(paths: Iterable<string>, read: (p: string) => string | undefined): Crate[] {
  const crates: Crate[] = [];
  for (const p of paths) {
    if (p !== "Cargo.toml" && !p.endsWith("/Cargo.toml")) continue;
    try {
      const pkg = (Bun.TOML.parse(read(p) ?? "") as { package?: { name?: string } }).package;
      if (pkg?.name) crates.push({ name: pkg.name.replaceAll("-", "_"), dir: p === "Cargo.toml" ? "" : p.slice(0, -"/Cargo.toml".length) });
    } catch {}
  }
  return crates.sort((a, b) => b.dir.length - a.dir.length);
}

function stripComments(text: string): string {
  return text.replace(/\/\*[\s\S]*?\*\//g, "").replace(/^\s*\/\/.*$/gm, "");
}

function resolveTs(from: string, spec: string, r: Resolver): string | null {
  if (!spec.startsWith(".")) return null;
  const base = normalize(join(dirname(from), spec));
  if (base.startsWith("..")) return null;
  const stems = [base];
  const m = base.match(/^(.*)\.(js|mjs|cjs|jsx)$/);
  if (m) stems.push(m[1]);
  for (const stem of stems) for (const e of TS_EXTS) if (r.exists(stem + e)) return stem + e;
  return null;
}

function expandUse(tree: string): string[][] {
  const out: string[][] = [];
  const walk = (prefix: string[], s: string): void => {
    s = s.trim();
    const brace = s.indexOf("{");
    if (brace === -1) {
      const segs = s.split(/\s+as\s+/)[0].split("::").map((x) => x.trim()).filter((x) => x && x !== "*" && x !== "self");
      out.push([...prefix, ...segs]);
      return;
    }
    const head = s.slice(0, brace).split("::").map((x) => x.trim()).filter(Boolean);
    const inner = s.slice(brace + 1, s.lastIndexOf("}"));
    let depth = 0, start = 0;
    for (let i = 0; i <= inner.length; i++) {
      const c = inner[i];
      if (c === "{") depth++;
      else if (c === "}") depth--;
      else if ((c === "," && depth === 0) || i === inner.length) {
        if (inner.slice(start, i).trim()) walk([...prefix, ...head], inner.slice(start, i));
        start = i + 1;
      }
    }
  };
  walk([], tree);
  return out;
}

function rustModule(path: string, crate: Crate): { root: string; mod: string[] } {
  const rel = crate.dir ? path.slice(crate.dir.length + 1) : path;
  const root = crate.dir ? `${crate.dir}/src` : "src";
  if (!rel.startsWith("src/")) return { root: dirname(path), mod: [] };
  const parts = rel.slice(4, -3).split("/");
  const last = parts[parts.length - 1];
  if (parts.length === 1 && (last === "lib" || last === "main")) return { root, mod: [] };
  if (last === "mod") parts.pop();
  return { root, mod: parts };
}

function rustFile(root: string, mod: string[], r: Resolver): string | null {
  if (mod.length === 0) {
    for (const f of ["lib.rs", "main.rs"]) if (r.exists(`${root}/${f}`)) return `${root}/${f}`;
    return null;
  }
  const p = `${root}/${mod.join("/")}`;
  if (r.exists(`${p}.rs`)) return `${p}.rs`;
  if (r.exists(`${p}/mod.rs`)) return `${p}/mod.rs`;
  return null;
}

function inlineModules(text: string, at: number): string[] {
  const stack: (string | null)[] = [];
  const re = /"(?:[^"\\]|\\.)*"|\bmod\s+(\w+)\s*\{|\{|\}/g;
  for (let m = re.exec(text); m && m.index < at; m = re.exec(text)) {
    if (m[0] === "}") stack.pop();
    else if (m[0] === "{") stack.push(null);
    else if (m[1] !== undefined) stack.push(m[1]);
  }
  return stack.filter((x): x is string => x !== null);
}

function resolveRust(from: string, segs: string[], r: Resolver, inline: string[] = []): string | null {
  const crate = r.crates.find((c) => c.dir === "" || from.startsWith(c.dir + "/"));
  if (!crate) return null;
  const here = rustModule(from, crate);
  let root = here.root;
  let base: string[];
  let fileDepth = 0;
  let rest = segs;
  if (segs[0] === "crate") { base = []; rest = segs.slice(1); root = crate.dir ? `${crate.dir}/src` : "src"; }
  else if (segs[0] === "self") { base = [...here.mod, ...inline]; fileDepth = here.mod.length; rest = segs.slice(1); }
  else if (segs[0] === "super") {
    base = [...here.mod, ...inline];
    fileDepth = here.mod.length;
    while (rest[0] === "super") { base.pop(); fileDepth = Math.min(fileDepth, base.length); rest = rest.slice(1); }
  } else {
    const other = r.crates.find((c) => c.name === segs[0]);
    if (!other) return null;
    root = other.dir ? `${other.dir}/src` : "src";
    base = [];
    rest = segs.slice(1);
  }
  for (let k = rest.length; k >= 0; k--) {
    const f = rustFile(root, [...base, ...rest.slice(0, k)], r);
    if (f) return f;
  }
  return rustFile(root, base.slice(0, fileDepth), r);
}

function resolveC(from: string, spec: string, r: Resolver): string | null {
  const local = normalize(join(dirname(from), spec));
  if (r.exists(local)) return local;
  if (r.exists(spec)) return spec;
  return null;
}

export function importsOf(path: string, text: string, r: Resolver): Set<string> {
  const out = new Set<string>();
  const add = (t: string | null) => { if (t && t !== path) out.add(t); };
  const e = ext(path);
  if (e === "rs") {
    const code = stripComments(text);
    for (const m of code.matchAll(RS_USE)) {
      const inline = inlineModules(code, m.index);
      for (const segs of expandUse(m[1])) add(resolveRust(path, segs, r, inline));
    }
  } else if (e === "c" || e === "h") {
    for (const m of text.matchAll(C_INCLUDE)) add(resolveC(path, m[1], r));
  } else {
    const t = text.replace(/^\s*\/\/.*$/gm, "");
    for (const re of [TS_FROM, TS_BARE, TS_CALL]) for (const m of t.matchAll(re)) add(resolveTs(path, m[1], r));
  }
  return out;
}
