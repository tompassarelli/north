import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join, resolve } from "node:path";

export type CavemanMode = "off" | "lite" | "full";
export type CavemanSource = "request" | "env" | "default";

const REPOSITORY = "github.com/tompassarelli/caveman";
const CANONICAL_REVISION = "020f650daa42a506660a2959f62f2a999d7e1018";
const CANONICAL_SKILL_SHA256 = "e38ec671ecbee47ce234190be12615daf60ac667d775b7340d49d07f4f63c7bc";
const SKILL_PATH = "skills/caveman/SKILL.md";

export interface CavemanResolution {
  requestedMode: CavemanMode;
  resolvedMode: CavemanMode;
  source: CavemanSource;
  decisionReason: "explicit-request" | "inherited-env" | "default-off-unproven-savings";
  implementation: "disabled" | "fork-skill";
  instructions: string;
  repository?: string;
  revision?: string;
  skillSha256?: string;
  skillBytes?: number;
  renderedSha256?: string;
  renderedBytes?: number;
  sourceKind?: "git-object" | "immutable-file";
  resolutionProvenance?: "explicit" | "local-dev";
  /** No before/after provider-context measurement exists at this seam. */
  measurementCoverage: "exact" | "unknown";
}

function sha256(value: string): string {
  return createHash("sha256").update(value).digest("hex");
}

function mode(value: unknown): CavemanMode | undefined {
  return value === "off" || value === "lite" || value === "full" ? value : undefined;
}

function normalizeOrigin(value: string): string | undefined {
  const normalized = value.trim().replace(/^git@github\.com:/, "github.com/")
    .replace(/^https?:\/\//, "").replace(/^ssh:\/\/git@/, "")
    .replace(/\.git\/?$/, "").replace(/\/$/, "").toLowerCase();
  return normalized === REPOSITORY ? REPOSITORY : undefined;
}

function git(home: string, args: string[]): string {
  return execFileSync("git", ["-C", home, ...args], {
    encoding: "utf8", stdio: ["ignore", "pipe", "ignore"], maxBuffer: 2 * 1024 * 1024,
  });
}

function artifact(env: NodeJS.ProcessEnv): {
  content: string;
  revision: string;
  sourceKind: "git-object" | "immutable-file";
  provenance: "explicit" | "local-dev";
} {
  const explicitHome = env.NORTH_CAVEMAN_HOME?.trim();
  const explicitRevision = env.NORTH_CAVEMAN_REV?.trim();
  if (Boolean(explicitHome) !== Boolean(explicitRevision))
    throw new Error("managed Caveman requires NORTH_CAVEMAN_HOME and NORTH_CAVEMAN_REV together");
  const home = resolve(explicitHome || join(homedir(), "code", "caveman"));
  const provenance = explicitHome ? "explicit" as const : "local-dev" as const;
  let revision: string;
  let content: string;
  try {
    const origin = normalizeOrigin(git(home, ["remote", "get-url", "origin"]).trim());
    if (!origin) throw new Error("wrong origin");
    revision = explicitRevision || git(home, ["rev-parse", "HEAD"]).trim();
    content = git(home, ["show", `${revision}:${SKILL_PATH}`]);
  } catch (gitError) {
    if (!explicitHome || !explicitRevision || !home.startsWith("/nix/store/"))
      throw new Error("managed Caveman fork provenance unavailable", { cause: gitError });
    revision = explicitRevision;
    try { content = readFileSync(resolve(home, SKILL_PATH), "utf8"); }
    catch (cause) { throw new Error("managed Caveman immutable skill unavailable", { cause }); }
    return { content, revision, sourceKind: "immutable-file", provenance };
  }
  return { content, revision, sourceKind: "git-object", provenance };
}

/**
 * Adapted from tompassarelli/caveman src/hooks/caveman-subagent.js at 8351247.
 * Original project copyright (c) 2026 Julius Brussee, MIT License.
 */
export function renderCavemanSkill(skillContent: string, selected: "lite" | "full"): string {
  const body = skillContent.replace(/^---[\s\S]*?---\s*/, "");
  const filtered = body.split("\n").reduce<string[]>((lines, line) => {
    const tableRow = line.match(/^\|\s*\*\*(\S+?)\*\*\s*\|/);
    if (tableRow) {
      if (tableRow[1] === selected) lines.push(line);
      return lines;
    }
    const example = line.match(/^- (\S+?):\s/);
    if (example) {
      if (example[1] === selected) lines.push(line);
      return lines;
    }
    lines.push(line);
    return lines;
  }, []);
  return `CAVEMAN MODE ACTIVE — level: ${selected}\n\n${filtered.join("\n")}`;
}

/** Resolve and attest once per managed run, before any provider side effect. */
export function resolveManagedCaveman(
  request: unknown,
  env: NodeJS.ProcessEnv = process.env,
): CavemanResolution {
  const requestedMode = mode(request) ?? mode(env.AGENT_CAVEMAN) ?? "off";
  const source: CavemanSource = mode(request) ? "request" : mode(env.AGENT_CAVEMAN) ? "env" : "default";
  const decisionReason = source === "request" ? "explicit-request"
    : source === "env" ? "inherited-env" : "default-off-unproven-savings";
  if (requestedMode === "off") return {
    requestedMode, resolvedMode: "off", source, implementation: "disabled",
    decisionReason, instructions: "", measurementCoverage: "unknown",
  };
  const loaded = artifact(env);
  const skillSha256 = sha256(loaded.content);
  if (loaded.revision !== CANONICAL_REVISION || skillSha256 !== CANONICAL_SKILL_SHA256)
    throw new Error("managed Caveman fork revision or artifact does not match canonical contract");
  const instructions = renderCavemanSkill(loaded.content, requestedMode);
  return {
    requestedMode, resolvedMode: requestedMode, source, decisionReason, implementation: "fork-skill",
    instructions, repository: REPOSITORY, revision: loaded.revision, skillSha256,
    skillBytes: Buffer.byteLength(loaded.content, "utf8"),
    renderedSha256: sha256(instructions), renderedBytes: Buffer.byteLength(instructions, "utf8"),
    sourceKind: loaded.sourceKind, resolutionProvenance: loaded.provenance,
    measurementCoverage: "exact",
  };
}
