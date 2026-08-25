import { expect, test } from "bun:test";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dir, "../..");
const read = (path: string) => readFileSync(join(root, path), "utf8");

test("North repo policy remains repository-scoped and public docs stay reference-only", () => {
  const repo = read("AGENTS.md");
  const manual = read("docs/operating-manual.md");
  const workflow = read("docs/workflow-map.md");

  expect(repo).not.toMatch(/(?:read|consult|must).*operating-manual/i);
  expect(existsSync(join(root, "profiles/tom/AGENTS.md"))).toBe(false);
  expect(repo).toContain("reference material, not agent policy");
  expect(manual).toContain("not agent-conduct policy");
  expect(workflow).toContain("a north repo reference doc, not steering");
  expect(workflow).toMatch(/\| `:7978` \| \*\*telemetry\*\*/);
  expect(workflow).not.toMatch(/`:7978`[^\n]*(?:retired|stranded by design)/i);
});

test("North resources do not claim intentional actor decisions", () => {
  const resources = [
    "AGENTS.md",
    "README.md",
    "cli/dispatch-mode.clj",
    "cli/message-routing.clj",
    "cli/north-listen.clj",
    "coordination/README.md",
    "coordination/assignments-distilled/SKILL.md",
    "coordination/guide.md",
    "docs/INFLUENCES.md",
    "docs/architecture.md",
    "docs/dispatch-interface.md",
    "docs/learning-regime.md",
    "docs/operating-manual.md",
    "docs/provider-architecture.md",
    "docs/workflow-map.md",
  ]
    .map(read)
    .join("\n");

  expect(resources).not.toMatch(
    /\b(?:North|Orchestration|Coordination|staffing|harness|system|AUTO)\s+(?:answers?|chooses?|decides?|decomposes?|orchestrates?|accepts?|makes the routing decision|owns (?:semantic task routing|reduction|(?:its )?children))\b/i,
  );
});
