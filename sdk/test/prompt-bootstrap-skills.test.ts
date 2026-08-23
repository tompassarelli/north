import { afterEach, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  activeSkillCatalog, canonicalGlobalAgents, harnessCompositionEvidence, harnessOptions,
} from "../src/harness";
import { applyOrchestrationStaffing } from "../src/orchestration-staffing";

const temporary: string[] = [];
const saved = Object.fromEntries([
  "HOME", "AGENT_LAWS", "AGENT_LAWS_PATH", "NORTH_AGENT_SKILLS", "NORTH_AGENT_STATE_ROOT",
  "NORTH_PROJECT", "AGENT_ESO",
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
  process.env.NORTH_AGENT_SKILLS = skills;
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

test("project skill packages compose additively for exact main, worktree, and explicit identities", () => {
  const home = mkdtempSync(join(tmpdir(), "north-project-skills-"));
  temporary.push(home);
  const state = join(home, "state");
  const generation = join(state, "gen-exact");
  const shared = join(generation, "skills", "shared");
  const projectSkills = join(generation, "projects", "beagle", "skill");
  writeSkill(shared, "repo-safety", "Use before repository writes.", "SHARED_BODY_CANARY");
  writeSkill(projectSkills, "code-as-facts", "Use when Beagle code is the evidence.", "PROJECT_BODY_CANARY");
  mkdirSync(state, { recursive: true });
  symlinkSync("gen-exact", join(state, "current"), "dir");
  const main = join(home, "code", "beagle", "main");
  const worktree = join(home, "code", "beagle", "worktrees", "lane");
  const unrelated = join(home, "code", "gjoa", "main");
  const clone = join(home, "ephemeral-clone");
  for (const root of [main, worktree, unrelated, clone])
    mkdirSync(join(root, ".git"), { recursive: true });
  process.env.HOME = home;
  process.env.NORTH_AGENT_STATE_ROOT = state;
  delete process.env.NORTH_AGENT_SKILLS;
  delete process.env.NORTH_PROJECT;
  process.env.AGENT_LAWS = "off";
  process.env.AGENT_ESO = "off";

  for (const cwd of [main, worktree]) {
    const catalog = activeSkillCatalog(process.env, cwd);
    expect(catalog.roots).toEqual([shared, projectSkills]);
    expect(catalog.candidates.map(({ name }) => name)).toEqual(["code-as-facts", "repo-safety"]);
    const options = harnessOptions({ self: `project-${cwd.length}`, cwd, presenceRegistrar: false }) as any;
    expect(options.systemPrompt).toContain(join(projectSkills, "code-as-facts", "SKILL.md"));
    expect(options.systemPrompt).not.toContain("PROJECT_BODY_CANARY");
  }

  const routed = harnessOptions({
    self: "project-domain-route",
    cwd: main,
    presenceRegistrar: false,
    routingMetadata: applyOrchestrationStaffing({
      role: "implementer",
      domainRequirements: ["code-as-facts"],
      composition: {
        kind: "template",
        id: "implementer",
        overrides: ["domainRequirements"],
        overrideReason: "exercise exact project-private domain discovery",
      },
    }),
  }) as any;
  expect(routed.systemPrompt).toContain("### code-as-facts");
  expect(routed.systemPrompt).toContain(join(projectSkills, "code-as-facts", "SKILL.md"));

  expect(activeSkillCatalog(process.env, unrelated).candidates.map(({ name }) => name))
    .toEqual(["repo-safety"]);
  process.env.NORTH_PROJECT = "beagle";
  expect(activeSkillCatalog(process.env, clone).candidates.map(({ name }) => name))
    .toEqual(["code-as-facts", "repo-safety"]);
});

test("project skill UnitIds deduplicate exact sources and reject divergent collisions", () => {
  const home = mkdtempSync(join(tmpdir(), "north-project-skill-collision-"));
  temporary.push(home);
  const state = join(home, "state");
  const generation = join(state, "gen-collision");
  const shared = join(generation, "skills", "shared");
  const projectSkills = join(generation, "projects", "beagle", "skill");
  writeSkill(shared, "same-skill", "Exact same skill.", "SAME_BODY");
  writeSkill(projectSkills, "same-skill", "Exact same skill.", "SAME_BODY");
  mkdirSync(state, { recursive: true });
  symlinkSync("gen-collision", join(state, "current"), "dir");
  const main = join(home, "code", "beagle", "main");
  mkdirSync(join(main, ".git"), { recursive: true });
  process.env.HOME = home;
  process.env.NORTH_AGENT_STATE_ROOT = state;
  delete process.env.NORTH_AGENT_SKILLS;
  delete process.env.NORTH_PROJECT;

  expect(activeSkillCatalog(process.env, main).candidates.map(({ name }) => name))
    .toEqual(["same-skill"]);
  writeSkill(projectSkills, "same-skill", "Divergent project skill.", "DIFFERENT_BODY");
  expect(() => activeSkillCatalog(process.env, main)).toThrow("active skill UnitId collision same-skill");
});
