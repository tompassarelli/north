import { afterEach, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  activeSkillCatalog, canonicalGlobalAgents, harnessCompositionEvidence, harnessOptions,
} from "../src/harness";

const temporary: string[] = [];
const saved = Object.fromEntries([
  "HOME", "AGENT_LAWS", "AGENT_LAWS_PATH", "AGENT_SKILLS_DIR", "AGENT_ESO",
].map((key) => [key, process.env[key]]));

afterEach(() => {
  for (const [key, value] of Object.entries(saved)) {
    if (value === undefined) delete process.env[key];
    else process.env[key] = value;
  }
  for (const path of temporary.splice(0)) rmSync(path, { recursive: true, force: true });
});

function writeSkill(root: string, name: string, description: string, bodyCanary: string): void {
  const directory = join(root, name);
  mkdirSync(directory, { recursive: true });
  writeFileSync(join(directory, "SKILL.md"), [
    "---",
    `name: ${name}`,
    "description: >-",
    ...description.split("\n").map((line) => `  ${line}`),
    "---",
    `# ${name}`,
    bodyCanary,
    "",
  ].join("\n"));
}

test("North composes one canonical bootstrap and a metadata-only active skill catalog", () => {
  const home = mkdtempSync(join(tmpdir(), "north-bootstrap-skills-"));
  temporary.push(home);
  const laws = join(home, ".agents", "AGENTS.md");
  const skills = join(home, "active-skills");
  const project = join(home, "project");
  mkdirSync(join(home, ".agents"), { recursive: true });
  mkdirSync(join(project, ".git"), { recursive: true });
  const bootstrap = [
    "# Canonical bootstrap",
    "BOOTSTRAP_IDENTITY_CANARY_92731",
    "",
    "## Push freely — a heading is ordinary source text",
    "<!-- north-section: obsolete-tag · bucket: write -->",
    "BOOTSTRAP_HEADING_BODY_CANARY_92731",
    "",
  ].join("\n");
  writeFileSync(laws, bootstrap);
  writeFileSync(join(project, "AGENTS.md"), "PROJECT_AGENTS_REACHABLE_92731\n");
  writeSkill(
    skills,
    "external-code",
    "Use whenever reading or adapting code from outside the current repository.",
    "EXTERNAL_SKILL_BODY_MUST_NOT_BE_INJECTED_92731",
  );
  writeSkill(
    skills,
    "repo-safety",
    "Use before editing, committing, or pushing in any repository.",
    "REPO_SKILL_BODY_MUST_NOT_BE_INJECTED_92731",
  );
  process.env.HOME = home;
  process.env.AGENT_LAWS = "on";
  process.env.AGENT_LAWS_PATH = laws;
  process.env.AGENT_SKILLS_DIR = skills;
  process.env.AGENT_ESO = "off";

  const catalog = activeSkillCatalog();
  expect(catalog.candidates.map(({ name }) => name)).toEqual(["external-code", "repo-safety"]);
  expect(catalog.appendix).toContain("Use whenever reading or adapting code from outside");
  expect(catalog.appendix).toContain(join(skills, "external-code", "SKILL.md"));
  expect(catalog.appendix).not.toContain("SKILL_BODY_MUST_NOT_BE_INJECTED");
  expect(canonicalGlobalAgents()!.text).toBe(bootstrap);

  const anthropic = harnessOptions({
    self: "bootstrap-anthropic", provider: "anthropic", cwd: project, presenceRegistrar: false,
  }) as any;
  const openai = harnessOptions({
    self: "bootstrap-openai", provider: "openai", cwd: project, presenceRegistrar: false,
  }) as any;
  expect(anthropic.systemPrompt.match(/BOOTSTRAP_IDENTITY_CANARY_92731/g)).toHaveLength(1);
  expect(anthropic.systemPrompt).toContain(bootstrap);
  expect(() => harnessOptions({
    self: "bootstrap-duplicate", provider: "anthropic", cwd: project, presenceRegistrar: false,
    systemPrompt: bootstrap.trim(),
  })).toThrow("Anthropic global AGENTS bootstrap expected exactly once, observed 2");
  expect(openai.systemPrompt).not.toContain("BOOTSTRAP_IDENTITY_CANARY_92731");
  for (const options of [anthropic, openai]) {
    expect(options.systemPrompt).toContain(catalog.appendix);
    expect(options.systemPrompt).toContain("PROJECT_AGENTS_REACHABLE_92731");
    expect(options.systemPrompt).not.toContain("SKILL_BODY_MUST_NOT_BE_INJECTED");
    expect(options.systemPrompt).not.toContain("Global laws — capability-gated");
    expect(options.systemPrompt).not.toContain("Global laws — repo-gated");
    expect(options.systemPrompt).not.toContain("full guard rides with write capability");
    expect(options.settings).toMatchObject({ autoCompactEnabled: true });
  }

  const evidence = harnessCompositionEvidence(anthropic)!;
  const coordination = anthropic.systemPrompt.indexOf('You are agent "bootstrap-anthropic"');
  expect(coordination).toBeGreaterThan(anthropic.systemPrompt.indexOf(catalog.appendix));
  expect(coordination).toBeGreaterThan(anthropic.systemPrompt.indexOf("PROJECT_AGENTS_REACHABLE_92731"));
  expect(evidence.promptEconomics!.stablePrefixBytes + evidence.promptEconomics!.uniqueTailBytes)
    .toBe(evidence.promptEconomics!.totalBytes);
  expect(evidence.environmentReceipt?.counts.availableSkills).toBe(2);
  const moduleIds = evidence.promptReceipt!.modules.map(({ id }) => id);
  expect(moduleIds).toContain("global-bootstrap");
  expect(moduleIds).toContain("active-skill-catalog");
  expect(moduleIds.some((id) => id.startsWith("constitution-"))).toBe(false);
});
