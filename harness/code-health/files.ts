import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";

export const SOURCE_LANG: Record<string, string> = {
  ts: "TypeScript", mts: "TypeScript", cts: "TypeScript",
  tsx: "Tsx",
  js: "JavaScript", mjs: "JavaScript", cjs: "JavaScript", jsx: "JavaScript",
  rs: "Rust",
  c: "C", h: "C",
};

export function ext(path: string): string {
  const base = path.slice(path.lastIndexOf("/") + 1);
  const dot = base.lastIndexOf(".");
  return dot > 0 ? base.slice(dot + 1) : "";
}

export function isSource(path: string): boolean {
  return ext(path) in SOURCE_LANG;
}

export class ToolError extends Error {}

const decoder = new TextDecoder();

export function git(repo: string, args: string[], input?: string | Uint8Array): string {
  const r = Bun.spawnSync(["git", "-C", repo, ...args], { stdin: input === undefined ? "ignore" : Buffer.from(input), stdout: "pipe", stderr: "pipe" });
  if (r.exitCode !== 0) throw new ToolError(`git ${args.join(" ")}: ${decoder.decode(r.stderr).trim()}`);
  return decoder.decode(r.stdout);
}

export function gitOk(repo: string, args: string[]): string | null {
  const r = Bun.spawnSync(["git", "-C", repo, ...args], { stdout: "pipe", stderr: "pipe" });
  return r.exitCode === 0 ? decoder.decode(r.stdout) : null;
}

export function toplevel(dir: string): string {
  return git(dir, ["rev-parse", "--show-toplevel"]).trim();
}

export function lineCount(text: string): number {
  if (text.length === 0) return 0;
  let n = 0;
  for (let i = text.indexOf("\n"); i !== -1; i = text.indexOf("\n", i + 1)) n++;
  return text.endsWith("\n") ? n : n + 1;
}

export class Snapshot {
  readonly tree = new Map<string, string>();
  private cache = new Map<string, string>();
  constructor(readonly repo: string, readonly rev: string) {
    const out = git(repo, ["ls-tree", "-r", "-z", "--full-tree", rev]);
    for (const entry of out.split("\0")) {
      if (!entry) continue;
      const tab = entry.indexOf("\t");
      const [, type, sha] = entry.slice(0, tab).split(" ");
      if (type === "blob") this.tree.set(entry.slice(tab + 1), sha);
    }
  }
  has(path: string): boolean {
    return this.tree.has(path);
  }
  text(path: string): string | undefined {
    if (!this.tree.has(path)) return undefined;
    if (!this.cache.has(path)) this.load([path]);
    return this.cache.get(path);
  }
  load(paths: string[]): void {
    const want = paths.filter((p) => this.tree.has(p) && !this.cache.has(p));
    if (want.length === 0) return;
    const r = Bun.spawnSync(["git", "-C", this.repo, "cat-file", "--batch"], {
      stdin: Buffer.from(want.map((p) => this.tree.get(p)).join("\n") + "\n"), stdout: "pipe", stderr: "pipe",
    });
    if (r.exitCode !== 0) throw new ToolError(`git cat-file: ${decoder.decode(r.stderr).trim()}`);
    const buf = r.stdout;
    let pos = 0;
    for (const path of want) {
      const nl = buf.indexOf(10, pos);
      const size = Number(decoder.decode(buf.subarray(pos, nl)).split(" ")[2]);
      this.cache.set(path, decoder.decode(buf.subarray(nl + 1, nl + 1 + size)));
      pos = nl + 1 + size + 1;
    }
  }
}

export interface Source { path: string; text: string }

export function withTempTree<T>(groups: Record<string, Source[]>, fn: (root: string) => T): T {
  const root = mkdtempSync(join(tmpdir(), "code-health-"));
  try {
    for (const [prefix, files] of Object.entries(groups)) {
      for (const f of files) {
        const out = join(root, prefix, f.path);
        mkdirSync(dirname(out), { recursive: true });
        writeFileSync(out, f.text);
      }
    }
    return fn(root);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}
