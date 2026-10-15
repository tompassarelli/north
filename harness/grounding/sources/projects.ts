import { existsSync, statSync } from "node:fs";
import { basename, dirname, join, relative } from "node:path";
import type { Doc } from "../store";
import { ownedMains } from "./_repos";
import { markdownDocs, tracked } from "./policy";

export const COMPLETE = true;
export const REFRESH_SECONDS = 1800;
const SKIP = new Set(["north", "south"]);

export function* iterDocs(_since: number | null): Iterable<Doc> {
  for (const main of ownedMains()) {
    const repo = basename(dirname(main));
    if (SKIP.has(repo)) continue;
    const paths = [...tracked(main, "docs"), join(main, "README.md"), join(main, "AGENTS.md")];
    for (const path of paths) {
      if (!path.endsWith(".md") || !existsSync(path)) continue;
      yield* markdownDocs(path, `${repo}:${relative(main, path)}`, statSync(path).mtimeMs / 1000, false, "projects");
    }
  }
}
