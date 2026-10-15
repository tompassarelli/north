import { OWNER, REPOS } from "./_threads.ts";

import type { Doc } from "../store";
type Node = { number: number; title: string; body: string | null; state: string; updatedAt: string; labels: { nodes: { name: string }[] } };
type Page = { pageInfo: { hasNextPage: boolean; endCursor: string | null }; nodes: Node[] };
export type Gh = (args: string[]) => Promise<string>;

export const REFRESH_SECONDS = 1800;
export const LIMIT = 2000;
const FIELDS = "pageInfo { hasNextPage endCursor } nodes { number title body state updatedAt labels(first: 20) { nodes { name } } }";

export function query(repos: string[]): string {
  const params = repos.map((_, i) => `, $c${i}: String`).join("");
  const parts = repos.map((repo, i) =>
    ` r${i}: repository(owner: $owner, name: "${repo}") { issues(first: 100, after: $c${i},` +
    ` orderBy: {field: UPDATED_AT, direction: ASC}, filterBy: {since: $since}) { ${FIELDS} } }`).join("");
  return `query($owner: String!, $since: DateTime${params}) {${parts} }`;
}

const iso = (at: number) => new Date(at * 1000).toISOString().replace(/\.\d+Z$/, "Z");

export async function gh(args: string[], sleep = Bun.sleep): Promise<string> {
  let delay = 30, waited = 0;
  for (;;) {
    const proc = Bun.spawn(["gh", ...args], { stdout: "pipe", stderr: "pipe", stdin: "ignore" });
    const [out, err, code] = await Promise.all([new Response(proc.stdout).text(), new Response(proc.stderr).text(), proc.exited]);
    if (code === 0) return out;
    if (!err.toLowerCase().includes("rate limit") || waited > 65 * 60) throw new Error(`gh ${args[0]} ${args[1] ?? ""}: ${err.trim()}`);
    const pause = delay * (0.8 + Math.random() * 0.4);
    console.error(`grounding github: GitHub rate limit, waiting ${pause.toFixed(0)} s`);
    await sleep(pause * 1000);
    waited += pause;
    delay = Math.min(delay * 2, 300);
  }
}

function document(repo: string, node: Node): Doc {
  const labels = node.labels.nodes.map((l) => l.name).join(", ");
  const text = [node.title, `state: ${node.state.toLowerCase()}`, labels ? `labels: ${labels}` : "", node.body ?? ""]
    .filter(Boolean).join("\n");
  const item = `${OWNER}/${repo}#${node.number}`;
  return { source: "github", id: item, ref: `github ${item}`, title: node.title,
    text: text.length <= LIMIT ? text : `${text.slice(0, LIMIT - 1)}…`,
    at: Date.parse(node.updatedAt) / 1000, decays: node.state !== "OPEN" };
}

export async function* iterDocs(since: number | null, run: Gh = gh): AsyncIterable<Doc> {
  const cursors = new Map<string, string | null>(REPOS.map((repo) => [repo, null]));
  while (cursors.size) {
    const repos = [...cursors.keys()];
    const args = ["api", "graphql", "-f", `query=${query(repos)}`, "-F", `owner=${OWNER}`];
    if (since !== null) args.push("-F", `since=${iso(since)}`);
    repos.forEach((repo, i) => { if (cursors.get(repo)) args.push("-F", `c${i}=${cursors.get(repo)}`); });
    const data = JSON.parse(await run(args)).data as Record<string, { issues: Page }>;
    for (const [i, repo] of repos.entries()) {
      const page = data[`r${i}`].issues;
      for (const node of page.nodes) {
        if (since === null || Date.parse(node.updatedAt) / 1000 > since) yield document(repo, node);
      }
      if (page.pageInfo.hasNextPage) cursors.set(repo, page.pageInfo.endCursor);
      else cursors.delete(repo);
    }
  }
}
