import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dir, "../..");
const read = (path: string) => readFileSync(join(root, path), "utf8");

test("agent policy has one authority and public docs remain reference-only", () => {
  const profile = read("profiles/tom/AGENTS.md");
  const repo = read("AGENTS.md");
  const north = read("profiles/tom/docs/north.md");
  const protocol = read("profiles/tom/docs/agent-protocol.md");
  const manual = read("docs/operating-manual.md");
  const workflow = read("docs/workflow-map.md");

  expect(`${profile}\n${repo}`).not.toMatch(/(?:read|consult|must).*operating-manual/i);
  expect(protocol).not.toMatch(/→.*workflow-map/);
  expect(`${north}\n${protocol}`).not.toMatch(
    /\/loop\b|(?:north-data\/|\.local\/state\/north\/)facts\.log|~1ms|\bnorth (?:doctor|up)\b/i,
  );
  expect(repo).toContain("reference material, not agent policy");
  expect(protocol).toContain("Neither document defines agent conduct");
  expect(manual).toContain("not agent-conduct policy");
  expect(workflow).toContain("a north repo reference doc, not steering");
  expect(workflow).toMatch(/\| `:7978` \| \*\*telemetry\*\*/);
  expect(workflow).not.toMatch(/`:7978`[^\n]*(?:retired|stranded by design)/i);
});
