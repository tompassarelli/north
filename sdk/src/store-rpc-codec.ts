// Binary Store RPC v2 codec. Every byte here MUST match beagle:src/
// store/rpc.bclj (TermCodecV1 + Store RPC v2) — one server decoder parses
// this module and north:cli/store-rpc-client.clj alike; test/fixtures golden
// packets come from that Clojure encoder.
// Packet encoding only: no IO, no retry, no publication orchestration.
// Fail-closed divergences: an Int outside Number.MAX_SAFE_INTEGER is refused
// rather than rounded, and one aggregate node budget reports every overflow as
// `rpc-term-node-limit`.
import { Keyword, kw } from "./coord-wire";

/** Recursive Term constructor — the only structural vocabulary the daemon reads. */
export class StoreTriple {
  constructor(readonly t1: Term, readonly t2: Term, readonly t3: Term) {}
}
export const triple = (t1: Term, t2: Term, t3: Term): StoreTriple =>
  new StoreTriple(t1, t2, t3);

/** Float atom (tag 3). Explicit because JS cannot tell 1.0 from 1: Clojure
 * encodes `1.0` as a Float and `1` as an Int, and a round trip must not move a
 * value between tags. */
export class StoreFloat {
  constructor(readonly value: number) {}
}
export const storeFloat = (value: number): StoreFloat => new StoreFloat(value);

/** Instant atom (tag 8): i64 epoch seconds + u32 nanos below 1e9. */
export class StoreInstant {
  constructor(readonly seconds: number, readonly nanos: number) {}
}
export const storeInstant = (seconds: number, nanos: number): StoreInstant =>
  new StoreInstant(seconds, nanos);

export type Term =
  | string | number | boolean | Keyword | StoreTriple | StoreFloat | StoreInstant;

/** A local packet encoding/codec failure. `code` carries Store's own keyword spelling
 * (without the colon) so a wire mismatch is diagnosable against the spec. */
export class StoreRpcCodecError extends Error {
  constructor(readonly code: string, message: string) {
    super(message);
    this.name = "StoreRpcCodecError";
  }
}

function fail(code: string, message: string): never {
  throw new StoreRpcCodecError(code, message);
}

// --- limits (beagle:src/store/rpc.bclj Store RPC v2) -------------------------------
export const RPC_V2_MAJOR = 2;
export const RPC_V2_MINOR = 0;
export const RPC_V2_HEADER_BYTES = 26;
export const RPC_V2_MAX_BODY_BYTES = 1048576;
export const RPC_V2_MAX_PACKET_BYTES = 1048602;
export const RPC_V2_MAX_STRING_BYTES = 1048576;
export const RPC_V2_MAX_SPACE_BYTES = 4096;
export const RPC_V2_MAX_TERM_NODES = 65536;
export const RPC_V2_MAX_TERM_DEPTH = 256;
export const RPC_V2_MAGIC = Uint8Array.from([0x46, 0x52, 0x41, 0x4d, 0x52, 0x50, 0x43, 0x00]);

export type RpcPacketKind = "request" | "response" | "cancel" | "event";

const KIND_CODES: Record<RpcPacketKind, number> = {
  request: 1, response: 2, cancel: 3, event: 4,
};
const CODE_KINDS: Record<number, RpcPacketKind> = {
  1: "request", 2: "response", 3: "cancel", 4: "event",
};

export interface RpcPageRequest { limit: number; cursor: Term | null }
export interface RpcPageResponse { ordinal: number; cursor: Term | null; done: boolean }
export interface RpcErrorTerm {
  code: Keyword; retryable: boolean; message: string; detail: Term | null;
}
export interface RpcRequest {
  space: string;
  op: Keyword;
  expectedVersion: number | null;
  page: RpcPageRequest | null;
  timeoutMs: number | null;
  payload: Term;
}
export interface RpcResponse {
  space: string;
  op: Keyword;
  servedVersion: number;
  page: RpcPageResponse | null;
  error: RpcErrorTerm | null;
  payload: Term | null;
}
export interface RpcPacket {
  kind: RpcPacketKind;
  requestId: number;
  request: RpcRequest | null;
  response: RpcResponse | null;
}

// --- byte plumbing ----------------------------------------------------------

class ByteWriter {
  private buffer = new Uint8Array(512);
  private length = 0;

  private reserve(extra: number): void {
    if (this.length + extra <= this.buffer.length) return;
    let capacity = this.buffer.length * 2;
    while (capacity < this.length + extra) capacity *= 2;
    const grown = new Uint8Array(capacity);
    grown.set(this.buffer.subarray(0, this.length));
    this.buffer = grown;
  }

  u8(value: number): void {
    this.reserve(1);
    this.buffer[this.length++] = value & 0xff;
  }

  u16(value: number): void {
    this.u8(value);
    this.u8(value >>> 8);
  }

  u32(value: number): void {
    if (!Number.isInteger(value) || value < 0 || value > 4294967295)
      fail("term-codec-integer-range", "u32 value is out of range");
    this.u8(value);
    this.u8(Math.floor(value / 0x100));
    this.u8(Math.floor(value / 0x10000));
    this.u8(Math.floor(value / 0x1000000));
  }

  i64(value: number): void {
    if (!Number.isSafeInteger(value))
      fail("term-codec-integer-range",
           "i64 value is not a safe JavaScript integer");
    let word = BigInt.asUintN(64, BigInt(value));
    for (let index = 0; index < 8; index += 1) {
      this.u8(Number(word & 0xffn));
      word >>= 8n;
    }
  }

  f64(value: number): void {
    const scratch = new DataView(new ArrayBuffer(8));
    scratch.setFloat64(0, value, true);
    for (let index = 0; index < 8; index += 1) this.u8(scratch.getUint8(index));
  }

  raw(bytes: Uint8Array): void {
    this.reserve(bytes.length);
    this.buffer.set(bytes, this.length);
    this.length += bytes.length;
  }

  size(): number { return this.length; }

  bytes(): Uint8Array { return this.buffer.slice(0, this.length); }
}

class ByteReader {
  private offset = 0;
  constructor(private readonly buffer: Uint8Array) {}

  remaining(): number { return this.buffer.length - this.offset; }

  private ensure(count: number, context: string): void {
    if (this.remaining() < count)
      fail("rpc-truncated", `Store RPC ended inside ${context}`);
  }

  u8(context: string): number {
    this.ensure(1, context);
    return this.buffer[this.offset++]!;
  }

  u16(context: string): number {
    return this.u8(context) | (this.u8(context) << 8);
  }

  u32(context: string): number {
    this.ensure(4, context);
    const value = this.buffer[this.offset]!
      + this.buffer[this.offset + 1]! * 0x100
      + this.buffer[this.offset + 2]! * 0x10000
      + this.buffer[this.offset + 3]! * 0x1000000;
    this.offset += 4;
    return value;
  }

  i64(context: string): number {
    this.ensure(8, context);
    let word = 0n;
    for (let index = 7; index >= 0; index -= 1)
      word = (word << 8n) | BigInt(this.buffer[this.offset + index]!);
    this.offset += 8;
    const signed = BigInt.asIntN(64, word);
    if (signed > BigInt(Number.MAX_SAFE_INTEGER) || signed < BigInt(Number.MIN_SAFE_INTEGER))
      fail("term-codec-integer-range",
           `${context} is outside the safe JavaScript integer range`);
    return Number(signed);
  }

  f64(context: string): number {
    this.ensure(8, context);
    const view = new DataView(
      this.buffer.buffer, this.buffer.byteOffset + this.offset, 8,
    );
    this.offset += 8;
    return view.getFloat64(0, true);
  }

  raw(count: number, context: string): Uint8Array {
    this.ensure(count, context);
    const slice = this.buffer.subarray(this.offset, this.offset + count);
    this.offset += count;
    return slice;
  }

  presence(context: string): boolean {
    const value = this.u8(context);
    if (value === 0) return false;
    if (value === 1) return true;
    return fail("rpc-invalid-marker", `${context} must be the strict byte 0 or 1`);
  }

  bool(context: string): boolean {
    const value = this.u8(context);
    if (value === 0) return false;
    if (value === 1) return true;
    return fail("rpc-invalid-boolean", `${context} must be the strict byte 0 or 1`);
  }
}

const TEXT_ENCODER = new TextEncoder();
const TEXT_DECODER = new TextDecoder("utf-8", { fatal: true });

/** Mirror of Store's `utf8-length!`: reject unpaired surrogates BEFORE encoding,
 * because TextEncoder would silently substitute U+FFFD and change the bytes the
 * daemon hashes. */
function utf8Bytes(value: string, maximum: number, label: string): Uint8Array {
  let total = 0;
  for (let index = 0; index < value.length; index += 1) {
    const unit = value.charCodeAt(index);
    const high = unit >= 0xd800 && unit <= 0xdbff;
    const low = unit >= 0xdc00 && unit <= 0xdfff;
    const next = high && index + 1 < value.length ? value.charCodeAt(index + 1) : -1;
    const paired = high && next >= 0xdc00 && next <= 0xdfff;
    let width: number;
    if (unit <= 0x7f) width = 1;
    else if (unit <= 0x7ff) width = 2;
    else if (paired) width = 4;
    else if (high || low) width = -1;
    else width = 3;
    if (width === -1)
      fail("term-codec-invalid-utf8", `${label} contains an unpaired UTF-16 surrogate`);
    total += width;
    if (total > maximum)
      fail("term-codec-string-limit", `${label} exceeds the UTF-8 byte limit`);
    if (paired) index += 1;
  }
  return TEXT_ENCODER.encode(value);
}

function utf8String(bytes: Uint8Array, context: string): string {
  try {
    return TEXT_DECODER.decode(bytes);
  } catch {
    return fail("term-codec-invalid-utf8", `${context} is not valid UTF-8`);
  }
}

// --- TermCodecV1 ------------------------------------------------------------

interface NodeBudget { used: number }

function countNode(budget: NodeBudget): void {
  budget.used += 1;
  if (budget.used > RPC_V2_MAX_TERM_NODES)
    fail("rpc-term-node-limit",
         "Store RPC body exceeds the aggregate Term node limit");
}

function writeSizedText(
  writer: ByteWriter, value: string, maxStringBytes: number, label: string,
): void {
  const bytes = utf8Bytes(value, maxStringBytes, label);
  writer.u32(bytes.length);
  writer.raw(bytes);
}

function writeTerm(
  writer: ByteWriter, term: Term, depth: number, budget: NodeBudget,
  maxStringBytes = RPC_V2_MAX_STRING_BYTES,
): void {
  if (depth > RPC_V2_MAX_TERM_DEPTH)
    fail("term-depth-exceeded",
         "recursive Term exceeds the TermCodecV1 depth bound");
  countNode(budget);
  if (term instanceof StoreTriple) {
    writer.u8(7);
    writeTerm(writer, term.t1, depth + 1, budget, maxStringBytes);
    writeTerm(writer, term.t2, depth + 1, budget, maxStringBytes);
    writeTerm(writer, term.t3, depth + 1, budget, maxStringBytes);
    return;
  }
  if (typeof term === "string") {
    writer.u8(1);
    writeSizedText(writer, term, maxStringBytes, "String atom");
    return;
  }
  if (typeof term === "number") {
    if (!Number.isInteger(term))
      fail("term-codec-unsupported-term",
           "a non-integer number must be wrapped in StoreFloat");
    writer.u8(2);
    writer.i64(term);
    return;
  }
  if (term instanceof StoreFloat) {
    writer.u8(3);
    writer.f64(term.value);
    return;
  }
  if (term === false) { writer.u8(4); return; }
  if (term === true) { writer.u8(5); return; }
  if (term instanceof Keyword) {
    if (term.name.length === 0)
      fail("term-codec-invalid-keyword", "Keyword atom spelling must be nonempty");
    writer.u8(6);
    writeSizedText(writer, term.name, maxStringBytes, "Keyword atom");
    return;
  }
  if (term instanceof StoreInstant) {
    writer.u8(8);
    writer.i64(term.seconds);
    if (!Number.isInteger(term.nanos) || term.nanos < 0 || term.nanos >= 1000000000)
      fail("term-codec-invalid-instant",
           "Instant nanoseconds are outside the canonical range");
    writer.u32(term.nanos);
    return;
  }
  fail("term-codec-unsupported-term",
       "TermCodecV1 encountered a value outside Term");
}

function readSizedText(
  reader: ByteReader, maxStringBytes: number, context: string,
): string {
  const length = reader.u32(context);
  if (length > maxStringBytes)
    fail("term-codec-string-limit", `${context} exceeds the UTF-8 byte limit`);
  return utf8String(reader.raw(length, context), context);
}

function readTerm(
  reader: ByteReader, depth: number, budget: NodeBudget,
  maxStringBytes = RPC_V2_MAX_STRING_BYTES,
): Term {
  if (depth > RPC_V2_MAX_TERM_DEPTH)
    fail("term-depth-exceeded",
         "recursive Term exceeds the TermCodecV1 depth bound");
  countNode(budget);
  const tag = reader.u8("Term tag");
  switch (tag) {
    case 1: return readSizedText(reader, maxStringBytes, "String atom");
    case 2: return reader.i64("Int atom");
    case 3: return new StoreFloat(reader.f64("Float atom"));
    case 4: return false;
    case 5: return true;
    case 6: {
      const spelling = readSizedText(reader, maxStringBytes, "Keyword atom");
      if (spelling.length === 0)
        fail("term-codec-invalid-keyword", "Keyword atom spelling must be nonempty");
      return kw(spelling);
    }
    case 7: {
      const t1 = readTerm(reader, depth + 1, budget, maxStringBytes);
      const t2 = readTerm(reader, depth + 1, budget, maxStringBytes);
      const t3 = readTerm(reader, depth + 1, budget, maxStringBytes);
      return new StoreTriple(t1, t2, t3);
    }
    case 8: {
      const seconds = reader.i64("Instant atom");
      const nanos = reader.u32("Instant nanos");
      if (nanos >= 1000000000)
        fail("term-codec-invalid-instant",
             "Instant nanoseconds are outside the canonical range");
      return new StoreInstant(seconds, nanos);
    }
    default:
      return fail("term-codec-bad-tag", "TermCodecV1 contains an unknown tag");
  }
}

/** Structural Term equality — the comparison an exact-projection readback needs;
 * `===` is wrong for every composite Term and for Keyword. */
export function termEquals(left: Term | null, right: Term | null): boolean {
  if (left === right) return true;
  if (left === null || right === null) return false;
  if (left instanceof Keyword && right instanceof Keyword)
    return left.name === right.name;
  if (left instanceof StoreTriple && right instanceof StoreTriple)
    return termEquals(left.t1, right.t1)
      && termEquals(left.t2, right.t2)
      && termEquals(left.t3, right.t3);
  if (left instanceof StoreFloat && right instanceof StoreFloat)
    return Object.is(left.value, right.value);
  if (left instanceof StoreInstant && right instanceof StoreInstant)
    return left.seconds === right.seconds && left.nanos === right.nanos;
  return false;
}

// --- packet encode -----------------------------------------------------------

function writeTermField(writer: ByteWriter, term: Term, budget: NodeBudget): void {
  writeTerm(writer, term, 0, budget);
}

function writePresent(writer: ByteWriter, value: unknown): void {
  writer.u8(value === null || value === undefined ? 0 : 1);
}

function requireSpace(space: string): void {
  if (typeof space !== "string")
    fail("rpc-invalid-field", "SpaceId must be a String");
  utf8Bytes(space, RPC_V2_MAX_SPACE_BYTES, "SpaceId");
}

function requireU32(value: number, label: string): void {
  if (!Number.isInteger(value) || value < 0 || value > 4294967295)
    fail("rpc-integer-range", `${label} is outside u32`);
}

function requireSafeI64(value: number, label: string): void {
  if (!Number.isSafeInteger(value))
    fail("rpc-integer-range", `${label} is outside the safe integer range`);
}

function writePageRequest(
  writer: ByteWriter, page: RpcPageRequest, budget: NodeBudget,
): void {
  requireU32(page.limit, "page limit");
  writer.u32(page.limit);
  writePresent(writer, page.cursor);
  if (page.cursor !== null) writeTermField(writer, page.cursor, budget);
}

function writePageResponse(
  writer: ByteWriter, page: RpcPageResponse, budget: NodeBudget,
): void {
  requireU32(page.ordinal, "page ordinal");
  writer.u32(page.ordinal);
  writePresent(writer, page.cursor);
  if (page.cursor !== null) writeTermField(writer, page.cursor, budget);
  writer.u8(page.done ? 1 : 0);
}

function writeError(
  writer: ByteWriter, error: RpcErrorTerm, budget: NodeBudget,
): void {
  writeTermField(writer, error.code, budget);
  writer.u8(error.retryable ? 1 : 0);
  writeTermField(writer, error.message, budget);
  writePresent(writer, error.detail);
  if (error.detail !== null) writeTermField(writer, error.detail, budget);
}

function writeRequestBody(writer: ByteWriter, request: RpcRequest): void {
  const budget: NodeBudget = { used: 0 };
  requireSpace(request.space);
  writeTermField(writer, request.space, budget);
  writeTermField(writer, request.op, budget);
  writePresent(writer, request.expectedVersion);
  if (request.expectedVersion !== null) {
    requireSafeI64(request.expectedVersion, "expected version");
    writer.i64(request.expectedVersion);
  }
  writePresent(writer, request.page);
  if (request.page !== null) writePageRequest(writer, request.page, budget);
  writePresent(writer, request.timeoutMs);
  if (request.timeoutMs !== null) {
    requireU32(request.timeoutMs, "timeout-ms");
    writer.u32(request.timeoutMs);
  }
  writeTermField(writer, request.payload, budget);
}

function writeResponseBody(writer: ByteWriter, response: RpcResponse): void {
  const budget: NodeBudget = { used: 0 };
  requireSpace(response.space);
  writeTermField(writer, response.space, budget);
  writeTermField(writer, response.op, budget);
  requireSafeI64(response.servedVersion, "served version");
  writer.i64(response.servedVersion);
  writePresent(writer, response.page);
  if (response.page !== null) writePageResponse(writer, response.page, budget);
  writePresent(writer, response.error);
  if (response.error !== null) writeError(writer, response.error, budget);
  writePresent(writer, response.payload);
  if (response.payload !== null && response.payload !== undefined)
    writeTermField(writer, response.payload, budget);
}

export function encodePacket(packet: RpcPacket): Uint8Array {
  requireSafeI64(packet.requestId, "request id");
  const body = new ByteWriter();
  if (packet.kind === "request") {
    if (packet.request === null || packet.response !== null)
      fail("rpc-invalid-shape", "request packet must carry exactly one RpcRequest");
    writeRequestBody(body, packet.request);
  } else if (packet.kind === "response" || packet.kind === "event") {
    if (packet.response === null || packet.request !== null)
      fail("rpc-invalid-shape",
           "response/event packet must carry exactly one RpcResponse");
    writeResponseBody(body, packet.response);
  } else if (packet.kind === "cancel") {
    if (packet.request !== null || packet.response !== null)
      fail("rpc-invalid-shape", "cancel packet body must be empty");
  } else {
    fail("rpc-invalid-kind", "Store RPC packet kind is unknown");
  }
  if (body.size() > RPC_V2_MAX_BODY_BYTES)
    fail("rpc-packet-too-large", "Store RPC body exceeds the configured byte limit");
  const out = new ByteWriter();
  out.raw(RPC_V2_MAGIC);
  out.u16(RPC_V2_MAJOR);
  out.u16(RPC_V2_MINOR);
  out.u8(KIND_CODES[packet.kind]);
  out.u8(0);
  out.u32(body.size());
  out.i64(packet.requestId);
  out.raw(body.bytes());
  return out.bytes();
}

export function encodeRequestPacket(requestId: number, request: RpcRequest): Uint8Array {
  return encodePacket({ kind: "request", requestId, request, response: null });
}

export function encodeResponsePacket(requestId: number, response: RpcResponse): Uint8Array {
  return encodePacket({ kind: "response", requestId, request: null, response });
}

export function encodeCancelPacket(requestId: number): Uint8Array {
  return encodePacket({ kind: "cancel", requestId, request: null, response: null });
}

// --- packet decode -----------------------------------------------------------

function readSpaceTerm(reader: ByteReader, budget: NodeBudget): string {
  countNode(budget);
  const tag = reader.u8("SpaceId Term tag");
  if (tag !== 1)
    fail("rpc-invalid-field", "Store RPC SpaceId must be a String Term");
  return readSizedText(reader, RPC_V2_MAX_SPACE_BYTES, "SpaceId");
}

function readKeywordTerm(
  reader: ByteReader, budget: NodeBudget, context: string,
): Keyword {
  const value = readTerm(reader, 0, budget);
  if (!(value instanceof Keyword))
    fail("rpc-invalid-field", `${context} must be a Keyword Term`);
  return value as Keyword;
}

function readStringTerm(
  reader: ByteReader, budget: NodeBudget, context: string,
): string {
  const value = readTerm(reader, 0, budget);
  if (typeof value !== "string")
    fail("rpc-invalid-field", `${context} must be a String Term`);
  return value;
}

function readRequestBody(reader: ByteReader, budget: NodeBudget): RpcRequest {
  const space = readSpaceTerm(reader, budget);
  const op = readKeywordTerm(reader, budget, "request op");
  const expectedVersion = reader.presence("expected-version marker")
    ? reader.i64("expected-version") : null;
  let page: RpcPageRequest | null = null;
  if (reader.presence("request page marker")) {
    const limit = reader.u32("page limit");
    const cursor = reader.presence("page cursor marker")
      ? readTerm(reader, 0, budget) : null;
    page = { limit, cursor };
  }
  const timeoutMs = reader.presence("timeout-ms marker")
    ? reader.u32("timeout-ms") : null;
  const payload = readTerm(reader, 0, budget);
  return { space, op, expectedVersion, page, timeoutMs, payload };
}

function readResponseBody(reader: ByteReader, budget: NodeBudget): RpcResponse {
  const space = readSpaceTerm(reader, budget);
  const op = readKeywordTerm(reader, budget, "response op");
  const servedVersion = reader.i64("served-version");
  let page: RpcPageResponse | null = null;
  if (reader.presence("response page marker")) {
    const ordinal = reader.u32("page ordinal");
    const cursor = reader.presence("next cursor marker")
      ? readTerm(reader, 0, budget) : null;
    const done = reader.bool("page done");
    page = { ordinal, cursor, done };
  }
  let error: RpcErrorTerm | null = null;
  if (reader.presence("response error marker")) {
    const code = readKeywordTerm(reader, budget, "error code");
    const retryable = reader.bool("error retryable");
    const message = readStringTerm(reader, budget, "error message");
    const detail = reader.presence("error detail marker")
      ? readTerm(reader, 0, budget) : null;
    error = { code, retryable, message, detail };
  }
  const payload = reader.presence("response payload marker")
    ? readTerm(reader, 0, budget) : null;
  return { space, op, servedVersion, page, error, payload };
}

/** Decode ONE complete packet. The caller must supply exactly the packet bytes:
 * trailing bytes are a protocol error, never a second packet, because the daemon
 * serves one request per connection. */
export function decodePacket(bytes: Uint8Array): RpcPacket {
  if (bytes.length > RPC_V2_MAX_PACKET_BYTES)
    fail("rpc-packet-too-large", "Store RPC packet exceeds the configured byte limit");
  if (bytes.length < RPC_V2_HEADER_BYTES)
    fail("rpc-truncated", "Store RPC packet ended inside its header");
  const reader = new ByteReader(bytes);
  for (let index = 0; index < RPC_V2_MAGIC.length; index += 1) {
    if (reader.u8("magic") !== RPC_V2_MAGIC[index])
      fail("rpc-invalid-magic", "Store RPC magic does not match");
  }
  const major = reader.u16("major version");
  const minor = reader.u16("minor version");
  const kindCode = reader.u8("packet kind");
  const flags = reader.u8("packet flags");
  const bodyLength = reader.u32("body length");
  const requestId = reader.i64("request id");
  const kind = CODE_KINDS[kindCode];
  if (kind === undefined)
    fail("rpc-invalid-kind", "Store RPC packet kind is unknown");
  if (major !== RPC_V2_MAJOR || minor !== RPC_V2_MINOR)
    fail("rpc-unsupported-version", "Store RPC major/minor version is unsupported");
  if (flags !== 0)
    fail("rpc-invalid-flags", "Store RPC v2 flags must be zero");
  if (bodyLength > RPC_V2_MAX_BODY_BYTES)
    fail("rpc-packet-too-large",
         "Store RPC declared body exceeds the configured byte limit");
  if (reader.remaining() < bodyLength)
    fail("rpc-truncated", "Store RPC body is shorter than declared");
  if (reader.remaining() > bodyLength)
    fail("rpc-trailing-bytes", "Store RPC packet has bytes beyond its declared body");
  const budget: NodeBudget = { used: 0 };
  let packet: RpcPacket;
  if (kind === "request") {
    packet = {
      kind, requestId, request: readRequestBody(reader, budget), response: null,
    };
  } else if (kind === "response" || kind === "event") {
    packet = {
      kind, requestId, request: null, response: readResponseBody(reader, budget),
    };
  } else {
    if (bodyLength !== 0)
      fail("rpc-invalid-shape", "Store RPC cancel body must be exactly empty");
    packet = { kind, requestId, request: null, response: null };
  }
  if (reader.remaining() !== 0)
    fail("rpc-trailing-bytes", "Store RPC body decoder left trailing bytes");
  return packet;
}

/** Body length declared by a complete 26-byte header, with every header
 * invariant the reference client checks. Lets the transport size ONE read
 * without allocating an untrusted declared body. */
export function decodePacketHeader(
  header: Uint8Array,
): { kind: RpcPacketKind; requestId: number; bodyLength: number } {
  if (header.length < RPC_V2_HEADER_BYTES)
    fail("rpc-truncated", "Store RPC packet ended inside its header");
  const reader = new ByteReader(header);
  for (let index = 0; index < RPC_V2_MAGIC.length; index += 1) {
    if (reader.u8("magic") !== RPC_V2_MAGIC[index])
      fail("rpc-invalid-magic", "Store RPC magic does not match");
  }
  const major = reader.u16("major version");
  const minor = reader.u16("minor version");
  const kindCode = reader.u8("packet kind");
  const flags = reader.u8("packet flags");
  const bodyLength = reader.u32("body length");
  const requestId = reader.i64("request id");
  const kind = CODE_KINDS[kindCode];
  if (major !== RPC_V2_MAJOR || minor !== RPC_V2_MINOR)
    fail("rpc-unsupported-version", "Store RPC major/minor version is unsupported");
  if (kind === undefined)
    fail("rpc-invalid-kind", "Store RPC packet kind is unknown");
  if (flags !== 0)
    fail("rpc-invalid-flags", "Store RPC v2 flags must be zero");
  if (bodyLength > RPC_V2_MAX_BODY_BYTES)
    fail("rpc-packet-too-large",
         "Store RPC declared body exceeds the configured byte limit");
  return { kind, requestId, bodyLength };
}

// --- native RPC ontology ----------------------------------------------------
// Records, options, and lists are ordinary recursive Triple terms; these helpers
// are the only structural vocabulary the daemon understands.

export const RPC_UNIT = kw("rpc/unit");
export const RPC_LIST = kw("rpc/list");
export const RPC_LIST_END = kw("rpc/list-end");
export const RPC_NONE = kw("rpc/none");
export const RPC_SOME = kw("rpc/some");
export const RPC_OPTION = kw("rpc/option");
export const RPC_RECORD = kw("rpc/record");
export const RPC_SUBJECT_ANY = kw("rpc/subject-any");
export const RPC_SUBJECT_EXISTING = kw("rpc/subject-existing");

export type SubjectPolicy = typeof RPC_SUBJECT_ANY | typeof RPC_SUBJECT_EXISTING;

export function rpcList(values: readonly Term[]): Term {
  let tail: Term = RPC_LIST_END;
  for (let index = values.length - 1; index >= 0; index -= 1)
    tail = new StoreTriple(RPC_LIST, values[index]!, tail);
  return tail;
}

export function rpcListValues(value: Term): Term[] {
  const result: Term[] = [];
  let cursor: Term = value;
  for (;;) {
    if (cursor instanceof Keyword && cursor.name === RPC_LIST_END.name) return result;
    if (result.length >= RPC_V2_MAX_TERM_NODES)
      fail("rpc-invalid-list", "RPC list exceeds the Term node bound");
    if (cursor instanceof StoreTriple
        && cursor.t1 instanceof Keyword && cursor.t1.name === RPC_LIST.name) {
      result.push(cursor.t2);
      cursor = cursor.t3;
      continue;
    }
    return fail("rpc-invalid-list", "RPC list must end with :rpc/list-end");
  }
}

export function rpcSome(value: Term): StoreTriple {
  return new StoreTriple(RPC_SOME, value, RPC_OPTION);
}

export function rpcOption(value: Term | null): Term {
  return value === null || value === undefined ? RPC_NONE : rpcSome(value);
}

export function rpcOptionPresent(value: Term): boolean {
  if (value instanceof Keyword && value.name === RPC_NONE.name) return false;
  if (value instanceof StoreTriple
      && value.t1 instanceof Keyword && value.t1.name === RPC_SOME.name
      && value.t3 instanceof Keyword && value.t3.name === RPC_OPTION.name)
    return true;
  return fail("rpc-invalid-option",
              "RPC option must be :rpc/none or (:rpc/some value :rpc/option)");
}

export function rpcOptionValue(value: Term): Term | null {
  return rpcOptionPresent(value) ? (value as StoreTriple).t2 : null;
}

export function rpcRecord(tag: Keyword, fields: readonly Term[]): StoreTriple {
  return new StoreTriple(tag, rpcList(fields), RPC_RECORD);
}

export function rpcRecordFields(
  value: Term | null, tag: Keyword, fieldCount: number,
): Term[] {
  if (!(value instanceof StoreTriple)
      || !(value.t1 instanceof Keyword) || value.t1.name !== tag.name
      || !(value.t3 instanceof Keyword) || value.t3.name !== RPC_RECORD.name)
    fail("rpc-invalid-record", "RPC record tag or marker is invalid");
  const fields = rpcListValues((value as StoreTriple).t2);
  if (fields.length !== fieldCount)
    fail("rpc-invalid-record", "RPC record contains the wrong number of fields");
  return fields;
}

/** `(R,H,epoch)` — the native fence every fenced mutation carries. */
export function rpcFence(resource: Term, holder: Term, epoch: number): StoreTriple {
  return rpcRecord(kw("rpc/fence"), [resource, holder, epoch]);
}

export interface ParsedFence { resource: Term; holder: Term; epoch: number }

export function parseFence(term: Term | null): ParsedFence {
  const [resource, holder, epoch] = rpcRecordFields(term, kw("rpc/fence"), 3);
  if (typeof epoch !== "number")
    fail("rpc-invalid-field", "lease fence epoch must be an Int Term");
  return { resource: resource!, holder: holder!, epoch };
}

export type MutationOp = "assert" | "retract";

export interface BatchAction {
  op: MutationOp;
  proposition: StoreTriple;
  policy?: Keyword;
}

export function rpcAction(action: BatchAction): StoreTriple {
  return rpcRecord(kw("rpc/action"), [
    kw(action.op === "assert" ? "rpc/assert" : "rpc/retract"),
    action.proposition,
    action.policy ?? RPC_SUBJECT_ANY,
  ]);
}

export function rpcBatch(actions: readonly BatchAction[], fence: Term | null): StoreTriple {
  return rpcRecord(kw("rpc/batch"), [
    rpcList(actions.map(rpcAction)), rpcOption(fence),
  ]);
}

export function rpcTriplePattern(
  t1: Term | null, t2: Term | null, t3: Term | null,
): StoreTriple {
  return rpcRecord(kw("rpc/triple-pattern"), [
    rpcOption(t1), rpcOption(t2), rpcOption(t3),
  ]);
}

export function rpcLeaseAcquire(
  resource: Term, holder: Term, ttlMs: number,
): StoreTriple {
  return rpcRecord(kw("lease/acquire"), [resource, holder, ttlMs]);
}

export function rpcLeaseRenew(fence: Term, ttlMs: number): StoreTriple {
  return rpcRecord(kw("lease/renew"), [fence, ttlMs]);
}

export interface ActionResult {
  inputIndex: number;
  changed: boolean;
  occurrence: StoreTriple;
}

function requireOccurrenceCoordinate(value: Term): StoreTriple {
  if (!(value instanceof StoreTriple)
      || !(value.t1 instanceof StoreTriple)
      || typeof value.t1.t1 !== "string" || value.t1.t1.length === 0
      || !(value.t1.t2 instanceof Keyword)
      || value.t1.t2.name !== "kernel/tx-sequence"
      || !Number.isSafeInteger(value.t1.t3) || (value.t1.t3 as number) < 0
      || !(value.t2 instanceof Keyword) || value.t2.name !== "kernel/op-ordinal"
      || !Number.isSafeInteger(value.t3) || (value.t3 as number) < 0) {
    fail("rpc-invalid-occurrence",
         "rpc/action-result requires one occurrence coordinate");
  }
  return value;
}

/** Decode `:rpc/mutation-result` — one entry per submitted action, in order. */
export function decodeMutationResult(payload: Term | null): ActionResult[] {
  const [encodedResults] = rpcRecordFields(payload, kw("rpc/mutation-result"), 1);
  return rpcListValues(encodedResults!).map((encoded) => {
    const [inputIndex, changed, occurrence] =
      rpcRecordFields(encoded, kw("rpc/action-result"), 3);
    if (!Number.isSafeInteger(inputIndex) || (inputIndex as number) < 0
        || typeof changed !== "boolean")
      fail("rpc-invalid-record", "rpc/action-result fields are mistyped");
    return {
      inputIndex: inputIndex as number,
      changed,
      occurrence: requireOccurrenceCoordinate(occurrence!),
    };
  });
}

/** Decode `:rpc/triples` (the rpc/scan payload) into its row Terms. */
export function decodeTriples(payload: Term | null): Term[] {
  const [rows] = rpcRecordFields(payload, kw("rpc/triples"), 1);
  return rpcListValues(rows!);
}

export interface LeaseGrant { fence: StoreTriple; expires: StoreInstant }

export function decodeLeaseGrant(payload: Term | null): LeaseGrant {
  const [fence, expires] = rpcRecordFields(payload, kw("lease/grant"), 2);
  if (!(fence instanceof StoreTriple) || !(expires instanceof StoreInstant))
    fail("rpc-invalid-record", "lease/grant fields are mistyped");
  return { fence: fence as StoreTriple, expires: expires as StoreInstant };
}

export function decodeLeaseReleased(payload: Term | null): boolean {
  const [released] = rpcRecordFields(payload, kw("lease/released"), 1);
  if (typeof released !== "boolean")
    fail("rpc-invalid-record", "lease/released field is mistyped");
  return released;
}

export interface LeaseCheck { valid: boolean; expires: StoreInstant | null }

export function decodeLeaseCheck(payload: Term | null): LeaseCheck {
  const [valid, expiresOption] = rpcRecordFields(payload, kw("lease/check"), 2);
  if (typeof valid !== "boolean")
    fail("rpc-invalid-record", "lease/check validity is mistyped");
  const expires = rpcOptionValue(expiresOption!);
  if (expires !== null && !(expires instanceof StoreInstant))
    fail("rpc-invalid-record", "lease/check expiry is mistyped");
  return { valid, expires: expires as StoreInstant | null };
}

export interface RpcStatus {
  state: Keyword;
  liveCount: number;
  engine: Keyword;
  cache: { hits: number; misses: number; bytes: number; evictions: number };
}

export function decodeStatus(payload: Term | null): RpcStatus {
  const [state, liveCount, engine, encodedCache] =
    rpcRecordFields(payload, kw("rpc/status"), 4);
  const [hits, misses, bytes, evictions] =
    rpcRecordFields(encodedCache!, kw("rpc/result-cache"), 4);
  if (!(state instanceof Keyword) || !(engine instanceof Keyword)
      || typeof liveCount !== "number" || typeof hits !== "number"
      || typeof misses !== "number" || typeof bytes !== "number"
      || typeof evictions !== "number")
    fail("rpc-invalid-record", "rpc/status fields are mistyped");
  return {
    state: state as Keyword,
    liveCount,
    engine: engine as Keyword,
    cache: { hits, misses, bytes, evictions },
  };
}

/** Instant → epoch milliseconds, for callers whose deadlines are JS clocks. */
export function instantToMillis(instant: StoreInstant): number {
  return instant.seconds * 1000 + Math.floor(instant.nanos / 1000000);
}
