import { appendFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { basename, join } from "node:path";
import { gitOk } from "./files";

export function repoName(repo: string): string {
  const code = join(homedir(), "code") + "/";
  if (repo.startsWith(code)) return repo.slice(code.length).split("/")[0];
  const url = gitOk(repo, ["remote", "get-url", "origin"])?.trim();
  if (url) return basename(url).replace(/\.git$/, "");
  return basename(repo);
}

export function stateDir(repo: string): string {
  const root = process.env.XDG_STATE_HOME || join(homedir(), ".local", "state");
  return join(root, "code-health", repoName(repo));
}

function readJson<T>(path: string): T | null {
  return existsSync(path) ? (JSON.parse(readFileSync(path, "utf8")) as T) : null;
}

function writeJson(path: string, value: unknown): void {
  mkdirSync(join(path, ".."), { recursive: true });
  writeFileSync(path, JSON.stringify(value, null, 2) + "\n");
}

export interface Marker { sha: string; at: string; hand_lines: number }

export const baselinePath = (repo: string) => join(stateDir(repo), "baseline.json");
export const readBaseline = <T>(repo: string) => readJson<T>(baselinePath(repo));
export const readMarker = (repo: string) => readJson<Marker>(join(stateDir(repo), "review.json"));
export const writeMarker = (repo: string, m: Marker) => writeJson(join(stateDir(repo), "review.json"), m);

export function save(repo: string, baseline: unknown, trendRow: unknown): void {
  writeJson(baselinePath(repo), baseline);
  appendFileSync(join(stateDir(repo), "trend.jsonl"), JSON.stringify(trendRow) + "\n");
}
