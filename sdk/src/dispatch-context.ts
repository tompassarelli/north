import { realpathSync, statSync } from "node:fs";
import { homedir } from "node:os";
import { isAbsolute, join, relative, resolve, sep } from "node:path";
import type { Fact } from "./north-client";

export interface DispatchContextOptions {
  cwd?: string;
  home?: string;
}

function isWithin(parent: string, child: string): boolean {
  const rel = relative(parent, child);
  return rel === "" || (!rel.startsWith(`..${sep}`) && rel !== ".." && !isAbsolute(rel));
}

function canonicalDirectory(
  input: string,
  home: string,
  label: string,
  requireWithinHome = false,
): string {
  const value = input.trim();
  if (!value) throw new Error(`${label} is empty`);
  if (value.includes("\0")) throw new Error(`${label} contains a NUL byte`);

  const homeAnchored = value === "~" || value.startsWith("~/");
  if (value.startsWith("~") && !homeAnchored)
    throw new Error(`${label} uses an unsupported user-home expansion: ${value}`);
  const expanded = value === "~" ? home : value.startsWith("~/") ? join(home, value.slice(2)) : value;
  if (!isAbsolute(expanded))
    throw new Error(`${label} must be absolute or ~-anchored: ${value}`);

  let canonical: string;
  try {
    canonical = realpathSync(resolve(expanded));
  } catch {
    throw new Error(`${label} does not resolve to an existing path: ${value}`);
  }
  if (!statSync(canonical).isDirectory())
    throw new Error(`${label} is not a directory: ${value}`);
  // Repo facts are executable authority: the worker receives edit + shell
  // tools rooted here. Absolute spellings must obey the same containment rule
  // as `~/...`; otherwise `/` or `/tmp/...` bypasses the only trust boundary.
  if ((homeAnchored || requireWithinHome) && !isWithin(realpathSync(home), canonical))
    throw new Error(`${label} escapes the home directory: ${value}`);
  return canonical;
}

/**
 * Resolve the repository in which a dispatched thread must execute.
 *
 * A single repo fact is authoritative. For a multi-repository thread, the
 * caller's current repository may disambiguate; otherwise dispatch fails
 * before provider selection instead of silently executing in an unrelated cwd.
 */
export function resolveDispatchWorkingDirectory(
  facts: Fact[],
  options: DispatchContextOptions = {},
): string {
  const home = realpathSync(resolve(options.home ?? process.env.HOME ?? homedir()));
  const current = canonicalDirectory(options.cwd ?? process.cwd(), home, "dispatch cwd");
  const values = [...new Set(facts
    .filter(({ predicate }) => predicate === "repo")
    .map(({ value }) => value.trim()))];
  if (!values.length) return current;

  const repositories = [...new Map(values.map((value) => {
    const canonical = canonicalDirectory(value, home, "thread repo", true);
    return [canonical, { canonical, value }] as const;
  })).values()];
  if (repositories.length === 1) return repositories[0].canonical;

  const matching = repositories
    .filter(({ canonical }) => isWithin(canonical, current))
    .sort((left, right) => right.canonical.length - left.canonical.length);
  if (matching.length) return matching[0].canonical;

  throw new Error(
    `thread has multiple repository facts and cwd does not disambiguate them: ${values.join(", ")}`,
  );
}
