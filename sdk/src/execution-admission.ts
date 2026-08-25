import { accessSync, constants } from "node:fs";
import { spawnSync } from "node:child_process";
import { basename, dirname, isAbsolute, resolve } from "node:path";
import type { OrchestrationCapability } from "./orchestration-capabilities";
import {
  hasAuthoringCapability, providerCapabilityRejectionCode,
} from "./orchestration-capabilities";
import { preflightReadonlyShell, ReadonlyShellUnavailableError } from "./readonly-shell";
import {
  PROVIDER_UNSENT_PROOF_VERSION,
  ProviderRetrySafeError,
  type ProviderId,
  type RoutingTarget,
} from "./providers/types";
import { admitRoutingRequest } from "./routing-admission";
import { orchestrationCapabilities } from "./orchestration-staffing";
import { spendGuardVerdict, reserveSpend } from "./spend-guard";
import { StoreRpcClient } from "./store-rpc-client";
import {
  admitDeliveryLivenessFact, deliveryDispatchClassFromEnvironment,
  deliveryLivenessInputRevision, deliveryLivenessPath,
  deliveryLivenessRequiredFromEnvironment, DeliveryLivenessAuthorityError,
  type DeliveryDispatchClass,
} from "./delivery-liveness";

const REPO = resolve(import.meta.dir, "../..");
const ENGINE = `${REPO}/bin/north`;
const MCP = `${REPO}/bin/north-mcp`;
const admissionReceipts = new WeakMap<object, Set<ProviderId>>();

/**
 * The complete environment boundary exposed to the managed North MCP process.
 *
 * Provider CLIs still receive their own scrubbed account environment, but MCP
 * servers are a separate authority boundary: never forward the ambient process
 * environment (which may contain credentials or unrelated provider settings).
 * Keep only lane identity, North/Beagle Store instance selection, routing runtime knobs,
 * and attribution/provenance selectors required by the North executable.
 */
export const MANAGED_NORTH_MCP_ENV_KEYS = [
  "HOME",
  "NORTH_BIN",
  "AGENT_ID",
  "AGENT_TOPOLOGY",
  "AGENT_COORDINATOR",
  "NORTH_PORT",
  "NORTH_STORE_HOST",
  "NORTH_TELEMETRY_PARTITION",
  "NORTH_TELEMETRY_PORT",
  "NORTH_TELEMETRY_SPACE_ID",
  "NORTH_RUN_ID",
  "NORTH_THREAD_ID",
  "NORTH_RUN_CAPABILITY",
  "NORTH_RUN_ARTIFACT_DIR",
  "NORTH_CHECKPOINT_ENABLED",
  "NORTH_CHECKPOINT_EXECUTION_ROOT",
  "NORTH_CHECKPOINT_WORKTREE",
  "NORTH_CHECKPOINT_REPOSITORY",
  "NORTH_CHECKPOINT_BRANCH",
  "NORTH_CHECKPOINT_BASE",
  "NORTH_CHECKPOINT_GIT",
  "NORTH_CHECKPOINT_GITLEAKS",
  "NORTH_HOME",
  "NORTH_STREAM_DIR",
  "NORTH_AGENT_LOGS_DIR",
  "NORTH_NO_COLOR",
  "NORTH_STALL_MS",
  "NORTH_BG_MAX_CONTINUATIONS",
  "NORTH_MCP_BB",
  "NORTH_MCP_BUN",
  "NORTH_MCP_MANAGED_CODEX_BIN",
  "NORTH_MKFIFO_BIN",
  "NORTH_GIT_BIN",
  "NORTH_PEER_BB",
  "BEAGLE_STORE_BIN",
  "BEAGLE_STORE_HOME",
  "BEAGLE_STORE_OUT",
  "BEAGLE_STORE_SERVER_CONNECT",
  "BEAGLE_STORE_SERVER_PORT",
  "BEAGLE_STORE_SPACE_ID",
  "BEAGLE_STORE_SINGLE_VALUED",
  "BEAGLE_STORE_TERMINAL_PREDS",
  "BEAGLE_STORE_THREADS",
  "BEAGLE_STORE_WITHDRAWN_PREDS",
  "AGENT_MACHINERY_HOME",
  "NORTH_AGENT_RUNTIME_HOME",
  "ORCHESTRATION_STAFFING_CATALOG",
  "NORTH_ROUTING_POLICY",
  "NORTH_PROVIDER_OBSERVATIONS",
  "NORTH_PROVIDER_MODEL_OBSERVATIONS",
  "NORTH_ALLOCATION_MODE",
  "NORTH_ANTHROPIC_ENTITLEMENT_PRESSURE",
  "NORTH_OPENAI_ENTITLEMENT_PRESSURE",
  "NORTH_PROVIDER_ORDER",
  "NORTH_PROVIDER_WEIGHTS",
  "NORTH_RESERVED_FRONTIER_PROVIDER",
  "NORTH_ENVELOPE_ACCOUNTING",
  "NORTH_HARNESS_STATE",
  "NORTH_DELIVERY_LIVENESS_REQUIRED",
  "NORTH_DELIVERY_DISPATCH_CLASS",
  "NORTH_AUTHOR",
  "NORTH_DRIVER",
  "NORTH_LEAD",
  "NORTH_PROJECT",
  "NORTH_PROPOSED_BY",
  "NORTH_SOURCE",
  "NORTH_PACKAGE_REV",
] as const;

export function managedNorthMcpEnvironment(
  source: NodeJS.ProcessEnv | Record<string, unknown>,
): Record<string, string> {
  const environment = Object.fromEntries(MANAGED_NORTH_MCP_ENV_KEYS.flatMap((key) => {
    const value = source[key];
    return typeof value === "string" ? [[key, value]] : [];
  }));
  const direct = source.NORTH_MCP_MANAGED_CODEX_BIN;
  const wrapper = source.NORTH_MANAGED_CODEX_BIN;
  if (typeof direct === "string" && typeof wrapper === "string" && direct !== wrapper)
    throw new Error("managed North MCP Codex selector is contradictory");
  const selector = typeof direct === "string" ? direct
    : typeof wrapper === "string" ? wrapper
    : undefined;
  if (selector) environment.NORTH_MCP_MANAGED_CODEX_BIN = selector;
  return environment;
}

function managedRunArtifactDirectory(value: unknown): string | undefined {
  if (value === undefined) return undefined;
  if (typeof value !== "string" || !isAbsolute(value) || value.includes("\0")) return undefined;
  const canonical = resolve(value);
  if (canonical !== value
      || basename(dirname(canonical)) !== "run-artifacts"
      || !/^run-[a-f0-9]{64}$/.test(basename(canonical))) return undefined;
  return canonical;
}

function sameStringMap(actual: unknown, expected: Record<string, string>): boolean {
  if (!actual || typeof actual !== "object" || Array.isArray(actual)) return false;
  const entries = Object.entries(actual as Record<string, unknown>);
  return entries.length === Object.keys(expected).length
    && entries.every(([key, value]) => value === expected[key])
    && Object.keys(expected).every((key) => Object.hasOwn(actual, key));
}

function deeplyFrozen(value: unknown): boolean {
  if (!value || typeof value !== "object") return false;
  if (!Object.isFrozen(value)) return false;
  return Object.values(value as Record<string, unknown>)
    .every((child) => !child || typeof child !== "object" || deeplyFrozen(child));
}

/**
 * Carry one successful async admission across the synchronous provider.query
 * construction seam. The receipt is scoped to the exact options object and
 * provider, and is consumed once; direct adapter calls have no receipt and
 * therefore retain their defense-in-depth admission.
 */
export function markExecutionAdmission(provider: ProviderId, options: unknown): void {
  if ((typeof options !== "object" && typeof options !== "function") || options === null) return;
  const key = options as object;
  const providers = admissionReceipts.get(key) ?? new Set<ProviderId>();
  providers.add(provider);
  admissionReceipts.set(key, providers);
}

export function consumeExecutionAdmission(provider: ProviderId, options: unknown): boolean {
  if ((typeof options !== "object" && typeof options !== "function") || options === null) return false;
  const key = options as object;
  const providers = admissionReceipts.get(key);
  if (!providers?.delete(provider)) return false;
  if (providers.size === 0) admissionReceipts.delete(key);
  return true;
}

export class ExecutionAdmissionError extends ProviderRetrySafeError {
  // Typed as string so a subclass may carry a distinct, queryable terminal
  // outcome (e.g. the spend guard) without masquerading as a preflight block.
  readonly code: string = "blocked_preflight";
  readonly processOutcome: string = "blocked_preflight";
  constructor(message: string, options?: ErrorOptions) {
    super(message, options, {
      version: PROVIDER_UNSENT_PROOF_VERSION,
      mode: "managed",
      source: "adapter_preflight",
      durability: "adapter_receipt",
      requestBytesPrepared: 0,
      requestBytesSent: 0,
      observableEvents: 0,
    });
    this.name = "ExecutionAdmissionError";
  }
}

/**
 * Refusal of an API-billed provider target that lacks a complete spend budget.
 * A distinct code/outcome (`blocked_spend_guard`) keeps a spend-policy refusal
 * queryable in run evidence instead of conflating it with infra preflight. It
 * still extends ExecutionAdmissionError → ProviderRetrySafeError, so an
 * auto-routed spawn falls back to a subscription sibling under the existing
 * pre-side-effect proof rules; budget absence degrades to subscription work.
 */
export class SpendGuardError extends ExecutionAdmissionError {
  readonly code = "blocked_spend_guard";
  readonly processOutcome = "blocked_spend_guard";
  constructor(message: string, options?: ErrorOptions) {
    super(message, options);
    this.name = "SpendGuardError";
  }
}

export class ManagedDispatchAuthorityError extends ExecutionAdmissionError {
  readonly code = "blocked_dispatch_mode";
  readonly processOutcome = "blocked_dispatch_mode";
  constructor(message: string, options?: ErrorOptions) {
    super(message, options);
    this.name = "ManagedDispatchAuthorityError";
  }
}

export class DeliveryLivenessDispatchError extends ExecutionAdmissionError {
  readonly code = "blocked_delivery_liveness";
  readonly processOutcome = "blocked_delivery_liveness";
  constructor(message: string, options?: ErrorOptions) {
    super(message, options);
    this.name = "DeliveryLivenessDispatchError";
  }
}

/**
 * Honor the live operator dispatch mode before any managed provider work.
 *
 * The existing `north config` surface owns parsing and the mode vocabulary. The
 * SDK consumes only its bounded admission decision, so CLI, MCP, and direct SDK
 * entrypoints cannot drift.
 */
export function admitManagedDispatchAuthority(
  environment: NodeJS.ProcessEnv = process.env,
  dispatchClass = deliveryDispatchClassFromEnvironment(environment),
  activationPath?: string,
): void {
  const result = spawnSync(
    ENGINE,
    ["config", "dispatch", "--managed-admission"],
    {
      cwd: REPO,
      env: environment,
      encoding: "utf8",
      timeout: 2_000,
      maxBuffer: 16_384,
      stdio: ["ignore", "pipe", "pipe"],
    },
  );
  const stderr = result.stderr?.trim();
  if (result.error || result.status !== 0) {
    throw new ManagedDispatchAuthorityError(
      `managed_dispatch_authority_unavailable${stderr ? `: ${stderr}` : ""}`,
      result.error ? { cause: result.error } : undefined,
    );
  }
  if (stderr) console.warn(stderr);
  const action = result.stdout.trim();
  if (action === "deny") {
    throw new ManagedDispatchAuthorityError(
      "managed_dispatch_denied_by_native",
    );
  }
  if (action !== "allow") {
    throw new ManagedDispatchAuthorityError(
      `managed_dispatch_authority_invalid_action: ${JSON.stringify(action)}`,
    );
  }
  // A repair or validated direct-human request is explicitly controlled;
  // automated feature dispatch consumes the deterministic Firn floor fact.
  try {
    if (dispatchClass !== "feature") return;
    if (!deliveryLivenessRequiredFromEnvironment(environment, activationPath)) return;
    admitDeliveryLivenessFact({
      path: deliveryLivenessPath(environment),
      expectedNixosConfigRevision: deliveryLivenessInputRevision(environment),
    });
  } catch (error) {
    const message = error instanceof DeliveryLivenessAuthorityError
      ? error.reason : "delivery_liveness_authority_unavailable";
    throw new DeliveryLivenessDispatchError(message, { cause: error });
  }
}

/**
 * Fail-closed spend admission. Subscription providers are an O(1) branch that
 * never reads the ledger; an API-billed provider target must carry a complete,
 * readable `@spend-budget:<target>` entity or admission refuses. Defense in
 * depth for the routing-eligibility guard: a direct adapter call cannot admit an
 * unguarded API-billed target.
 */
export function admitSpendGuard(provider: string, target?: RoutingTarget): void {
  const targetId = target?.id ?? provider;
  const verdict = spendGuardVerdict(target?.provider ?? provider, targetId);
  if (!verdict.ok) throw new SpendGuardError(verdict.reason ?? `${provider}_spend_budget_incomplete`);
}

/**
 * The hard CAS reservation — design §2 touch point 2, run after the budget is
 * proven complete and before the provider query is constructed. Subscription
 * providers short-circuit O(1) with zero ledger reads. An API-billed target
 * commits a worst-case envelope reservation; a refusal (over-cap, conflict-
 * exhausted, missing schema/price, or an unreachable ledger) becomes a
 * `blocked_spend_guard` — retry-safe, so an auto-routed spawn degrades to a
 * subscription sibling instead of failing.
 */
export function admitSpendReservation(provider: string, target?: RoutingTarget): void {
  const reservationProvider = target?.provider ?? provider;
  const reservationTarget = target?.id ?? provider;
  const reservation = reserveSpend(reservationProvider, reservationTarget);
  if (!reservation.ok) {
    throw new SpendGuardError(
      `${reservationTarget}_spend_reservation_refused:${reservation.reason ?? "unknown"}`,
    );
  }
}

/**
 * `northCapabilities` marks a North-managed lane. Managed execution must carry
 * the canonical North MCP and an explicit child topology: an absent topology is
 * intentionally ambient authority for interactive top-level sessions, so
 * accepting it here would let a worker's shell invoke North as an orchestrator.
 */
export function validateManagedExecutionEnvelope(
  provider: ProviderId,
  capabilities: readonly OrchestrationCapability[],
  options: any,
): void {
  const topology = capabilities.includes("coordination") ? "orchestrator" : "worker";
  try {
    if (!deeplyFrozen(options?.northRoutingRequest))
      throw new Error("managed routing request is not immutable");
    const request = admitRoutingRequest(
      options.northRoutingRequest, `${provider} managed execution`,
    );
    const expectedCapabilities = orchestrationCapabilities(request);
    if (request.topology !== topology
        || JSON.stringify(expectedCapabilities) !== JSON.stringify(capabilities)) {
      throw new Error("managed routing request disagrees with compiled capability authority");
    }
  } catch (cause) {
    throw new ExecutionAdmissionError(
      `${provider}_managed_orchestration_request_contract_missing`, { cause },
    );
  }
  const agentId = typeof options?.env?.AGENT_ID === "string"
    ? options.env.AGENT_ID.trim()
    : "";
  if (!agentId || options?.env?.AGENT_TOPOLOGY !== topology) {
    throw new ExecutionAdmissionError(`${provider}_managed_identity_topology_contract_missing`);
  }

  const north = options?.mcpServers?.north;
  const artifactDirectory = managedRunArtifactDirectory(
    north?.env?.NORTH_RUN_ARTIFACT_DIR,
  );
  if (north?.env?.NORTH_RUN_ARTIFACT_DIR !== undefined && artifactDirectory === undefined) {
    throw new ExecutionAdmissionError(`${provider}_managed_run_artifact_contract_invalid`);
  }
  const expectedNorthEnv = managedNorthMcpEnvironment({
    ...options?.env,
    NORTH_BIN: ENGINE,
    ...(artifactDirectory === undefined ? {} : { NORTH_RUN_ARTIFACT_DIR: artifactDirectory }),
  });
  if (options?.northDataOnly === true) {
    if (!options?.mcpServers || Object.keys(options.mcpServers).length !== 0) {
      throw new ExecutionAdmissionError(`${provider}_data_only_mcp_surface_must_be_empty`);
    }
  } else if (north?.type !== "stdio"
      || typeof north.command !== "string"
      || resolve(north.command) !== MCP
      || !Array.isArray(north.args)
      || north.args.length !== 0
      || expectedNorthEnv.NORTH_BIN !== ENGINE
      || expectedNorthEnv.AGENT_ID !== agentId
      || expectedNorthEnv.AGENT_TOPOLOGY !== topology
      || typeof expectedNorthEnv.NORTH_PORT !== "string"
      || !expectedNorthEnv.NORTH_PORT.trim()
      || !sameStringMap(north.env, expectedNorthEnv)) {
    throw new ExecutionAdmissionError(`${provider}_managed_north_mcp_contract_missing`);
  }
  const checkpointKeys = [
    "NORTH_CHECKPOINT_ENABLED", "NORTH_CHECKPOINT_EXECUTION_ROOT",
    "NORTH_CHECKPOINT_WORKTREE", "NORTH_CHECKPOINT_REPOSITORY",
    "NORTH_CHECKPOINT_BRANCH", "NORTH_CHECKPOINT_BASE",
    "NORTH_CHECKPOINT_GIT", "NORTH_CHECKPOINT_GITLEAKS",
  ] as const;
  const checkpointValues = Object.fromEntries(
    checkpointKeys.map((key) => [key, expectedNorthEnv[key]]),
  ) as Record<(typeof checkpointKeys)[number], string | undefined>;
  const checkpointEnabled = checkpointValues.NORTH_CHECKPOINT_ENABLED === "1";
  const checkpointResidue = checkpointKeys.some((key) => expectedNorthEnv[key] !== undefined);
  if (checkpointResidue && !checkpointEnabled)
    throw new ExecutionAdmissionError(`${provider}_managed_checkpoint_contract_invalid`);
  if (checkpointEnabled) {
    const allPresent = checkpointKeys.every((key) => {
      const value = checkpointValues[key];
      return typeof value === "string" && value.trim() === value && value.length > 0;
    });
    if (topology !== "worker"
        || !hasAuthoringCapability(capabilities)
        || !allPresent
        || checkpointValues.NORTH_CHECKPOINT_EXECUTION_ROOT !== options.cwd
        || checkpointValues.NORTH_CHECKPOINT_WORKTREE !== options.cwd
        || !isAbsolute(checkpointValues.NORTH_CHECKPOINT_REPOSITORY!)
        || !/^lane-[A-Za-z0-9][A-Za-z0-9._-]*$/.test(checkpointValues.NORTH_CHECKPOINT_BRANCH!)
        || !/^[0-9a-f]{40}(?:[0-9a-f]{24})?$/.test(checkpointValues.NORTH_CHECKPOINT_BASE!)
        || !isAbsolute(checkpointValues.NORTH_CHECKPOINT_GIT!)
        || !isAbsolute(checkpointValues.NORTH_CHECKPOINT_GITLEAKS!)
        || typeof expectedNorthEnv.NORTH_RUN_ID !== "string"
        || typeof expectedNorthEnv.NORTH_THREAD_ID !== "string"
        || typeof expectedNorthEnv.NORTH_RUN_CAPABILITY !== "string") {
      throw new ExecutionAdmissionError(`${provider}_managed_checkpoint_contract_invalid`);
    }
  }
}

async function requireCoordinator(
  northEnvironment: unknown,
  timeoutMs = 30_000,
): Promise<void> {
  if (!northEnvironment || typeof northEnvironment !== "object"
      || Array.isArray(northEnvironment))
    throw new ExecutionAdmissionError("north_coordination_contract_missing");
  const environment = northEnvironment as Record<string, unknown>;
  const portValue = environment.NORTH_PORT;
  const serverPortValue = environment.BEAGLE_STORE_SERVER_PORT;
  if (typeof portValue !== "string" || !portValue.trim()
      || typeof serverPortValue !== "string" || !serverPortValue.trim())
    throw new ExecutionAdmissionError("north_coordination_port_missing");
  const port = Number(portValue);
  const serverPort = Number(serverPortValue);
  if (!Number.isInteger(port) || port < 1 || port > 65_535
      || !Number.isInteger(serverPort) || serverPort < 1 || serverPort > 65_535)
    throw new ExecutionAdmissionError("north_coordination_port_invalid");
  if (port !== serverPort)
    throw new ExecutionAdmissionError("north_coordination_port_identity_mismatch");
  const spaceValue = environment.BEAGLE_STORE_SPACE_ID;
  if (typeof spaceValue !== "string" || !spaceValue.trim())
    throw new ExecutionAdmissionError("north_coordination_space_missing");
  if (spaceValue.trim() !== spaceValue)
    throw new ExecutionAdmissionError("north_coordination_space_invalid");
  const hostValue = environment.NORTH_STORE_HOST ?? environment.BEAGLE_STORE_SERVER_CONNECT
    ?? "127.0.0.1";
  if (typeof hostValue !== "string" || !hostValue.trim()
      || hostValue.trim() !== hostValue)
    throw new ExecutionAdmissionError("north_coordination_host_invalid");
  const boundedTimeout = Math.min(999_999, Math.max(1, timeoutMs));
  let client: StoreRpcClient | null = null;
  try {
    client = await StoreRpcClient.connect({
      host: hostValue,
      port,
      spaceId: spaceValue,
      connectTimeoutMs: boundedTimeout,
      readTimeoutMs: boundedTimeout,
      maxAttempts: 1,
      retryDelayMs: 0,
      jitterMs: 0,
    });
  } catch (cause) {
    throw new ExecutionAdmissionError("north_coordinator_preflight_failed", { cause });
  } finally {
    client?.close();
  }
}

/**
 * Provider-neutral, pre-turn admission. Every adapter calls this after compiling
 * its authority envelope and before constructing a provider query.
 */
export async function admitExecution(
  provider: ProviderId,
  capabilities: readonly OrchestrationCapability[],
  cwd: string,
  options?: any,
  target?: RoutingTarget,
): Promise<void> {
  const capabilityRejection = providerCapabilityRejectionCode(provider, capabilities);
  if (capabilityRejection) throw new ExecutionAdmissionError(capabilityRejection);
  // Fail-closed spend guard (defense in depth). Subscription providers return
  // O(1) without a ledger read; an API-billed target without a complete budget
  // refuses here even if it somehow bypassed routing eligibility. Then commit
  // the hard CAS reservation — the point past which concurrent admissions can
  // no longer collectively exceed the cap.
  admitSpendGuard(provider, target);
  admitSpendReservation(provider, target);
  try {
    accessSync(ENGINE, constants.X_OK);
  } catch (cause) {
    throw new ExecutionAdmissionError("north_executable_unavailable", { cause });
  }
  try {
    accessSync(MCP, constants.X_OK);
  } catch (cause) {
    throw new ExecutionAdmissionError("north_mcp_executable_unavailable", { cause });
  }
  if (provider === "anthropic" && capabilities.includes("shell.readonly")
      && options?.northDataOnly !== true) {
    try {
      preflightReadonlyShell(cwd, options?.env ?? process.env);
    } catch (error) {
      if (error instanceof ReadonlyShellUnavailableError)
        throw new ExecutionAdmissionError(error.message, { cause: error });
      throw error;
    }
  }
  // North MCP is every managed lane's identity and reporting surface, so a
  // managed worker fails closed against a dead coordinator. An interactive
  // session (coordinationOptional) may run unrecorded: for chat, the
  // coordinator is telemetry when present, never a launch gate.
  if (options?.northDataOnly !== true) {
    try {
      await requireCoordinator(options?.mcpServers?.north?.env);
    } catch (error) {
      if (!isCoordinationOptional(options)) throw error;
      console.warn("north: coordinator unreachable — session proceeds unrecorded");
    }
  }
}

// The harness authority seal covers the exact option key set, so writing a
// coordinationOptional property onto sealed options invalidates the seal
// (anthropic_harness_authority_seal_missing, 2026-08-09). Sealed callers mark
// the object here instead; the property remains honored for unsealed options.
const coordinationOptionalSessions = new WeakSet<object>();

export function markCoordinationOptional(options: object): void {
  coordinationOptionalSessions.add(options);
}

function isCoordinationOptional(options: any): boolean {
  if (!options || typeof options !== "object") return false;
  return coordinationOptionalSessions.has(options as object)
    || options.coordinationOptional === true;
}

export function admitPinnedProvider(
  provider: ProviderId | "auto" | undefined,
  capabilities: readonly OrchestrationCapability[],
): void {
  if (!provider || provider === "auto") return;
  const capabilityRejection = providerCapabilityRejectionCode(provider, capabilities);
  if (capabilityRejection) throw new ExecutionAdmissionError(capabilityRejection);
}
