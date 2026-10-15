import { readFileSync } from "node:fs";
import { join } from "node:path";

const source = readFileSync(join(import.meta.dir, "../../../bin/threads"), "utf8");

function list(name: string): string[] {
  const found = source.match(new RegExp(`^${name} = \\[([^\\]]*)\\]`, "m"));
  if (!found) throw new Error(`bin/threads no longer defines ${name} as a list`);
  return [...found[1].matchAll(/"([^"]+)"/g)].map((m) => m[1]);
}

const owner = source.match(/^OWNER = "([^"]+)"/m);
if (!owner) throw new Error("bin/threads no longer defines OWNER");

export const OWNER = owner[1];
export const REPOS = list("REPOS");
export const NATIVE = list("NATIVE");

export function visibleNative(env: Record<string, string | undefined> = process.env): string[] {
  return env.THREADS_NO_NATIVE || env.CLAUDE_CODE_REMOTE ? [] : NATIVE;
}
