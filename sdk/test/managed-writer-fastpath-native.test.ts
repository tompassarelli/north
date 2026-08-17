import { afterEach, expect, test } from "bun:test";
import { kw, Keyword } from "../src/coord-wire";
import {
  FramRpcClient, FramRpcTransportError,
  type FramRpcTransportInput,
} from "../src/store-rpc-client";
import {
  FramTriple, RPC_SUBJECT_ANY, framInstant, rpcFence, rpcList, rpcListValues,
  rpcOption, rpcOptionValue, rpcRecord, rpcRecordFields, termEquals, triple,
  type BatchAction, type RpcErrorTerm, type RpcResponse, type Term,
} from "../src/store-rpc-codec";
import {
  fastPublish, identityMarker, normalizeAgentEntity, writeLeaseResource,
} from "../src/managed-writer-fastpath";

const PRESET: Record<string, string> = {
  kind: "lane",
  role: "integrator",
  goal: "prove native publication",
  provider: "openai",
  provider_target: "codex-a",
  live_input: "turn-framed",
  live_input_state: "armed",
  live_input_epoch: "00000000-0000-4000-8000-000000000303",
  model: "gpt-5.6-sol",
  effort: "high",
  composition_kind: "template",
  composition_id: "integrator",
  composition_overrides: "[]",
  repo: "north",
  spawned_at: "2026-08-04T01:00:00Z",
  display_handle: "openai-a-sol-high-integrator",
  display_name: "openai:codex-a · sol · high · orchestration:integrator",
};

const SUBJECT = "agent:native-probe";
const ENTITY = normalizeAgentEntity(SUBJECT)!;
const HOLDER = "managed-agent-writer:native";
const OPERATION_ID = "native-op";
const MARKER = identityMarker(PRESET);
const RESOURCE = writeLeaseResource(ENTITY);

const savedDisable = process.env.NORTH_MANAGED_WRITER_FASTPATH;
const savedRedirect = process.env.NORTH_IDENTITY_TEST_REDIRECT;

afterEach(() => {
  if (savedDisable === undefined) delete process.env.NORTH_MANAGED_WRITER_FASTPATH;
  else process.env.NORTH_MANAGED_WRITER_FASTPATH = savedDisable;
  if (savedRedirect === undefined) delete process.env.NORTH_IDENTITY_TEST_REDIRECT;
  else process.env.NORTH_IDENTITY_TEST_REDIRECT = savedRedirect;
});

type BatchOutcome =
  | "success"
  | "conflict"
  | "fence-mismatch"
  | "sent-commit"
  | "durability-ambiguous"
  | "success-without-apply"
  | "malformed-success";

interface HarnessOptions {
  initialRows?: FramTriple[];
  batchOutcomes?: BatchOutcome[];
  acquireSentAmbiguous?: boolean;
}

interface NativeHarness {
  client: FramRpcClient;
  calls: FramRpcTransportInput[];
  rows: FramTriple[];
}

function rpcError(code: string, retryable: boolean): RpcErrorTerm {
  return { code: kw(code), retryable, message: code, detail: null };
}

function response(
  input: FramRpcTransportInput,
  servedVersion: number,
  payload: Term | null,
  error: RpcErrorTerm | null = null,
  page: RpcResponse["page"] = null,
): RpcResponse {
  return {
    space: input.request.space,
    op: input.request.op,
    servedVersion,
    page,
    error,
    payload,
  };
}

function decodedBatch(input: FramRpcTransportInput): { actions: BatchAction[]; fence: Term | null } {
  const [encodedActions, encodedFence] = rpcRecordFields(
    input.request.payload, kw("rpc/batch"), 2,
  );
  const actions = rpcListValues(encodedActions!).map((encoded) => {
    const [operation, proposition, policy] = rpcRecordFields(
      encoded, kw("rpc/action"), 3,
    );
    if (!(operation instanceof Keyword) || !(proposition instanceof FramTriple)
        || !(policy instanceof Keyword)) throw new Error("mistyped test batch");
    return {
      op: operation.name === "rpc/assert" ? "assert" as const : "retract" as const,
      proposition,
      policy,
    };
  });
  return { actions, fence: rpcOptionValue(encodedFence!) };
}

function mutationPayload(actions: readonly BatchAction[], malformed = false): Term {
  const count = malformed ? Math.max(0, actions.length - 1) : actions.length;
  const transaction = triple("north-coordination", kw("kernel/tx-sequence"), 1);
  return rpcRecord(kw("rpc/mutation-result"), [rpcList(
    Array.from({ length: count }, (_, index) => rpcRecord(
      kw("rpc/action-result"), [
        index, true, triple(transaction, kw("kernel/op-ordinal"), index),
      ],
    )),
  )]);
}

function projectionRows(
  projection: Record<string, string> = PRESET,
  marker = MARKER,
): FramTriple[] {
  return [
    ...Object.entries(projection).map(([predicate, value]) => triple(ENTITY, predicate, value)),
    triple(ENTITY, "identity_manifest_sha256", marker),
  ];
}

function startHarness(options: HarnessOptions = {}): NativeHarness {
  const calls: FramRpcTransportInput[] = [];
  const rows = [...(options.initialRows ?? [])];
  const batchOutcomes = [...(options.batchOutcomes ?? ["success"])];
  let version = 10;
  let activeFence: Term | null = null;
  let acquireAmbiguityPending = options.acquireSentAmbiguous === true;

  const apply = (actions: readonly BatchAction[]) => {
    for (const action of actions) {
      if (action.op === "assert") rows.push(action.proposition);
      else {
        const index = rows.findLastIndex((row) => termEquals(row, action.proposition));
        if (index >= 0) rows.splice(index, 1);
      }
    }
    version += 1;
  };

  const transport = async (input: FramRpcTransportInput): Promise<RpcResponse> => {
    calls.push(input);
    switch (input.request.op.name) {
      case "rpc/version":
        return response(input, version, kw("rpc/unit"));
      case "rpc/lease-acquire": {
        const expected = input.request.expectedVersion!;
        const candidate = rpcFence(RESOURCE, HOLDER, expected + 1);
        if (acquireAmbiguityPending) {
          acquireAmbiguityPending = false;
          activeFence = candidate;
          version = expected + 1;
          throw new FramRpcTransportError(
            "rpc-truncated", "lease acknowledgement lost", true,
            input.request.op.name, 1,
          );
        }
        activeFence = candidate;
        version = expected + 1;
        return response(input, version, rpcRecord(
          kw("lease/grant"), [candidate, framInstant(1_800_000_000, 0)],
        ));
      }
      case "rpc/lease-check":
        return response(input, version, rpcRecord(kw("lease/check"), [
          activeFence !== null && termEquals(activeFence, input.request.payload),
          rpcOption(null),
        ]));
      case "rpc/scan":
        return response(
          input, version, rpcRecord(kw("rpc/triples"), [rpcList(rows)]), null,
          { ordinal: 0, cursor: null, done: true },
        );
      case "rpc/batch": {
        const decoded = decodedBatch(input);
        const outcome = batchOutcomes.shift() ?? "success";
        if (outcome === "conflict") {
          version += 1;
          return response(input, version, null, rpcError("rpc/conflict", true));
        }
        if (outcome === "fence-mismatch")
          return response(input, version, null, rpcError("rpc/lease-fence-mismatch", false));
        if (outcome === "durability-ambiguous")
          return response(input, version, null, rpcError("durability-ambiguous", true));
        if (outcome === "sent-commit") {
          apply(decoded.actions);
          throw new FramRpcTransportError(
            "rpc-truncated", "batch acknowledgement lost", true,
            input.request.op.name, 1,
          );
        }
        if (outcome === "success") apply(decoded.actions);
        return response(
          input, version,
          mutationPayload(decoded.actions, outcome === "malformed-success"),
        );
      }
      case "rpc/lease-release":
        activeFence = null;
        return response(input, version, rpcRecord(kw("lease/released"), [true]));
      default:
        throw new Error(`unexpected op ${input.request.op.name}`);
    }
  };

  return {
    client: FramRpcClient.create({
      port: 1,
      spaceId: "north-coordination",
      maxAttempts: 1,
      retryDelayMs: 0,
      jitterMs: 0,
      transport,
    }),
    calls,
    rows,
  };
}

async function publish(harness: NativeHarness) {
  delete process.env.NORTH_MANAGED_WRITER_FASTPATH;
  delete process.env.NORTH_IDENTITY_TEST_REDIRECT;
  return fastPublish(
    SUBJECT, PRESET, HOLDER, OPERATION_ID, 5_000, { client: harness.client },
  );
}

test("framrpc fresh publish sends one sorted fenced batch with marker last", async () => {
  const harness = startHarness();
  expect(await publish(harness)).toEqual({ status: "committed", operationId: OPERATION_ID });

  const batches = harness.calls.filter((call) => call.request.op.name === "rpc/batch");
  expect(batches).toHaveLength(1);
  const batch = batches[0]!;
  const decoded = decodedBatch(batch);
  expect(batch.request.expectedVersion).toBe(11);
  expect(termEquals(decoded.fence, rpcFence(RESOURCE, HOLDER, 11))).toBe(true);
  expect(decoded.actions.every((action) => action.policy.name === RPC_SUBJECT_ANY.name)).toBe(true);
  const predicates = decoded.actions.map((action) => action.proposition.t2);
  expect(predicates.at(-1)).toBe("identity_manifest_sha256");
  expect(predicates.slice(0, -1)).toEqual(
    Object.keys(PRESET).sort((left, right) => left < right ? -1 : left > right ? 1 : 0),
  );
  expect(harness.calls.map((call) => call.request.op.name)).toEqual([
    "rpc/version", "rpc/lease-acquire", "rpc/scan", "rpc/batch", "rpc/scan",
    "rpc/lease-release",
  ]);
});

test("framrpc exact replay includes occurrence frequencies and sends no publication mutation", async () => {
  const exact = startHarness({ initialRows: projectionRows() });
  expect(await publish(exact)).toEqual({
    status: "committed", operationId: OPERATION_ID, reason: "exact_replay",
  });
  expect(exact.calls.some((call) => call.request.op.name === "rpc/batch")).toBe(false);

  const duplicate = startHarness({
    initialRows: [...projectionRows(), triple(ENTITY, "role", PRESET.role)],
  });
  expect(await publish(duplicate)).toBeNull();
  expect(duplicate.calls.some((call) => call.request.op.name === "rpc/batch")).toBe(false);
});

test("framrpc declines killed prefixes, non-exact projections, and foreign occupancy", async () => {
  for (const rows of [
    [triple(ENTITY, "kind", "lane")],
    projectionRows({ ...PRESET, role: "worker" }, identityMarker({ ...PRESET, role: "worker" })),
    [triple(ENTITY, "terminal_manifest_sha256", "foreign")],
  ]) {
    const harness = startHarness({ initialRows: rows });
    expect(await publish(harness)).toBeNull();
    expect(harness.calls.some((call) => call.request.op.name === "rpc/batch")).toBe(false);
    expect(harness.calls.at(-1)!.request.op.name).toBe("rpc/lease-release");
  }
});

test("framrpc conflict is zero-applied and replans under the same fence", async () => {
  const harness = startHarness({ batchOutcomes: ["conflict", "success"] });
  expect(await publish(harness)).toEqual({ status: "committed", operationId: OPERATION_ID });
  const batches = harness.calls.filter((call) => call.request.op.name === "rpc/batch");
  expect(batches).toHaveLength(2);
  expect(batches.map((call) => call.request.expectedVersion)).toEqual([11, 12]);
  expect(termEquals(decodedBatch(batches[0]!).fence, decodedBatch(batches[1]!).fence)).toBe(true);
});

test("framrpc fence mismatch performs final classification and never retries stale", async () => {
  const harness = startHarness({ batchOutcomes: ["fence-mismatch"] });
  expect(await publish(harness)).toBeNull();
  expect(harness.calls.filter((call) => call.request.op.name === "rpc/batch")).toHaveLength(1);
  expect(harness.calls.filter((call) => call.request.op.name === "rpc/scan")).toHaveLength(2);
});

test("framrpc sent ambiguity retries identical bytes and exact readback decides", async () => {
  const harness = startHarness({ batchOutcomes: ["sent-commit", "conflict"] });
  expect(await publish(harness)).toEqual({ status: "committed", operationId: OPERATION_ID });
  const batches = harness.calls.filter((call) => call.request.op.name === "rpc/batch");
  expect(batches).toHaveLength(2);
  expect(batches[1]!.request.expectedVersion).toBe(batches[0]!.request.expectedVersion);
  expect(termEquals(batches[1]!.request.payload, batches[0]!.request.payload)).toBe(true);
});

test("framrpc sent lease ambiguity uses the reconstructable candidate fence", async () => {
  const harness = startHarness({ acquireSentAmbiguous: true });
  expect(await publish(harness)).toEqual({ status: "committed", operationId: OPERATION_ID });
  const check = harness.calls.find((call) => call.request.op.name === "rpc/lease-check")!;
  expect(termEquals(check.request.payload, rpcFence(RESOURCE, HOLDER, 11))).toBe(true);
  expect(harness.calls.filter((call) => call.request.op.name === "rpc/lease-acquire")).toHaveLength(1);
});

test("framrpc acknowledged mismatch and malformed action results are indeterminate", async () => {
  const missing = startHarness({ batchOutcomes: ["success-without-apply"] });
  expect(await publish(missing)).toEqual({
    status: "indeterminate", operationId: OPERATION_ID,
    reason: "native_batch_readback_mismatch",
  });

  const malformed = startHarness({ batchOutcomes: ["malformed-success"] });
  expect(await publish(malformed)).toEqual({
    status: "indeterminate", operationId: OPERATION_ID,
    reason: "unexpected_native_batch_result",
  });
});

test("framrpc durability ambiguity is restart-required and performs no readback or release", async () => {
  const harness = startHarness({ batchOutcomes: ["durability-ambiguous"] });
  expect(await publish(harness)).toEqual({
    status: "indeterminate", operationId: OPERATION_ID,
    reason: "durability_ambiguous_restart_required",
  });
  expect(harness.calls.filter((call) => call.request.op.name === "rpc/scan")).toHaveLength(1);
  expect(harness.calls.some((call) => call.request.op.name === "rpc/lease-release")).toBe(false);
});
