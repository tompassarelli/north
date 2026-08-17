import { expect, test } from "bun:test";

import {
	buildEnvironmentReceipt,
	buildPromptReceipt,
	buildRunEnvelope,
	sha256Bytes,
} from "../src/composition-receipt";
import {
	assignLearningEpisode,
	DEFAULT_LEARNING_POLICY,
} from "../src/learning-regime";
import { wireLedgerSummary } from "../src/run-ledger";
import { wireModelAvailabilityReceipt } from "../src/run-provenance";
import { wireRunTelemetryFacts } from "../src/telemetry";
import {
	WireEventWriter,
	reduceWireEvents,
	wireEventId,
	wireRunId,
} from "../src/wire";

test("terminal wire telemetry preserves hashed learning provenance without private provider material", () => {
	const exactModelCanary = "private-model-canary-v99";
	const rawPromptCanary = "PRIVATE RAW PROMPT SENTINEL";
	const rawDiagnosticCanary = "PRIVATE RAW DIAGNOSTIC SENTINEL";
	const assignment = assignLearningEpisode(DEFAULT_LEARNING_POLICY, {
		episodeId: "telemetry-episode",
		taskSignatureSha256: sha256Bytes("task"),
		taskSignatureCoverage: "exact",
		risk: "p1",
		baseline: {
			modelTier: "senior",
			effort: "high",
			prompt: "baseline",
			authoring: "text",
			history: "git",
		},
	});
	const promptReceipt = buildPromptReceipt({
		coverage: "exact",
		wirePrompt: rawPromptCanary,
		modules: [{
			id: "core",
			schemaVersion: "v1",
			position: 0,
			sourceSha256: sha256Bytes("source"),
			rendered: rawPromptCanary,
		}],
	});
	const environmentReceipt = buildEnvironmentReceipt({
		availableSkills: [],
		activatedResources: [],
		tools: [],
		hooks: [],
		configs: [],
		executables: [],
		instructions: [],
	});
	const runEnvelopeReceipt = buildRunEnvelope({
		promptReceipt,
		environmentReceipt,
		assignmentSha256: assignment.manifestSha256,
		tier: "senior",
		effort: "high",
		model: exactModelCanary,
		providerAdapterVersion: "adapter-v1",
		providerRuntimeVersion: "runtime-v1",
	});
	const modelAvailability = wireModelAvailabilityReceipt({
		provider: "anthropic",
		targetId: "claude-primary",
		authMode: "ambient",
		model: exactModelCanary,
		observedAt: "2026-08-10T01:59:00.000Z",
		source: "claude-agent-sdk:Query.supportedModels",
		observationDigest: sha256Bytes("model availability receipt"),
	});
	expect(modelAvailability).toEqual({
		provider: "anthropic",
		targetId: "claude-primary",
		observedAt: "2026-08-10T01:59:00.000Z",
		source: "claude-agent-sdk:Query.supportedModels",
		observationDigest: sha256Bytes("model availability receipt"),
	});
	expect(JSON.stringify(modelAvailability)).not.toContain(exactModelCanary);
	let tick = 0;
	const writer = new WireEventWriter({
		runId: wireRunId("run:learning-telemetry"),
		eventId: (sequence) => wireEventId(`event:learning-telemetry:${sequence}`),
		now: () => new Date(Date.UTC(2026, 7, 10, 2, 0, tick++)).toISOString(),
	});
	writer.append({ kind: "run.started", lifecycle: "running", owner: "learning-lane" });
	writer.append({
		kind: "run.progress",
		lifecycle: "running",
		progress: {
			model: { provider: "anthropic", tier: "senior", capabilityClass: "authoring" },
			effort: "high",
		},
	});
	writer.terminate({ lifecycle: "completed", reason: { code: "completed" } });
	const events = writer.events();
	const projection = wireRunTelemetryFacts(
		{ thread: "@telemetry-learning", agent: "learning-lane" },
		reduceWireEvents(events),
		{ status: "recorded", summary: wireLedgerSummary(events) },
		{
			posture: "spawn",
			role: "implementer",
			provider: "anthropic",
			providerTarget: "claude-primary",
			providerReason: "requested_target_available",
			modelAvailability,
			requestedProvider: "auto",
			requestedTarget: "claude-primary",
			requestedTier: "senior",
			requestedEffort: "high",
			routingMetadata: {
				role: "implementer",
				taskGrade: "senior",
				domainRequirements: ["typescript"],
				topology: "worker",
				tier: "senior",
				reasoning: "high",
				posture: "deliver",
				composition: {
					kind: "template",
					id: "implementer",
					overrides: ["tier"],
					overrideReason: rawDiagnosticCanary,
				},
			},
			routingPinEvidence: {
				policyVersion: "north-routing-pin-v1",
				issuedAt: "2026-08-10T01:55:00.000Z",
				expiresAt: "2026-08-10T03:55:00.000Z",
				reasonCode: "explicit-human-request",
				detail: rawDiagnosticCanary,
				pins: [
					{ kind: "provider", value: "anthropic" },
					{ kind: "account", value: "private-account-canary" },
					{ kind: "model", value: exactModelCanary },
				],
			},
			promptComposition: {
				roleKind: "preset",
				roleId: "implementer",
				capabilities: ["coordination"],
				taskGrade: "senior",
				topology: "worker",
				tier: "senior",
				reasoning: "high",
				posture: "deliver",
				domainRequirements: ["typescript"],
				modelDelta: {
					provider: "anthropic",
					model: exactModelCanary,
					kind: "calibrated",
					path: rawDiagnosticCanary,
					reason: rawDiagnosticCanary,
				},
			},
			learningAssignment: assignment,
			promptReceipt,
			environmentReceipt,
			runEnvelopeReceipt,
			mcpActivity: {
				source: "fixture",
				coverage: "exact",
				totalCalls: 3,
				tools: [{ server: "beagle-store", tool: "show", count: 3 }],
				operationReceipts: [
					{ tool: "beagle-store/show", operation: "reasoning.inspect", durationMs: 2, resultSize: 1, outcome: "ok" },
					{ tool: "beagle-store/show", operation: "reasoning.inspect", durationMs: 3, resultSize: 1, outcome: "ok" },
					{ tool: "beagle-store/show", operation: "reasoning.inspect", durationMs: 4, resultSize: 1, outcome: "typed_failure" },
				],
				operationAggregates: [{
					operation: "reasoning.inspect",
					count: 3,
					totalDurationMs: 9,
					meanDurationMs: 3,
					failureCount: 1,
				}],
			},
			nativeCommandActivity: {
				source: "fixture",
				coverage: "exact",
				totalCommands: 1,
				successfulCommands: 1,
				readCommands: 1,
				northBinaryProbe: "passed",
				completions: [{
					commandSha256: sha256Bytes("private command"),
					outputSha256: sha256Bytes("private output"),
					status: "completed",
					exitCode: 0,
					shape: "read",
					durationMs: 7,
				}],
			},
			effectiveAuthority: {
				provider: "anthropic",
				capabilities: ["coordination"],
				nativeMultiAgent: "disabled",
				liveInput: "streaming",
				northEnabledTools: ["show"],
				authoringHooks: "harness-exact",
				builtins: ["Read"],
				managedTools: ["mcp__north__show"],
				web: "disabled",
			},
			allocationMode: "balanced",
			entitlementPressure: "low",
			fallbackCount: 0,
			fallbackPath: ["anthropic"],
			fallbackTargetPath: ["claude-primary"],
			envelopeScopes: ["project"],
			envelopeRetries: 0,
			processOutcome: "ran",
			deliveryOutcome: "unverified",
			deliveryReason: "delivery_reservation_unavailable_at_finalize",
		},
	);

	expect(projection.facts).toContainEqual([
		"learning_assignment_sha256",
		assignment.manifestSha256,
	]);
	expect(projection.facts).toContainEqual(["prompt_receipt_sha256", promptReceipt.manifestSha256]);
	expect(projection.facts).toContainEqual(["run_envelope_sha256", runEnvelopeReceipt.manifestSha256]);
	expect(projection.facts).toContainEqual([
		"model_availability_digest",
		sha256Bytes("model availability receipt"),
	]);
	expect(projection.facts).toContainEqual([
		"mcp_operation_aggregate",
		JSON.stringify({
			operation: "reasoning.inspect",
			count: 3,
			totalDurationMs: 9,
			meanDurationMs: 3,
			failureCount: 1,
		}),
	]);
	expect(projection.facts.some(([predicate]) => predicate === "native_command_completion")).toBe(true);
	expect(projection.facts).toContainEqual(["effective_authority_provider", "anthropic"]);
	expect(projection.facts.filter(([predicate]) => predicate === "routing_pin")).toEqual([
		["routing_pin", JSON.stringify({ kind: "provider", value: "anthropic" })],
	]);
	const encoded = JSON.stringify(projection);
	expect(encoded).not.toContain(rawPromptCanary);
	expect(encoded).not.toContain(exactModelCanary);
	expect(encoded).not.toContain(rawDiagnosticCanary);
	expect(encoded).not.toContain("private-account-canary");
});
