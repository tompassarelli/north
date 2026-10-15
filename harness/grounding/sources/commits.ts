import { basename, dirname } from "node:path";
import type { Doc } from "../store";
import { ownedMains } from "./_repos";

export const RETAIN_DAYS = 90;
export const REFRESH_SECONDS = 900;
const MARK = "\x1e";

export function* iterDocs(since: number | null): Iterable<Doc> {
  const floor = Math.floor(Math.max(since ?? 0, Date.now() / 1000 - RETAIN_DAYS * 86400));
  const seen = new Set<string>();
  for (const main of ownedMains()) {
    const repo = basename(dirname(main));
    const out = Bun.spawnSync(["git", "-C", main, "log", `--since=@${floor}`, `--format=${MARK}%H %ct %s`, "--name-only", "HEAD"],
      { stdout: "pipe", stderr: "ignore" });
    if (!out.success) continue;
    for (const chunk of out.stdout.toString().split(MARK).slice(1)) {
      const lines = chunk.split("\n").filter((l) => l.trim());
      if (!lines.length) continue;
      const [sha, at, ...subject] = lines[0].split(" ");
      if (seen.has(`${repo}@${sha}`)) continue;
      seen.add(`${repo}@${sha}`);
      yield { source: "commits", id: `${repo}@${sha}`, ref: `${repo}@${sha.slice(0, 8)}`,
        title: `landed ${repo}@${sha.slice(0, 8)}: ${subject.join(" ")}`, text: lines.slice(1, 61).join("\n"), at: Number(at), decays: true };
    }
  }
}
