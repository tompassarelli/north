import * as fs from "node:fs";

import {
  harnessCompositionEvidence, harnessOptions, type HarnessCompositionEvidence,
} from "./harness";
import { makeExecutionFold } from "./execution-fold";
import {
  applyOrchestrationStaffing, orchestrationCapabilities,
} from "./orchestration-staffing";
import { resolveTier } from "./providers/catalog";
import { routedQuery, selectProviderForExecution } from "./providers";
import { SerializedWireEventCommitter, StreamWriter } from "./stream-writer";
import { resolveStrugglePolicy } from "./struggle";
import { newRunId } from "./telemetry";
import {
  TIER1_DISTILLER_PROMPT, TIER1_DISTILLER_ROUTING,
  type Tier1ModelRequest, type Tier1ModelResult,
} from "./tier1-distiller";
import { WireEventWriter, type WireEvent, type WireQuery } from "./wire";

function retainFailure(current: unknown, next: unknown, message: string): unknown {
  return current === undefined ? next : new AggregateError([current, next], message);
}

export async function runTier1Model(request: Tier1ModelRequest): Promise<Tier1ModelResult> {
  request.signal?.throwIfAborted();
  const routing = applyOrchestrationStaffing(TIER1_DISTILLER_ROUTING);
  const capabilities = orchestrationCapabilities(routing);
  const abort = new AbortController();
  const onAbort = (): void => abort.abort(request.signal?.reason);
  request.signal?.addEventListener("abort", onAbort, { once: true });
  if (request.signal?.aborted) onAbort();
  const emptyCwd = await fs.promises.mkdtemp("/tmp/north-tier1-distiller-");
  let stream: StreamWriter | undefined;
  let query: WireQuery | undefined;
  let failure: unknown;
  let modelResult: Tier1ModelResult | undefined;
  try {
    const decision = await selectProviderForExecution(
      { provider: "anthropic" },
      undefined,
      {
        tier: routing.tier,
        reasoning: routing.reasoning,
        stableKey: `${request.project.digest}:${request.provenance.lineageDigest}`,
        capabilities,
        signal: abort.signal,
      },
    );
    const resolved = resolveTier(
      decision.provider,
      routing.tier,
      undefined,
      routing.reasoning,
    );
    const options = harnessOptions({
      self: request.attemptAgentId,
      provider: decision.provider,
      routingMetadata: routing,
      role: routing.role,
      posture: routing.posture,
      effort: routing.reasoning,
      cwd: emptyCwd,
      maxTurns: 1,
      systemPrompt: TIER1_DISTILLER_PROMPT,
      presenceRegistrar: false,
      presenceRenewer: false,
      activatedResources: [],
      availableSkills: [],
      dataOnly: true,
      modelAvailability: {
        exactModelPinned: false,
        targetId: decision.target,
        receipt: decision.modelAvailabilityReceipts?.[decision.target],
      },
      abortController: abort,
    });
    const writer = new WireEventWriter({ runId: newRunId(request.attemptAgentId) });
    stream = await StreamWriter.open(request.attemptAgentId);
    const committer = new SerializedWireEventCommitter(writer, stream);
    const fold = makeExecutionFold(resolveStrugglePolicy("worker", {}));
    let observed = 0;
    const observe = async (event: WireEvent): Promise<void> => {
      await committer.commitThrough(event);
      fold.observe(event);
      observed += 1;
    };
    const observeUnseen = async (): Promise<void> => {
      const events = writer.events();
      while (observed < events.length) await observe(events[observed]!);
    };
    const started = writer.append({
      kind: "run.started",
      lifecycle: "running",
      owner: request.attemptAgentId,
    });
    await observe(started);
    let composition: HarnessCompositionEvidence | undefined = harnessCompositionEvidence(options);
    query = routedQuery(
      decision,
      {
        input: request.input,
        options,
        writer,
        eventCommitter: committer,
      },
      routing.tier,
      undefined,
      (_route, evidence) => { composition = evidence ?? composition; },
    );
    try {
      for await (const event of query) await observe(event);
    } catch (error) {
      failure = error;
    }
    try { await query.close?.(); }
    catch (error) { failure = retainFailure(failure, error, "tier-1 provider execution and close failed"); }
    try { await observeUnseen(); }
    catch (error) { failure = retainFailure(failure, error, "tier-1 provider event persistence failed"); }
    const snapshot = fold.snapshot();
    if (failure === undefined) {
      if (!snapshot?.latestModelCallTerminal
          || snapshot.latestModelCallTerminal.status !== "succeeded") {
        failure = new Error("tier-1 provider ended without a successful model call");
      } else if (snapshot.toolActivity.admitted !== 0
          || snapshot.toolActivity.progressed !== 0
          || snapshot.toolActivity.terminal !== 0) {
        failure = new Error("tier-1 provider used tools despite its data-only contract");
      } else if (!snapshot.lastCompletedAssistantOutput?.trim()) {
        failure = new Error("tier-1 provider returned no Markdown body");
      } else if (!composition?.promptReceipt || composition.promptReceipt.coverage !== "exact") {
        failure = new Error("tier-1 provider has no exact prompt composition receipt");
      } else if (!composition.environmentReceipt
          || composition.environmentReceipt.counts.availableSkills !== 0
          || composition.environmentReceipt.counts.activatedResources !== 0) {
        failure = new Error("tier-1 provider environment includes ambient skill or resource authority");
      } else {
        modelResult = {
          body: snapshot.lastCompletedAssistantOutput,
          execution: {
            provider: decision.provider,
            wireRunId: writer.runId,
            wirePromptSha256: composition.promptReceipt.wireBytesSha256,
            promptManifestSha256: composition.promptReceipt.manifestSha256,
            environmentReceiptSha256: composition.environmentReceipt.manifestSha256,
            availableSkillCatalogSha256: composition.environmentReceipt.availableSkillCatalogSha256,
            activatedResourceClosureSha256: composition.environmentReceipt.activatedResourceClosureSha256,
          },
        };
      }
    }
    try {
      const terminal = writer.terminate(failure === undefined
        ? { lifecycle: "completed", reason: { code: "completed" } }
        : { lifecycle: "failed", reason: { code: "provider_error", detail: "tier1_distillation_failed" } });
      for (const event of terminal) await observe(event);
    } catch (error) {
      failure = retainFailure(failure, error, "tier-1 provider execution and terminal persistence failed");
    }
  } catch (error) {
    failure = retainFailure(failure, error, "tier-1 provider setup failed");
  } finally {
    request.signal?.removeEventListener("abort", onAbort);
    try { await query?.close?.(); }
    catch (error) { failure = retainFailure(failure, error, "tier-1 provider cleanup failed"); }
    try { await stream?.close(); }
    catch (error) { failure = retainFailure(failure, error, "tier-1 wire stream cleanup failed"); }
    try { await fs.promises.rm(emptyCwd, { recursive: true, force: true }); }
    catch (error) { failure = retainFailure(failure, error, "tier-1 empty authority root cleanup failed"); }
  }
  if (failure !== undefined) throw failure;
  if (!modelResult) throw new Error("tier-1 provider completed without a result");
  return modelResult;
}
