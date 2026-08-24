import { createHash, randomUUID } from "node:crypto";
import {
  acquireDeliveryAttemptLeases,
  commitDeliveryAttemptProviderStart,
  commitDeliveryAttemptProvedUnsent,
  commitDeliveryAttemptTerminal,
  DELIVERY_ATTEMPT_LEASE_TTL_MS,
  newDeliveryRunContext,
  reserveDeliveryRun,
  reserveDeliveryRunWithRecovery,
  writeDeliveryAttemptLaunchIntent,
  type DeliveryAttemptLaunchIntent,
  type DeliveryAttemptProviderStart,
  type DeliveryReservation,
} from "../delivery-evidence";
import { getThreadFacts, normalizeNorthEntityId, type Fact } from "../north-client";
import { selectProviderForExecution } from "../provider-routing";
import { resolveTier } from "../providers/catalog";
import { newRunId } from "../telemetry";
import { encodeWireJsonlLine, type WireEvent } from "../wire";
import {
  StoreBridgeCommandReceipts,
  type BridgeCommandReceipts,
} from "./command-receipts";
import { resolveBridgeLaunchSelection } from "./provider";
import type { BridgeLaunchRole, BridgeLaunchSelection } from "./protocol";

export interface BridgeAppLaunchRequest extends BridgeLaunchSelection {
  role: BridgeLaunchRole;
  prompt: string;
  cwd: string;
  selectedThreadId?: string;
}

export type BridgeAppLaunchUnsentReason =
  | "attempt-binding-refused"
  | "daemon-not-contacted"
  | "daemon-launch-refused";

export interface ManagedBridgeAppLaunch {
  readonly attemptId: string;
  readonly executionId: string;
  readonly threadId: string;
  readonly provider: "openai";
  readonly model: string;
  readonly providerEffectObserved: boolean;
  readonly settled: boolean;
  readonly leaseFailure: Promise<Error>;
  observeDurableWireEvent(event: WireEvent): Promise<void>;
  proveUnsent(
    reason: Exclude<BridgeAppLaunchUnsentReason, "attempt-binding-refused">,
  ): Promise<void>;
}

interface BridgeAppLaunchDependencies {
  env?: NodeJS.ProcessEnv;
  loadThreadFacts?: (threadId: string) => readonly Fact[];
  selectProvider?: typeof selectProviderForExecution;
  acquireLeases?: typeof acquireDeliveryAttemptLeases;
  reserve?: typeof reserveDeliveryRun;
  launchIntent?: typeof writeDeliveryAttemptLaunchIntent;
  providerStart?: typeof commitDeliveryAttemptProviderStart;
  provedUnsent?: typeof commitDeliveryAttemptProvedUnsent;
  terminal?: typeof commitDeliveryAttemptTerminal;
  commandReceipts?: BridgeCommandReceipts;
  executionId?: string;
  leaseRenewIntervalMs?: number;
}

function sha256(value: string): string {
  return createHash("sha256").update(value, "utf8").digest("hex");
}

function exactRegisteredThread(
  selectedThreadId: string | undefined,
  env: NodeJS.ProcessEnv,
  loadThreadFacts: (threadId: string) => readonly Fact[],
): string {
  const selected = selectedThreadId?.trim();
  const control = env.NORTH_BRIDGE_CONTROL_THREAD?.trim()
    || env.NORTH_THREAD_ID?.trim()
    || env.AGENT_THREAD?.trim();
  const candidate = selected || control;
  if (!candidate) {
    throw new Error(
      "Bridge app launch requires an exact selected or managed control thread",
    );
  }
  const threadId = normalizeNorthEntityId(candidate);
  const titles = loadThreadFacts(threadId)
    .filter(({ predicate }) => predicate === "title")
    .map(({ value }) => value);
  if (titles.length !== 1 || !titles[0]?.trim()) {
    throw new Error(`Bridge app launch thread @${threadId} is not registered in Store`);
  }
  return threadId;
}

function reporterAgentId(env: NodeJS.ProcessEnv, executionId: string): string {
  const managed = env.AGENT_ID?.trim().replace(/^@?agent:/u, "");
  return managed || `bridge-app-${executionId}`;
}

function unsentReceipt(
  executionId: string,
  attemptId: string,
  reason: BridgeAppLaunchUnsentReason,
): string {
  return sha256(`north:bridge-app-launch-unsent:v1\0${executionId}\0${attemptId}\0${reason}`);
}

export async function prepareManagedBridgeAppLaunch(
  request: BridgeAppLaunchRequest,
  dependencies: BridgeAppLaunchDependencies = {},
): Promise<ManagedBridgeAppLaunch> {
  if (!request.prompt.trim()) throw new Error("Bridge app launch requires a prompt");
  if (!request.cwd.trim()) throw new Error("Bridge app launch requires a working directory");
  if (request.provider !== undefined && request.provider !== "openai") {
    throw new Error("Bridge app launch requires a Store-authorized OpenAI execution route");
  }

  const env = dependencies.env ?? process.env;
  const threadId = exactRegisteredThread(
    request.selectedThreadId,
    env,
    dependencies.loadThreadFacts ?? getThreadFacts,
  );
  const executionId = dependencies.executionId ?? randomUUID();
  const reporter = reporterAgentId(env, executionId);
  const selection = resolveBridgeLaunchSelection("openai", request.role, request);
  const routing = await (dependencies.selectProvider ?? selectProviderForExecution)(
    { provider: "openai" },
    undefined,
    {
      tier: selection.resolved.tier,
      reasoning: selection.resolved.effort,
      model: request.model,
      stableKey: reporter,
    },
  );
  const accountReceipt = "executionAccountReceipt" in routing
    ? routing.executionAccountReceipt
    : undefined;
  if (routing.provider !== "openai" || !accountReceipt) {
    throw new Error("Bridge app launch has no Store-authorized OpenAI execution route");
  }
  const resolved = resolveTier(
    routing.provider,
    selection.resolved.tier,
    request.model,
    selection.resolved.effort,
  );
  if (!resolved.model) throw new Error("Bridge app launch could not resolve an execution model");

  const context = newDeliveryRunContext(newRunId(reporter), threadId, reporter);
  const leases = await (dependencies.acquireLeases ?? acquireDeliveryAttemptLeases)(
    context,
    routing.target,
  );
  const reserve = dependencies.reserve ?? reserveDeliveryRun;
  const launchIntent = dependencies.launchIntent ?? writeDeliveryAttemptLaunchIntent;
  const provedUnsent = dependencies.provedUnsent ?? commitDeliveryAttemptProvedUnsent;
  const commandReceipts = dependencies.commandReceipts ?? new StoreBridgeCommandReceipts();
  let reservation: DeliveryReservation | undefined;
  let intent: DeliveryAttemptLaunchIntent | undefined;
  try {
    reservation = reserveDeliveryRunWithRecovery(context, {
      provider: routing.provider,
      accountId: routing.target,
      model: resolved.model,
      accountAuthorityReceiptSha256: accountReceipt.accountAuthority.digest,
      routeObservationReceiptSha256: accountReceipt.usage.receipt.digest,
      threadLease: leases.threadLease,
      accountLease: leases.accountLease,
    }, reserve);
    intent = launchIntent(context, reservation);
    await commandReceipts.bindExecution(executionId, reservation.attemptId, {
      provider: routing.provider,
      model: resolved.model,
    });
  } catch (error) {
    if (reservation && intent) {
      try {
        provedUnsent(
          context,
          reservation,
          intent,
          unsentReceipt(executionId, reservation.attemptId, "attempt-binding-refused"),
        );
      } catch (settlementError) {
        await leases.release();
        commandReceipts.close?.();
        throw new AggregateError(
          [error, settlementError],
          "Bridge app attempt binding and proved-unsent settlement both failed",
        );
      }
    }
    await leases.release();
    commandReceipts.close?.();
    throw error;
  }
  commandReceipts.close?.();

  let effectObserved = false;
  let providerStart: DeliveryAttemptProviderStart | undefined;
  let isSettled = false;
  let released = false;
  let renewalTimer: ReturnType<typeof setTimeout> | undefined;
  let renewing: Promise<void> | undefined;
  const leaseFailure = Promise.withResolvers<Error>();
  const renewalIntervalMs = Math.max(
    1,
    dependencies.leaseRenewIntervalMs ?? Math.floor(DELIVERY_ATTEMPT_LEASE_TTL_MS / 3),
  );
  const scheduleRenewal = (): void => {
    if (released || isSettled) return;
    renewalTimer = setTimeout(async () => {
      renewalTimer = undefined;
      if (released || isSettled) return;
      renewing = leases.renew();
      try {
        await renewing;
        scheduleRenewal();
      } catch (error) {
        leaseFailure.resolve(error instanceof Error
          ? error
          : new Error("Bridge app launch lease renewal failed"));
      } finally {
        renewing = undefined;
      }
    }, renewalIntervalMs);
  };
  scheduleRenewal();
  const release = async (): Promise<void> => {
    if (released) return;
    released = true;
    if (renewalTimer !== undefined) clearTimeout(renewalTimer);
    renewalTimer = undefined;
    await renewing?.catch(() => undefined);
    await leases.release();
  };
  const commitUnsent = async (receiptSha256: string): Promise<void> => {
    if (isSettled) return;
    if (effectObserved) {
      throw new Error("Bridge app launch observed a provider effect and cannot be proved unsent");
    }
    provedUnsent(context, reservation, intent, receiptSha256);
    isSettled = true;
    await release();
  };

  return {
    attemptId: reservation.attemptId,
    executionId,
    threadId,
    provider: "openai",
    model: resolved.model,
    get providerEffectObserved() { return effectObserved; },
    get settled() { return isSettled; },
    leaseFailure: leaseFailure.promise,
    async observeDurableWireEvent(event: WireEvent): Promise<void> {
      if (event.kind === "model-call.started" && !providerStart) {
        effectObserved = true;
        providerStart = (dependencies.providerStart ?? commitDeliveryAttemptProviderStart)(
          context,
          reservation,
          intent,
          sha256(encodeWireJsonlLine(event)),
        );
      }
      if (event.kind !== "run.terminated" || isSettled) return;
      if (!effectObserved) {
        await commitUnsent(sha256(encodeWireJsonlLine(event)));
        return;
      }
      if (!providerStart) {
        throw new Error("Bridge app provider-start settlement is unavailable at terminal");
      }
      (dependencies.terminal ?? commitDeliveryAttemptTerminal)(
        context,
        reservation,
        intent,
        providerStart,
        sha256(encodeWireJsonlLine(event)),
      );
      isSettled = true;
      await release();
    },
    async proveUnsent(reason): Promise<void> {
      await commitUnsent(unsentReceipt(executionId, reservation.attemptId, reason));
    },
  };
}
