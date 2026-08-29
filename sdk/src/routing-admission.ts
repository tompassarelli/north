import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
import { applyOrchestrationStaffing } from "./orchestration-staffing";
import {
  ROUTING_REQUEST_FIELDS, parseCompleteRoutingRequest, routingMetadataFromEnv,
  type RoutingDraft, type RoutingRequest,
} from "./routing-metadata";

function deepFreeze<T>(value: T): T {
  if (value && typeof value === "object" && !Object.isFrozen(value)) {
    for (const child of Object.values(value as Record<string, unknown>)) deepFreeze(child);
    Object.freeze(value);
  }
  return value;
}

export interface RoutingAdmissionContext {
  /** Binding project-exposure-v1 sidecar; never part of the portable request. */
  projectProfile: unknown;
}

export interface ResolvedProjectExposureProfile {
  version: string;
  scope: string;
  facts: {
    consumer: string;
    state: string;
    effect: string;
    correctness: string;
    boundaries: string[];
    stage: string;
    explicitLifecycleEscalation: boolean;
  };
  engineeringContext: string;
  lifecycleBudget: Array<{ mechanism: string; evidence: string }>;
  $schema?: string;
}

export interface ResolvedRoutingAdmission {
  routingRequest: RoutingRequest;
  /** Binding project-exposure-v1 sidecar; never part of the portable request. */
  projectProfile: ResolvedProjectExposureProfile;
}

function canonicalJson(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.entries(value as Record<string, unknown>)
      .sort(([left], [right]) => left.localeCompare(right))
      .map(([key, child]) => `${JSON.stringify(key)}:${canonicalJson(child)}`)
      .join(",")}}`;
  }
  return JSON.stringify(value) ?? "undefined";
}

function validateCanonicalRoutingAdmission(
  projectProfile: unknown,
  request: RoutingRequest,
  surface: string,
): ResolvedRoutingAdmission {
  const orchestrationRoot = resolve(
    process.env.AGENT_MACHINERY_HOME
      ?? resolve(process.env.HOME ?? "", "code/agent-machinery/main"),
  );
  const entrypoint = resolve(orchestrationRoot, "index.mjs");
  const validation = spawnSync(process.execPath, [
    "--eval",
    "import {pathToFileURL} from 'node:url';const m=await import(pathToFileURL(process.argv[1]).href);let s='';for await(const c of process.stdin)s+=c;const v=JSON.parse(s);const p=m.resolveProjectExposureProfile(v.projectProfile);const r=m.validateRoutingAdmission(p,v.routingRequest);process.stdout.write(JSON.stringify({routingRequest:r,projectProfile:p}));",
    entrypoint,
  ], {
    input: JSON.stringify({ projectProfile, routingRequest: request }),
    encoding: "utf8",
    timeout: 5_000,
  });
  if (validation.error || validation.status !== 0) {
    const detail = validation.stderr?.trim()
      || validation.error?.message
      || "canonical validator failed";
    throw new Error(`${surface} failed canonical Orchestration routing admission: ${detail}`);
  }
  let canonicalAdmission: ResolvedRoutingAdmission;
  try { canonicalAdmission = JSON.parse(validation.stdout) as ResolvedRoutingAdmission; }
  catch {
    throw new Error(`${surface} canonical Orchestration routing admission returned invalid JSON`);
  }
  if (canonicalJson(canonicalAdmission.routingRequest) !== canonicalJson(request)) {
    throw new Error(`${surface} canonical Orchestration routing admission changed the request`);
  }
  return deepFreeze(canonicalAdmission);
}

function admitCanonicalRoutingRequest(
  value: RoutingDraft,
  surface: string,
): RoutingRequest {
  const request = parseCompleteRoutingRequest(value, surface);
  const admitted = applyOrchestrationStaffing(request);
  const changed = ROUTING_REQUEST_FIELDS.filter((field) =>
    JSON.stringify(admitted[field]) !== JSON.stringify(request[field]));
  if (changed.length) {
    throw new Error(
      `${surface} must carry a canonical complete Orchestration request; composer changed: `
      + changed.join(", ")
      + " (recover the valid payload shape: north show @contract:dispatch)",
    );
  }
  return admitted;
}

export function admitResolvedRoutingRequest(
  value: RoutingDraft,
  surface = "managed North agent",
  context: RoutingAdmissionContext = { projectProfile: undefined },
): ResolvedRoutingAdmission {
  const admitted = admitCanonicalRoutingRequest(value, surface);
  return validateCanonicalRoutingAdmission(context.projectProfile, admitted, surface);
}

/**
 * Strict managed-wire admission: prove both the complete structural request
 * and Orchestration's stock/bespoke catalog semantics without allowing this boundary
 * to hydrate or rewrite any caller-owned axis.
 */
export function admitRoutingRequest(
  value: RoutingDraft,
  surface = "managed North agent",
  context?: RoutingAdmissionContext,
): RoutingRequest {
  const admitted = admitCanonicalRoutingRequest(value, surface);
  if (context)
    return validateCanonicalRoutingAdmission(context.projectProfile, admitted, surface).routingRequest;
  return deepFreeze(JSON.parse(JSON.stringify(admitted)) as RoutingRequest);
}

export function routingRequestFromEnv(surface = "managed North environment"): RoutingRequest {
  const draft = routingMetadataFromEnv();
  return admitRoutingRequest(draft, surface);
}

export function projectProfileFromEnv(): unknown {
  const raw = process.env.AGENT_PROJECT_PROFILE;
  if (!raw) return undefined;
  try { return JSON.parse(raw); }
  catch { throw new Error("AGENT_PROJECT_PROFILE must contain valid JSON"); }
}
