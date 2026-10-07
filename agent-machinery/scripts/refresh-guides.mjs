#!/usr/bin/env bun
// Refreshes the vendored harness-authoring guides under
// skills/skill-maintenance/references/guides/, or with --check lists the
// ones older than STALE_DAYS. Only openly licensed sources are vendored;
// link-only sources live in skills/skill-maintenance/references/guides.md.
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";

const STALE_DAYS = 90;
const root = join(import.meta.dir, "..", "skills", "skill-maintenance", "references", "guides");
const manifestPath = join(root, "sources.json");

const SOURCES = [
  { repo: "agentskills/agentskills", path: "docs/specification.mdx", out: "agentskills-specification.md", license: "Apache-2.0" },
  { repo: "agentskills/agentskills", path: "docs/skill-creation/best-practices.mdx", out: "agentskills-best-practices.md", license: "Apache-2.0" },
  { repo: "agentskills/agentskills", path: "docs/skill-creation/optimizing-descriptions.mdx", out: "agentskills-optimizing-descriptions.md", license: "Apache-2.0" },
  { repo: "agentskills/agentskills", path: "docs/skill-creation/evaluating-skills.mdx", out: "agentskills-evaluating-skills.md", license: "Apache-2.0" },
  { repo: "anthropics/skills", path: "skills/skill-creator/SKILL.md", out: "anthropic-skill-creator.md", license: "Apache-2.0" },
  { repo: "openai/openai-cookbook", path: "examples/gpt-5/codex_prompting_guide.ipynb", out: "openai-codex-prompting-guide.md", license: "MIT" },
];

const github = async (url) => {
  const response = await fetch(url, { headers: { "user-agent": "agent-machinery-refresh-guides" } });
  if (!response.ok) throw new Error(`${url}: HTTP ${response.status}`);
  return response;
};

const notebookToMarkdown = (text) =>
  JSON.parse(text)
    .cells.map((cell) => {
      const body = Array.isArray(cell.source) ? cell.source.join("") : cell.source;
      return cell.cell_type === "code" ? "```\n" + body + "\n```" : body;
    })
    .join("\n\n");

const ageDays = (iso) => Math.floor((Date.now() - Date.parse(iso)) / 86_400_000);

if (process.argv.includes("--check")) {
  const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
  const stale = manifest.filter((entry) => ageDays(entry.retrieved) > STALE_DAYS);
  for (const entry of manifest) {
    console.log(`${String(ageDays(entry.retrieved)).padStart(4)} days  ${entry.out}`);
  }
  if (stale.length) {
    console.log(`${stale.length} guide(s) older than ${STALE_DAYS} days: run \`bun run refresh:guides\`, then re-check the harness against them.`);
    process.exit(1);
  }
  process.exit(0);
}

const retrieved = new Date().toISOString().slice(0, 10);
const manifest = [];
for (const source of SOURCES) {
  const commit = await (await github(`https://api.github.com/repos/${source.repo}/commits/HEAD`)).json();
  const raw = await (await github(`https://raw.githubusercontent.com/${source.repo}/${commit.sha}/${source.path}`)).text();
  const body = source.path.endsWith(".ipynb") ? notebookToMarkdown(raw) : raw;
  const url = `https://github.com/${source.repo}/blob/${commit.sha}/${source.path}`;
  const header = `<!-- Vendored from ${url} (${source.license}), retrieved ${retrieved}. Refresh with \`bun run refresh:guides\`; do not edit. -->\n\n`;
  const out = join(root, source.out);
  await mkdir(dirname(out), { recursive: true });
  await writeFile(out, header + body.trimEnd() + "\n");
  manifest.push({ out: source.out, url, license: source.license, retrieved });
  console.log(`${source.out} ← ${source.repo}@${commit.sha.slice(0, 12)}`);
}
await writeFile(manifestPath, JSON.stringify(manifest, null, 2) + "\n");
