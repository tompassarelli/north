import { createHash } from "node:crypto";

export type ReceiptCoverage = "exact" | "partial" | "unknown";

export interface PromptModuleInput {
  id: string;
  schemaVersion: string;
  position: number;
  dependencies?: readonly string[];
  sourceSha256: string;
  rendered: string | Uint8Array;
  safeParameters?: Readonly<Record<string, string | number | boolean | null>>;
  parameterDigests?: Readonly<Record<string, string>>;
}

export interface PromptBranchInput {
  ruleId: string;
  conditionId: string;
  inputDigest: string;
  branch: string;
}

export interface PromptReceiptInput {
  modules: readonly PromptModuleInput[];
  branches?: readonly PromptBranchInput[];
  wirePrompt: string | Uint8Array;
  coverage: ReceiptCoverage;
  coverageReason?: string;
}

export interface PromptReceipt {
  version: "north-prompt-receipt:v1";
  coverage: ReceiptCoverage;
  coverageReason?: string;
  modules: ReadonlyArray<{
    id: string;
    schemaVersion: string;
    position: number;
    dependencies: readonly string[];
    sourceSha256: string;
    renderedBytesSha256: string;
    renderedBytes: number;
    safeParameters: Readonly<Record<string, string | number | boolean | null>>;
    parameterDigests: Readonly<Record<string, string>>;
  }>;
  branches: readonly PromptBranchInput[];
  wireBytesSha256: string;
  wireBytes: number;
  manifestSha256: string;
}

export interface EnvironmentArtifact {
  id: string;
  sha256?: string;
  coverage: ReceiptCoverage;
}

export interface EnvironmentReceiptInput {
  availableSkills: readonly EnvironmentArtifact[];
  activatedResources: readonly EnvironmentArtifact[];
  tools: readonly EnvironmentArtifact[];
  hooks: readonly EnvironmentArtifact[];
  configs: readonly EnvironmentArtifact[];
  executables: readonly EnvironmentArtifact[];
  instructions: readonly EnvironmentArtifact[];
  coverageReason?: string;
}

export interface EnvironmentReceipt {
  version: "north-environment-receipt:v1";
  coverage: ReceiptCoverage;
  coverageReason?: string;
  availableSkillCatalogSha256: string;
  activatedResourceClosureSha256: string;
  toolCatalogSha256: string;
  hookClosureSha256: string;
  configClosureSha256: string;
  executableClosureSha256: string;
  instructionClosureSha256: string;
  counts: Readonly<Record<string, number>>;
  manifestSha256: string;
}

export interface RunEnvelopeInput {
  promptReceipt: PromptReceipt;
  environmentReceipt: EnvironmentReceipt;
  assignmentSha256: string;
  tier: string;
  effort: string;
  model?: string;
  providerAdapterVersion: string;
  providerRuntimeVersion: string;
}

export interface RunEnvelopeReceipt extends RunEnvelopeInput {
  version: "north-run-envelope:v1";
  manifestSha256: string;
}

const SHA256 = /^[a-f0-9]{64}$/;
const IDENTIFIER = /^[A-Za-z0-9][A-Za-z0-9_.:/-]{0,127}$/;
const SECRET_NAME = /(?:secret|token|password|credential|api[_-]?key|cookie|authorization)/i;
const RECEIPT_COVERAGES = new Set<ReceiptCoverage>(["exact", "partial", "unknown"]);

function canonical(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(canonical);
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.entries(value as Record<string, unknown>)
      .sort(([left], [right]) => left.localeCompare(right))
      .map(([key, child]) => [key, canonical(child)]));
  }
  return value;
}

export function canonicalReceiptJson(value: unknown): string {
  return JSON.stringify(canonical(value));
}

export function sha256Bytes(value: string | Uint8Array): string {
  return createHash("sha256").update(value).digest("hex");
}

export function sha256Manifest(value: unknown): string {
  return sha256Bytes(canonicalReceiptJson(value));
}

function requireIdentifier(value: string, label: string): string {
  if (!IDENTIFIER.test(value)) throw new Error(`${label} must be a portable identifier`);
  return value;
}

function requireDigest(value: string, label: string): string {
  if (!SHA256.test(value)) throw new Error(`${label} must be a lowercase SHA-256 digest`);
  return value;
}

function safeParameters(
  value: PromptModuleInput["safeParameters"],
): Readonly<Record<string, string | number | boolean | null>> {
  const result: Record<string, string | number | boolean | null> = {};
  for (const [key, item] of Object.entries(value ?? {}).sort(([a], [b]) => a.localeCompare(b))) {
    requireIdentifier(key, "prompt parameter name");
    if (SECRET_NAME.test(key)) throw new Error(`prompt parameter ${key} is secret-shaped`);
    if (typeof item === "string" && Buffer.byteLength(item, "utf8") > 256)
      throw new Error(`prompt parameter ${key} exceeds the privacy-bounded limit`);
    if (typeof item === "number" && !Number.isFinite(item))
      throw new Error(`prompt parameter ${key} must be finite`);
    result[key] = item;
  }
  return Object.freeze(result);
}

function digestMap(value: Readonly<Record<string, string>> | undefined): Readonly<Record<string, string>> {
  return Object.freeze(Object.fromEntries(Object.entries(value ?? {})
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([key, digest]) => [requireIdentifier(key, "parameter digest name"), requireDigest(digest, key)])));
}

export function buildPromptReceipt(input: PromptReceiptInput): PromptReceipt {
  if (input.modules.length === 0) throw new Error("prompt receipt requires at least one module");
  if (!RECEIPT_COVERAGES.has(input.coverage)) throw new Error("invalid prompt receipt coverage");
  const positions = new Set<number>();
  const modules = [...input.modules]
    .sort((left, right) => left.position - right.position || left.id.localeCompare(right.id))
    .map((module) => {
      if (!Number.isSafeInteger(module.position) || module.position < 0)
        throw new Error("prompt module position must be a nonnegative safe integer");
      if (positions.has(module.position)) throw new Error("prompt module positions must be unique");
      positions.add(module.position);
      const renderedBytes = Buffer.byteLength(module.rendered);
      return Object.freeze({
        id: requireIdentifier(module.id, "prompt module id"),
        schemaVersion: requireIdentifier(module.schemaVersion, "prompt module schema version"),
        position: module.position,
        dependencies: Object.freeze([...(module.dependencies ?? [])]
          .map((dependency) => requireIdentifier(dependency, "prompt module dependency"))),
        sourceSha256: requireDigest(module.sourceSha256, "prompt module source digest"),
        renderedBytesSha256: sha256Bytes(module.rendered),
        renderedBytes,
        safeParameters: safeParameters(module.safeParameters),
        parameterDigests: digestMap(module.parameterDigests),
      });
    });
  const ids = new Set(modules.map(({ id }) => id));
  if (ids.size !== modules.length) throw new Error("prompt module ids must be unique");
  for (const module of modules)
    for (const dependency of module.dependencies)
      if (!ids.has(dependency)) throw new Error(`prompt module ${module.id} has missing dependency ${dependency}`);
  const branches = Object.freeze([...(input.branches ?? [])].map((branch) => Object.freeze({
    ruleId: requireIdentifier(branch.ruleId, "prompt branch rule"),
    conditionId: requireIdentifier(branch.conditionId, "prompt branch condition"),
    inputDigest: requireDigest(branch.inputDigest, "prompt branch input digest"),
    branch: requireIdentifier(branch.branch, "prompt branch selection"),
  })));
  const base = {
    version: "north-prompt-receipt:v1" as const,
    coverage: input.coverage,
    ...(input.coverageReason ? { coverageReason: input.coverageReason.slice(0, 256) } : {}),
    modules: Object.freeze(modules),
    branches,
    wireBytesSha256: sha256Bytes(input.wirePrompt),
    wireBytes: Buffer.byteLength(input.wirePrompt),
  };
  return Object.freeze({ ...base, manifestSha256: sha256Manifest(base) });
}

function artifactClosure(
  artifacts: readonly EnvironmentArtifact[],
  label: string,
): { digest: string; coverage: ReceiptCoverage; count: number } {
  const normalized = [...artifacts]
    .sort((left, right) => left.id.localeCompare(right.id))
    .map(({ id, sha256, coverage }) => {
      if (!RECEIPT_COVERAGES.has(coverage)) throw new Error(`invalid ${label} artifact coverage`);
      return {
        id: requireIdentifier(id, `${label} artifact id`),
        coverage,
        ...(sha256 ? { sha256: requireDigest(sha256, `${label} artifact digest`) } : {}),
      };
    });
  if (new Set(normalized.map(({ id }) => id)).size !== normalized.length)
    throw new Error(`${label} artifact ids must be unique`);
  const coverages = normalized.map(({ coverage, sha256 }) => sha256 ? coverage : "unknown" as const);
  const coverage: ReceiptCoverage = coverages.includes("unknown") ? "unknown"
    : coverages.includes("partial") ? "partial" : "exact";
  return { digest: sha256Manifest(normalized), coverage, count: normalized.length };
}

function combinedCoverage(values: readonly ReceiptCoverage[]): ReceiptCoverage {
  return values.includes("unknown") ? "unknown" : values.includes("partial") ? "partial" : "exact";
}

export function buildEnvironmentReceipt(input: EnvironmentReceiptInput): EnvironmentReceipt {
  const available = artifactClosure(input.availableSkills, "available skill");
  const activated = artifactClosure(input.activatedResources, "activated resource");
  const tools = artifactClosure(input.tools, "tool");
  const hooks = artifactClosure(input.hooks, "hook");
  const configs = artifactClosure(input.configs, "config");
  const executables = artifactClosure(input.executables, "executable");
  const instructions = artifactClosure(input.instructions, "instruction");
  const coverage = combinedCoverage([
    available.coverage, activated.coverage, tools.coverage, hooks.coverage,
    configs.coverage, executables.coverage, instructions.coverage,
  ]);
  const base = {
    version: "north-environment-receipt:v1" as const,
    coverage,
    ...(input.coverageReason ? { coverageReason: input.coverageReason.slice(0, 256) } : {}),
    availableSkillCatalogSha256: available.digest,
    activatedResourceClosureSha256: activated.digest,
    toolCatalogSha256: tools.digest,
    hookClosureSha256: hooks.digest,
    configClosureSha256: configs.digest,
    executableClosureSha256: executables.digest,
    instructionClosureSha256: instructions.digest,
    counts: Object.freeze({
      availableSkills: available.count,
      activatedResources: activated.count,
      tools: tools.count,
      hooks: hooks.count,
      configs: configs.count,
      executables: executables.count,
      instructions: instructions.count,
    }),
  };
  return Object.freeze({ ...base, manifestSha256: sha256Manifest(base) });
}

export function buildRunEnvelope(input: RunEnvelopeInput): RunEnvelopeReceipt {
  requireDigest(input.assignmentSha256, "learning assignment digest");
  requireIdentifier(input.tier, "run tier");
  requireIdentifier(input.effort, "run effort");
  requireIdentifier(input.providerAdapterVersion, "provider adapter version");
  requireIdentifier(input.providerRuntimeVersion, "provider runtime version");
  if (input.model) requireIdentifier(input.model, "run model");
  const base = {
    version: "north-run-envelope:v1" as const,
    promptReceipt: input.promptReceipt,
    environmentReceipt: input.environmentReceipt,
    assignmentSha256: input.assignmentSha256,
    tier: input.tier,
    effort: input.effort,
    ...(input.model ? { model: input.model } : {}),
    providerAdapterVersion: input.providerAdapterVersion,
    providerRuntimeVersion: input.providerRuntimeVersion,
  };
  return Object.freeze({ ...base, manifestSha256: sha256Manifest(base) });
}
