// Golden frames in fixtures/framrpc-golden-frames.json were produced by Fram's
// own encoder (`bb -cp "$FRAM_OUT"` loading `framrpc`), so an
// assertion here is a cross-check against the server's wire authority,
// not against this module's own idea of the format.
import { expect, test } from "bun:test";
import { createServer } from "node:net";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { kw, Keyword, framSpaceId, nativeRouteForSubject } from "../src/coord-wire";
import {
  decodeFrame, decodeLeaseCheck, decodeLeaseGrant, decodeLeaseReleased,
  decodeMutationResult, decodeStatus, decodeTriples, encodeRequestFrame,
  encodeResponseFrame, FramFloat, FramInstant, FramRpcCodecError, FramTriple,
  framInstant, instantToMillis, rpcBatch, rpcFence, rpcLeaseAcquire,
  rpcLeaseRenew, rpcList, rpcListValues, rpcOption, rpcOptionValue, rpcRecord,
  rpcTriplePattern, termEquals, triple, RPC_UNIT, RPC_SUBJECT_EXISTING,
  RPC_V2_HEADER_BYTES, RPC_V2_MAX_TERM_DEPTH,
  type RpcRequest, type RpcResponse, type Term,
} from "../src/framrpc-codec";
import {
  FramRpcClient, FramRpcServerError, FramRpcTransportError, socketRoundTrip,
  type FramRpcTransportInput,
} from "../src/framrpc-client";

const GOLDEN: Record<string, string> = JSON.parse(readFileSync(
  join(import.meta.dir, "fixtures", "framrpc-golden-frames.json"), "utf8",
));

const SPACE = "north-coordination";
const RESOURCE = "managed-agent-write:8f2a";
const HOLDER = "north-sdk-writer";
const FENCE = rpcFence(RESOURCE, HOLDER, 42);
const TRANSACTION = triple(SPACE, kw("kernel/tx-sequence"), 46);
const occurrence = (ordinal: number): FramTriple =>
  triple(TRANSACTION, kw("kernel/op-ordinal"), ordinal);
const CURSOR = rpcRecord(kw("query/cursor"), [
  42, "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef", 1,
  rpcRecord(kw("query/row"), [rpcList([7, triple("@agent:x", "role", "worker")])]),
]);

const golden = (name: string): Uint8Array => {
  const encoded = GOLDEN[name];
  if (encoded === undefined) throw new Error(`missing golden frame ${name}`);
  return Uint8Array.from(Buffer.from(encoded, "base64"));
};
const base64 = (bytes: Uint8Array): string => Buffer.from(bytes).toString("base64");

const request = (over: Partial<RpcRequest> & { op: Keyword; payload: Term }): RpcRequest => ({
  space: SPACE, expectedVersion: null, page: null, timeoutMs: null, ...over,
});
const response = (over: Partial<RpcResponse> & { op: Keyword }): RpcResponse => ({
  space: SPACE, servedVersion: 42, page: null, error: null, payload: null, ...over,
});

function nest(depth: number): Term {
  let term: Term = "leaf";
  for (let index = 0; index < depth; index += 1) term = triple(term, false, true);
  return term;
}

// --- golden frames: encode --------------------------------------------------

const REQUEST_CASES: Array<[string, number, RpcRequest]> = [
  ["version-request", 1, request({ op: kw("rpc/version"), payload: RPC_UNIT })],
  ["scan-request-page1", 2, request({
    op: kw("rpc/scan"), page: { limit: 200, cursor: null },
    payload: rpcTriplePattern("@agent:x", null, null),
  })],
  ["scan-request-page2", 3, request({
    op: kw("rpc/scan"), page: { limit: 200, cursor: CURSOR },
    payload: rpcTriplePattern("@agent:x", null, null),
  })],
  ["lease-acquire-request", 4, request({
    op: kw("rpc/lease-acquire"), expectedVersion: 41,
    payload: rpcLeaseAcquire(RESOURCE, HOLDER, 60000),
  })],
  ["lease-renew-request", 5, request({
    op: kw("rpc/lease-renew"), payload: rpcLeaseRenew(FENCE, 60000),
  })],
  ["lease-release-request", 6, request({ op: kw("rpc/lease-release"), payload: FENCE })],
  ["lease-check-request", 7, request({ op: kw("rpc/lease-check"), payload: FENCE })],
  ["batch-request", 8, request({
    op: kw("rpc/batch"), expectedVersion: 99,
    payload: rpcBatch([
      { op: "assert", proposition: triple("@agent:x", "role", "worker") },
      {
        op: "retract", proposition: triple("@agent:x", "kind", "lane"),
        policy: RPC_SUBJECT_EXISTING,
      },
    ], FENCE),
  })],
  ["batch-request-unfenced", 9, request({
    op: kw("rpc/batch"),
    payload: rpcBatch(
      [{ op: "assert", proposition: triple("@agent:x", "goal", "naïve 😀 goal") }],
      null,
    ),
  })],
  ["deep-term-request-256", 10, request({
    op: kw("rpc/batch"), payload: nest(RPC_V2_MAX_TERM_DEPTH),
  })],
];

const RESPONSE_CASES: Array<[string, number, RpcResponse]> = [
  ["version-response", 1, response({ op: kw("rpc/version"), payload: RPC_UNIT })],
  ["scan-response-page", 2, response({
    op: kw("rpc/scan"), page: { ordinal: 0, cursor: CURSOR, done: false },
    payload: rpcRecord(kw("rpc/triples"), [rpcList([
      triple("@agent:x", "role", "worker"), triple("@agent:x", "kind", "lane"),
    ])]),
  })],
  ["scan-response-final", 3, response({
    op: kw("rpc/scan"), page: { ordinal: 1, cursor: null, done: true },
    payload: rpcRecord(kw("rpc/triples"), [rpcList([])]),
  })],
  ["lease-grant-response", 4, response({
    op: kw("rpc/lease-acquire"), servedVersion: 43,
    payload: rpcRecord(kw("lease/grant"), [FENCE, framInstant(1785000000, 123456789)]),
  })],
  ["lease-released-response", 6, response({
    op: kw("rpc/lease-release"), servedVersion: 45,
    payload: rpcRecord(kw("lease/released"), [true]),
  })],
  ["lease-check-response", 7, response({
    op: kw("rpc/lease-check"), servedVersion: 45,
    payload: rpcRecord(kw("lease/check"), [true, rpcOption(framInstant(1785000060, 0))]),
  })],
  ["lease-check-absent-response", 7, response({
    op: kw("rpc/lease-check"), servedVersion: 45,
    payload: rpcRecord(kw("lease/check"), [false, rpcOption(null)]),
  })],
  ["batch-response", 8, response({
    op: kw("rpc/batch"), servedVersion: 46,
    payload: rpcRecord(kw("rpc/mutation-result"), [rpcList([
      rpcRecord(kw("rpc/action-result"), [
        0, true, occurrence(0),
      ]),
      rpcRecord(kw("rpc/action-result"), [1, false, occurrence(1)]),
    ])]),
  })],
  ["conflict-error-response", 8, response({
    op: kw("rpc/batch"), servedVersion: 47,
    error: {
      code: kw("rpc/conflict"), retryable: true,
      message: "expected-version does not match current version", detail: null,
    },
  })],
  ["lease-fence-mismatch-error-response", 8, response({
    op: kw("rpc/batch"), servedVersion: 47,
    error: {
      code: kw("rpc/lease-fence-mismatch"), retryable: false,
      message: "lease fence does not name the current lease", detail: null,
    },
  })],
  ["durability-ambiguous-error-response", 8, response({
    op: kw("rpc/batch"), servedVersion: 47,
    error: {
      code: kw("durability-ambiguous"), retryable: true,
      message: "commit outcome is durability-ambiguous; restart is required",
      detail: triple("detail", "restart", "required"),
    },
  })],
  ["space-mismatch-error-response", 1, response({
    op: kw("rpc/version"), servedVersion: 0,
    error: {
      code: kw("rpc/space-mismatch"), retryable: false,
      message: "request SpaceId does not match the served space", detail: null,
    },
  })],
  ["lease-held-error-response", 4, response({
    op: kw("rpc/lease-acquire"), servedVersion: 42,
    error: {
      code: kw("rpc/lease-held"), retryable: false,
      message: "lease resource is already held", detail: null,
    },
  })],
  ["status-response", 11, response({
    op: kw("rpc/status"),
    payload: rpcRecord(kw("rpc/status"), [
      kw("serving"), 1234, kw("native"),
      rpcRecord(kw("rpc/result-cache"), [1, 2, 3, 4]),
    ]),
  })],
  ["term-atoms-response", 12, response({
    op: kw("rpc/scan"),
    payload: rpcRecord(kw("rpc/triples"), [rpcList([
      triple(-9007199254740991, new FramFloat(1.5), true),
      triple(false, RPC_UNIT, "naïve 😀"),
      triple(framInstant(0, 0), framInstant(-1, 999999999), 0),
    ])]),
  })],
];

for (const [name, id, value] of REQUEST_CASES) {
  test(`request frame ${name} matches the Clojure encoder byte for byte`, () => {
    expect(base64(encodeRequestFrame(id, value))).toBe(GOLDEN[name]);
  });
}

for (const [name, id, value] of RESPONSE_CASES) {
  test(`response frame ${name} matches the Clojure encoder byte for byte`, () => {
    expect(base64(encodeResponseFrame(id, value))).toBe(GOLDEN[name]);
  });
}

// --- golden frames: decode + round trip -------------------------------------

for (const [name, id] of [...REQUEST_CASES, ...RESPONSE_CASES]) {
  test(`frame ${name} round-trips decode → encode unchanged`, () => {
    const bytes = golden(name);
    const frame = decodeFrame(bytes);
    expect(frame.requestId).toBe(id);
    const reencoded = frame.kind === "request"
      ? encodeRequestFrame(frame.requestId, frame.request!)
      : encodeResponseFrame(frame.requestId, frame.response!);
    expect(base64(reencoded)).toBe(base64(bytes));
  });
}

test("a decoded request exposes op, expected-version, page, and payload", () => {
  const frame = decodeFrame(golden("batch-request"));
  const decoded = frame.request!;
  expect(decoded.space).toBe(SPACE);
  expect(decoded.op.name).toBe("rpc/batch");
  expect(decoded.expectedVersion).toBe(99);
  expect(decoded.page).toBeNull();
  expect(termEquals(decoded.payload, rpcBatch([
    { op: "assert", proposition: triple("@agent:x", "role", "worker") },
    {
      op: "retract", proposition: triple("@agent:x", "kind", "lane"),
      policy: RPC_SUBJECT_EXISTING,
    },
  ], FENCE))).toBe(true);
});

test("served-version is extracted from every response", () => {
  expect(decodeFrame(golden("version-response")).response!.servedVersion).toBe(42);
  expect(decodeFrame(golden("batch-response")).response!.servedVersion).toBe(46);
  expect(decodeFrame(golden("conflict-error-response")).response!.servedVersion).toBe(47);
});

test("scan payloads decode to their row triples and page metadata", () => {
  const first = decodeFrame(golden("scan-response-page")).response!;
  const rows = decodeTriples(first.payload);
  expect(rows.length).toBe(2);
  expect(termEquals(rows[0]!, triple("@agent:x", "role", "worker"))).toBe(true);
  expect(first.page!.done).toBe(false);
  expect(first.page!.ordinal).toBe(0);
  expect(termEquals(first.page!.cursor, CURSOR)).toBe(true);
  const last = decodeFrame(golden("scan-response-final")).response!;
  expect(decodeTriples(last.payload)).toEqual([]);
  expect(last.page!.done).toBe(true);
});

test("lease payloads decode to grant, release, and check results", () => {
  const grant = decodeLeaseGrant(decodeFrame(golden("lease-grant-response")).response!.payload);
  expect(termEquals(grant.fence, FENCE)).toBe(true);
  expect(grant.expires.seconds).toBe(1785000000);
  expect(instantToMillis(grant.expires)).toBe(1785000000123);
  expect(decodeLeaseReleased(
    decodeFrame(golden("lease-released-response")).response!.payload,
  )).toBe(true);
  const check = decodeLeaseCheck(
    decodeFrame(golden("lease-check-response")).response!.payload,
  );
  expect(check.valid).toBe(true);
  expect(check.expires).toEqual(framInstant(1785000060, 0));
  const absent = decodeLeaseCheck(
    decodeFrame(golden("lease-check-absent-response")).response!.payload,
  );
  expect(absent.valid).toBe(false);
  expect(absent.expires).toBeNull();
});

test("a batch response decodes one ordered action-result per action", () => {
  const results = decodeMutationResult(
    decodeFrame(golden("batch-response")).response!.payload,
  );
  expect(results.map((r) => [r.inputIndex, r.changed])).toEqual([[0, true], [1, false]]);
  expect(termEquals(results[0]!.occurrence, occurrence(0))).toBe(true);
  expect(termEquals(results[1]!.occurrence, occurrence(1))).toBe(true);
});

test("status decodes the served state, live count, engine, and cache", () => {
  const status = decodeStatus(decodeFrame(golden("status-response")).response!.payload);
  expect(status.state.name).toBe("serving");
  expect(status.liveCount).toBe(1234);
  expect(status.engine.name).toBe("native");
  expect(status.cache).toEqual({ hits: 1, misses: 2, bytes: 3, evictions: 4 });
});

test("every Term atom survives the Clojure encoding", () => {
  const rows = decodeTriples(decodeFrame(golden("term-atoms-response")).response!.payload);
  const first = rows[0] as FramTriple;
  expect(first.t1).toBe(Number.MIN_SAFE_INTEGER);
  expect((first.t2 as FramFloat).value).toBe(1.5);
  expect(first.t3).toBe(true);
  const second = rows[1] as FramTriple;
  expect(second.t1).toBe(false);
  expect((second.t2 as Keyword).name).toBe("rpc/unit");
  expect(second.t3).toBe("naïve 😀");
  const third = rows[2] as FramTriple;
  expect(third.t1).toEqual(framInstant(0, 0));
  expect(third.t2).toEqual(framInstant(-1, 999999999));
});

// --- ontology ---------------------------------------------------------------

test("RPC lists and options are ordinary recursive triples", () => {
  expect(rpcListValues(rpcList(["a", 1, true]))).toEqual(["a", 1, true]);
  expect(rpcListValues(rpcList([]))).toEqual([]);
  expect(rpcOptionValue(rpcOption(null))).toBeNull();
  expect(rpcOptionValue(rpcOption("x"))).toBe("x");
  expect(() => rpcListValues(triple("a", "b", "c")))
    .toThrow("RPC list must end with :rpc/list-end");
});

// --- bounds -----------------------------------------------------------------

/** Splice a payload's raw bytes into the request-frame template, so decode-side
 * bounds can be probed with terms the encoder itself refuses to build. */
function requestFrameWithPayloadBytes(payload: Uint8Array): Uint8Array {
  const template = encodeRequestFrame(10, request({ op: kw("rpc/batch"), payload: "leaf" }));
  const leaf = Uint8Array.from([1, 4, 0, 0, 0, 0x6c, 0x65, 0x61, 0x66]);
  const body = template.subarray(RPC_V2_HEADER_BYTES);
  const prefix = body.subarray(0, body.length - leaf.length);
  const frame = new Uint8Array(RPC_V2_HEADER_BYTES + prefix.length + payload.length);
  frame.set(template.subarray(0, RPC_V2_HEADER_BYTES));
  frame.set(prefix, RPC_V2_HEADER_BYTES);
  frame.set(payload, RPC_V2_HEADER_BYTES + prefix.length);
  // Body length is header offset 14 (8 magic + 2 major + 2 minor + kind + flags).
  new DataView(frame.buffer).setUint32(14, prefix.length + payload.length, true);
  return frame;
}

function nestedPayloadBytes(depth: number): Uint8Array {
  let bytes = Uint8Array.from([1, 4, 0, 0, 0, 0x6c, 0x65, 0x61, 0x66]);
  for (let index = 0; index < depth; index += 1) {
    const wrapped = new Uint8Array(bytes.length + 3);
    wrapped[0] = 7;
    wrapped.set(bytes, 1);
    wrapped[bytes.length + 1] = 4;
    wrapped[bytes.length + 2] = 5;
    bytes = wrapped;
  }
  return bytes;
}

test("term depth 256 encodes and decodes; 257 is refused on both sides", () => {
  expect(base64(encodeRequestFrame(10, request({
    op: kw("rpc/batch"), payload: nest(RPC_V2_MAX_TERM_DEPTH),
  })))).toBe(GOLDEN["deep-term-request-256"]);
  expect(decodeFrame(golden("deep-term-request-256")).request!.payload)
    .toBeInstanceOf(FramTriple);
  expect(() => encodeRequestFrame(10, request({
    op: kw("rpc/batch"), payload: nest(RPC_V2_MAX_TERM_DEPTH + 1),
  }))).toThrow("recursive Term exceeds the TermCodecV1 depth bound");
  const tooDeep = requestFrameWithPayloadBytes(
    nestedPayloadBytes(RPC_V2_MAX_TERM_DEPTH + 1),
  );
  expect(() => decodeFrame(tooDeep))
    .toThrow("recursive Term exceeds the TermCodecV1 depth bound");
  expect(decodeFrame(requestFrameWithPayloadBytes(
    nestedPayloadBytes(RPC_V2_MAX_TERM_DEPTH),
  )).request!.payload).toBeInstanceOf(FramTriple);
});

test("an i64 outside the safe JavaScript range is refused, never rounded", () => {
  const payload = new Uint8Array(9);
  payload[0] = 2;
  new DataView(payload.buffer).setBigInt64(1, 1n << 60n, true);
  try {
    decodeFrame(requestFrameWithPayloadBytes(payload));
    throw new Error("expected an integer-range refusal");
  } catch (error) {
    expect(error).toBeInstanceOf(FramRpcCodecError);
    expect((error as FramRpcCodecError).code).toBe("term-codec-integer-range");
  }
});

test("framing invariants fail closed", () => {
  const frame = golden("version-request");
  const badMagic = Uint8Array.from(frame);
  badMagic[0] = 0x47;
  expect(() => decodeFrame(badMagic)).toThrow("FRAMRPC magic does not match");
  const badVersion = Uint8Array.from(frame);
  badVersion[10] = 2;
  expect(() => decodeFrame(badVersion)).toThrow("major/minor version is unsupported");
  const badFlags = Uint8Array.from(frame);
  badFlags[13] = 1;
  expect(() => decodeFrame(badFlags)).toThrow("flags must be zero");
  expect(() => decodeFrame(frame.subarray(0, frame.length - 1)))
    .toThrow("FRAMRPC body is shorter than declared");
  const trailing = new Uint8Array(frame.length + 1);
  trailing.set(frame);
  expect(() => decodeFrame(trailing)).toThrow("bytes beyond its declared body");
});

test("an unpaired surrogate and an over-long SpaceId are refused", () => {
  expect(() => encodeRequestFrame(1, request({
    op: kw("rpc/batch"), payload: "\ud800",
  }))).toThrow("unpaired UTF-16 surrogate");
  expect(() => encodeRequestFrame(1, {
    ...request({ op: kw("rpc/version"), payload: RPC_UNIT }),
    space: "s".repeat(4097),
  })).toThrow("SpaceId exceeds the UTF-8 byte limit");
});

// --- client: typed errors, retries, transport ambiguity ---------------------

interface Recorder { calls: FramRpcTransportInput[] }

function replayTransport(
  frames: string[], recorder: Recorder,
): (input: FramRpcTransportInput) => Promise<RpcResponse> {
  let index = 0;
  return async (input) => {
    recorder.calls.push(input);
    const name = frames[Math.min(index, frames.length - 1)]!;
    index += 1;
    const decoded = decodeFrame(golden(name)).response!;
    return { ...decoded, space: input.request.space, op: input.request.op };
  };
}

const testClient = (
  transport: (input: FramRpcTransportInput) => Promise<RpcResponse>,
) => FramRpcClient.create({
  port: 1, spaceId: SPACE, transport, retryDelayMs: 0, jitterMs: 0,
});

for (const [name, code, retryable] of [
  ["conflict-error-response", "rpc/conflict", true],
  ["lease-fence-mismatch-error-response", "rpc/lease-fence-mismatch", false],
  ["durability-ambiguous-error-response", "durability-ambiguous", true],
  ["space-mismatch-error-response", "rpc/space-mismatch", false],
  ["lease-held-error-response", "rpc/lease-held", false],
] as const) {
  test(`${code} decodes as a typed refusal carrying its served version`, async () => {
    const recorder: Recorder = { calls: [] };
    const client = testClient(replayTransport([name], recorder));
    try {
      await client.batch([{ op: "assert", proposition: triple("@a", "b", "c") }]);
      throw new Error("expected a typed refusal");
    } catch (error) {
      expect(error).toBeInstanceOf(FramRpcServerError);
      const typed = error as FramRpcServerError;
      expect(typed.code).toBe(code);
      expect(typed.retryable).toBe(retryable);
      expect(typed.servedVersion).toBeGreaterThan(-1);
      expect(recorder.calls.length).toBe(1);
    }
  });
}

test("durability-ambiguous carries its detail Term to the caller", async () => {
  const recorder: Recorder = { calls: [] };
  const client = testClient(replayTransport(["durability-ambiguous-error-response"], recorder));
  const error = await client
    .batch([{ op: "assert", proposition: triple("@a", "b", "c") }])
    .then(() => null, (caught) => caught as FramRpcServerError);
  expect(error!.detail).toBeInstanceOf(FramTriple);
});

test("a mutation whose bytes reached the socket is never re-asked", async () => {
  const recorder: Recorder = { calls: [] };
  const client = testClient(async (input) => {
    recorder.calls.push(input);
    throw new FramRpcTransportError(
      "rpc-truncated", "response lost", true, input.request.op.name, 1,
    );
  });
  const error = await client
    .batch([{ op: "assert", proposition: triple("@a", "b", "c") }])
    .then(() => null, (caught) => caught as FramRpcTransportError);
  expect(error).toBeInstanceOf(FramRpcTransportError);
  expect(error!.requestSent).toBe(true);
  expect(error!.op).toBe("rpc/batch");
  expect(recorder.calls.length).toBe(1);
});

test("a mutation that provably never left the process is re-asked", async () => {
  const recorder: Recorder = { calls: [] };
  const client = testClient(async (input) => {
    recorder.calls.push(input);
    throw new FramRpcTransportError(
      "ECONNREFUSED", "never sent", false, input.request.op.name, 1,
    );
  });
  const error = await client
    .leaseAcquire(RESOURCE, HOLDER, 60000)
    .then(() => null, (caught) => caught as FramRpcTransportError);
  expect(error!.requestSent).toBe(false);
  expect(error!.attempts).toBe(3);
  expect(recorder.calls.length).toBe(3);
});

test("a read is re-asked even when its request was sent", async () => {
  const recorder: Recorder = { calls: [] };
  let failures = 0;
  const client = testClient(async (input) => {
    recorder.calls.push(input);
    if (failures < 2) {
      failures += 1;
      throw new FramRpcTransportError(
        "rpc-timeout", "read timed out", true, input.request.op.name, 1,
      );
    }
    const decoded = decodeFrame(golden("version-response")).response!;
    return { ...decoded, space: input.request.space, op: input.request.op };
  });
  const result = await client.version();
  expect(result.servedVersion).toBe(42);
  expect(result.attempts).toBe(3);
  expect(recorder.calls.length).toBe(3);
});

test("connect refuses a coordinator serving another SpaceId", async () => {
  const recorder: Recorder = { calls: [] };
  const error = await FramRpcClient
    .connect({
      port: 1, spaceId: SPACE, retryDelayMs: 0, jitterMs: 0,
      transport: replayTransport(["space-mismatch-error-response"], recorder),
    })
    .then(() => null, (caught) => caught as FramRpcServerError);
  expect(error).toBeInstanceOf(FramRpcServerError);
  expect(error!.code).toBe("rpc/space-mismatch");
  expect(recorder.calls.length).toBe(1);
  expect(recorder.calls[0]!.request.space).toBe(SPACE);
});

test("connect proves the served space and keeps the client usable", async () => {
  const recorder: Recorder = { calls: [] };
  const client = await FramRpcClient.connect({
    port: 1, spaceId: SPACE, retryDelayMs: 0, jitterMs: 0,
    transport: replayTransport(["status-response"], recorder),
  });
  expect(client.closed).toBe(false);
  expect(client.spaceId).toBe(SPACE);
});

test("a paged scan drains its cursor and refuses a torn snapshot", async () => {
  const recorder: Recorder = { calls: [] };
  const client = testClient(
    replayTransport(["scan-response-page", "scan-response-final"], recorder),
  );
  const drained = await client.scanAll("@agent:x", null, null);
  expect(drained.rows.length).toBe(2);
  expect(drained.pages).toBe(2);
  expect(drained.servedVersion).toBe(42);
  expect(recorder.calls[0]!.request.page).toEqual({ limit: 200, cursor: null });
  expect(termEquals(recorder.calls[1]!.request.page!.cursor, CURSOR)).toBe(true);

  const torn = testClient(async (input) => {
    const first = decodeFrame(golden("scan-response-page")).response!;
    const later = decodeFrame(golden("scan-response-final")).response!;
    const base = recorder.calls.push(input) > 1 ? later : first;
    return {
      ...base, space: input.request.space, op: input.request.op,
      servedVersion: base === later ? 99 : 42,
    };
  });
  recorder.calls.length = 0;
  await expect(torn.scanAll("@agent:x", null, null))
    .rejects.toThrow("page drain changed snapshot");
});

test("a batch request carries its fence and expected-version to the wire", async () => {
  const recorder: Recorder = { calls: [] };
  const client = testClient(replayTransport(["batch-response"], recorder));
  const result = await client.batch(
    [{ op: "assert", proposition: triple("@agent:x", "role", "worker") }],
    { fence: FENCE, expectedVersion: 99 },
  );
  expect(result.servedVersion).toBe(46);
  const sent = recorder.calls[0]!.request;
  expect(sent.expectedVersion).toBe(99);
  expect(termEquals(sent.payload, rpcBatch(
    [{ op: "assert", proposition: triple("@agent:x", "role", "worker") }], FENCE,
  ))).toBe(true);
});

// --- socket transport -------------------------------------------------------

async function withFramedServer(
  reply: (frame: ReturnType<typeof decodeFrame>) => Uint8Array | null,
  body: (port: number) => Promise<void>,
): Promise<void> {
  const server = createServer((socket) => {
    const chunks: Buffer[] = [];
    socket.on("data", (chunk) => {
      chunks.push(chunk);
      const buffer = Buffer.concat(chunks);
      if (buffer.length < RPC_V2_HEADER_BYTES) return;
      const declared = new DataView(
        buffer.buffer, buffer.byteOffset, buffer.length,
      ).getUint32(14, true);
      if (buffer.length < RPC_V2_HEADER_BYTES + declared) return;
      const bytes = reply(decodeFrame(Uint8Array.from(buffer)));
      if (bytes === null) socket.destroy();
      else socket.end(Buffer.from(bytes));
    });
    socket.on("error", () => { /* client-side assertions own the outcome */ });
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  if (address === null || typeof address === "string") throw new Error("no port");
  try {
    await body(address.port);
  } finally {
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
}

test("the socket transport round-trips one frame against a live listener", async () => {
  await withFramedServer(
    (frame) => encodeResponseFrame(frame.requestId, response({
      op: frame.request!.op, servedVersion: 7, payload: RPC_UNIT,
    })),
    async (port) => {
      const client = FramRpcClient.create({ port, spaceId: SPACE, retryDelayMs: 0, jitterMs: 0 });
      expect((await client.version()).servedVersion).toBe(7);
    },
  );
});

test("a listener that closes mid-exchange reports the request as sent", async () => {
  await withFramedServer(() => null, async (port) => {
    const error = await socketRoundTrip({
      host: "127.0.0.1", port, requestId: 1,
      request: request({ op: kw("rpc/batch"), payload: RPC_UNIT }),
      connectTimeoutMs: 2000, readTimeoutMs: 2000,
    }).then(() => null, (caught) => caught as FramRpcTransportError);
    expect(error).toBeInstanceOf(FramRpcTransportError);
    expect(error!.requestSent).toBe(true);
  });
});

test("a refused connection reports the request as never sent", async () => {
  let closedPort = 0;
  await withFramedServer(() => null, async (port) => { closedPort = port; });
  const error = await socketRoundTrip({
    host: "127.0.0.1", port: closedPort, requestId: 1,
    request: request({ op: kw("rpc/batch"), payload: RPC_UNIT }),
    connectTimeoutMs: 2000, readTimeoutMs: 2000,
  }).then(() => null, (caught) => caught as FramRpcTransportError);
  expect(error).toBeInstanceOf(FramRpcTransportError);
  expect(error!.requestSent).toBe(false);
});

test("a mismatched response identity is a transport failure, not a result", async () => {
  await withFramedServer(
    (frame) => encodeResponseFrame(frame.requestId, response({
      op: kw("rpc/status"), servedVersion: 7, payload: RPC_UNIT,
    })),
    async (port) => {
      const error = await socketRoundTrip({
        host: "127.0.0.1", port, requestId: 1,
        request: request({ op: kw("rpc/version"), payload: RPC_UNIT }),
        connectTimeoutMs: 2000, readTimeoutMs: 2000,
      }).then(() => null, (caught) => caught as FramRpcTransportError);
      expect(error!.code).toBe("rpc-response-mismatch");
      expect(error!.requestSent).toBe(true);
    },
  );
});

// --- environment ------------------------------------------------------------

test("FRAM_SPACE_ID selects the space and defaults to north-coordination", () => {
  const previous = process.env.FRAM_SPACE_ID;
  try {
    delete process.env.FRAM_SPACE_ID;
    expect(framSpaceId()).toBe("north-coordination");
    process.env.FRAM_SPACE_ID = "north-graph-lane-abc";
    expect(framSpaceId()).toBe("north-graph-lane-abc");
    expect(nativeRouteForSubject("@agent:x").spaceId).toBe("north-graph-lane-abc");
  } finally {
    if (previous === undefined) delete process.env.FRAM_SPACE_ID;
    else process.env.FRAM_SPACE_ID = previous;
  }
});
