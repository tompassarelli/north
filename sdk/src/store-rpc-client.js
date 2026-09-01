import { str as $$bc$str } from './bridge/generated/beagle/core.js';
import { js_obj as $$bh$js_obj } from './bridge/generated/beagle/host.js';
import { catch_dispatch as $$bd$catch_dispatch } from './bridge/generated/beagle/exception-dispatch.js';

const coord_wire_module = require("./coord-wire");

const coord_port = coord_wire_module.coordPort;

const store_space_id = coord_wire_module.storeSpaceId;

const kw = coord_wire_module.kw;

const codec_module = require("./store-rpc-codec");

const decode_packet = codec_module.decodePacket;

const decode_packet_header = codec_module.decodePacketHeader;

const decode_lease_check = codec_module.decodeLeaseCheck;

const decode_lease_grant = codec_module.decodeLeaseGrant;

const decode_lease_released = codec_module.decodeLeaseReleased;

const decode_mutation_result = codec_module.decodeMutationResult;

const decode_status = codec_module.decodeStatus;

const decode_triples = codec_module.decodeTriples;

const encode_request_packet = codec_module.encodeRequestPacket;

const rpc_batch = codec_module.rpcBatch;

const rpc_lease_acquire = codec_module.rpcLeaseAcquire;

const rpc_lease_renew = codec_module.rpcLeaseRenew;

const rpc_triple_pattern = codec_module.rpcTriplePattern;

const RPC_UNIT = codec_module.RPC_UNIT;

const RPC_V2_HEADER_BYTES = codec_module.RPC_V2_HEADER_BYTES;

const net_module = require("node:net");

const net_connect = net_module.connect;

function store_rpc_server_error_new_bang(code, message, retryable, served_version, detail, op, attempts) {
  const error = new Error(message);
  Object.setPrototypeOf(error, store_rpc_server_error_new_bang.prototype);
  (error.name = "StoreRpcServerError");
  (error.code = code);
  (error.retryable = retryable);
  (error.servedVersion = served_version);
  (error.detail = detail);
  (error.op = op);
  (error.attempts = attempts);
  return error;
}

Object.setPrototypeOf(store_rpc_server_error_new_bang.prototype, Error.prototype);

const StoreRpcServerError = store_rpc_server_error_new_bang;

function store_rpc_transport_error_new_bang(code, message, request_sent, op, attempts, options) {
  const error = new Error(message);
  Object.setPrototypeOf(error, store_rpc_transport_error_new_bang.prototype);
  (error.name = "StoreRpcTransportError");
  (error.code = code);
  (error.requestSent = request_sent);
  (error.op = op);
  (error.attempts = attempts);
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (!(options.cause === undefined)) : _logical))(options))) {
    (error.cause = options.cause);
  }
  return error;
}

Object.setPrototypeOf(store_rpc_transport_error_new_bang.prototype, Error.prototype);

const StoreRpcTransportError = store_rpc_transport_error_new_bang;

function store_rpc_transport_error_p(value) {
  return ((value instanceof Error) && store_rpc_transport_error_new_bang.prototype.isPrototypeOf(value));
}

const RETRYABLE__ERROR__CODES = new Set(["rpc/conflict", "rpc/cancelled", "query-cancelled", "query-time-limit", "query-work-limit", "query/archive-unavailable", "durability-ambiguous"]);

const NEVER_AUTO_RETRIED = new Set(["rpc/conflict", "durability-ambiguous"]);

const MUTATION_OPS = new Set(["rpc/assert", "rpc/retract", "rpc/batch", "rpc/lease-acquire", "rpc/lease-renew", "rpc/lease-release"]);

const EFFECTIVE__PAGE__LIMIT = 200;

const REQUEST_ID_SEQUENCE_WIDTH = 2147483648;

const REQUEST_ID_MAX_PID = 4194303;

const REQUEST_ID_MAX_SEQUENCE = 2147483647;

const request_sequence_state = $$bh$js_obj("value", 0);

function storeRpcRequestId(pid, sequence) {
  if (((!Number.isSafeInteger(pid)) || ((pid < 1) || (pid > REQUEST_ID_MAX_PID)))) {
    (() => { throw new Error("Store RPC process PID is outside the request-id namespace"); })();
  }
  if (((!Number.isSafeInteger(sequence)) || ((sequence < 1) || (sequence > REQUEST_ID_MAX_SEQUENCE)))) {
    (() => { throw new Error("Store RPC sequence is outside the request-id namespace"); })();
  }
  const request_id = ((pid * REQUEST_ID_SEQUENCE_WIDTH) + sequence);
  if ((!Number.isSafeInteger(request_id))) {
    (() => { throw new Error("Store RPC request id is not a safe integer"); })();
  }
  return request_id;
}

function next_request_id_bang() {
  const sequence = (request_sequence_state.value + 1);
  (request_sequence_state.value = sequence);
  return storeRpcRequestId(process.pid, sequence);
}

function positive_integer_bang(label, value) {
  if (((!Number.isSafeInteger(value)) || (value <= 0))) {
    (() => { throw new Error($$bc$str("Store RPC ", label, " must be a positive integer")); })();
  }
  return value;
}

function non_negative_integer_bang(label, value) {
  if (((!Number.isSafeInteger(value)) || (value < 0))) {
    (() => { throw new Error($$bc$str("Store RPC ", label, " must be a non-negative integer")); })();
  }
  return value;
}

function delay_bang(ms) {
  const deferred = Promise.withResolvers();
  setTimeout(() => { deferred.resolve(null);
return null; }, ms);
  return deferred.promise;
}

function transport_code(cause) {
  const code = (((_truthy) => _truthy !== false && _truthy != null)(cause) ? (() => { return cause.code; })() : null);
  return ((typeof code === "string") ? code : "rpc-transport-failed");
}

function socket_round_trip_bang(input) {
  const deferred = Promise.withResolvers();
  const op = input.request.op.name;
  const state = $$bh$js_obj("sent", false, "settled", false, "socket", undefined, "chunks", [], "received", 0, "bodyLength", null);
  const fail_with = (code, message, cause) => { if ((!((_truthy) => _truthy !== false && _truthy != null)(state.settled))) {
  (state.settled = true);
  if (((_truthy) => _truthy !== false && _truthy != null)(state.socket)) {
    state.socket.destroy();
  }
  deferred.reject(store_rpc_transport_error_new_bang(code, message, (((_truthy) => _truthy !== false && _truthy != null)(state.sent) ? true : false), op, 1, $$bh$js_obj("cause", cause)));
}
return null; };
  const succeed = (response) => { if ((!((_truthy) => _truthy !== false && _truthy != null)(state.settled))) {
  (state.settled = true);
  state.socket.destroy();
  deferred.resolve(response);
}
return null; };
  (() => { try {
    const bytes = encode_request_packet(input.requestId, input.request);
  (() => { try {
    (state.socket = net_connect($$bh$js_obj("host", input.host, "port", input.port)));
  const socket = state.socket;
  socket.setTimeout(positive_integer_bang("connect-timeout-ms", input.connectTimeoutMs));
  socket.on("timeout", () => fail_with("rpc-timeout", (((_truthy) => _truthy !== false && _truthy != null)(state.sent) ? "Store RPC read timed out" : "Store RPC connect timed out"), undefined));
  socket.on("error", (cause) => fail_with(transport_code(cause), $$bc$str("Store RPC transport failed: ", cause), cause));
  socket.on("close", () => fail_with("rpc-truncated", "Store RPC connection closed before a complete response", undefined));
  socket.on("connect", () => { socket.setTimeout(input.readTimeoutMs);
(state.sent = true);
socket.write(bytes);
return null; });
  return socket.on("data", (chunk) => { state.chunks.push(chunk);
(state.received = (state.received + chunk.length));
(() => { try {
    if (((!((_truthy) => _truthy !== false && _truthy != null)(state.bodyLength)) && (state.received >= RPC_V2_HEADER_BYTES))) {
    const joined = Buffer.concat(state.chunks);
    const header = Uint8Array.from(joined.subarray(0, RPC_V2_HEADER_BYTES));
    (state.bodyLength = decode_packet_header(header).bodyLength);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(state.bodyLength)) {
    const total = (RPC_V2_HEADER_BYTES + state.bodyLength);
    return (((state.received < total)) ? null : ((state.received > total)) ? fail_with("rpc-trailing-bytes", "Store RPC response carried bytes beyond its declared body", undefined) : (() => { const packet = decode_packet(Uint8Array.from(Buffer.concat(state.chunks))); return ((((!(packet.kind === "response")) || (!((_truthy) => _truthy !== false && _truthy != null)(packet.response)))) ? fail_with("rpc-invalid-kind", "Store RPC unary request received a non-response packet", undefined) : ((!(packet.requestId === input.requestId))) ? fail_with("rpc-request-id-mismatch", "Store RPC response request-id does not match", undefined) : (((!(packet.response.space === input.request.space)) || (!(packet.response.op.name === input.request.op.name)))) ? fail_with("rpc-response-mismatch", "Store RPC response identity does not match its request", undefined) : succeed(packet.response)); })());
  }
  } catch (_catch_0) {
    switch ($$bd$catch_dispatch(_catch_0, [Error])) {
      case 0: {
        const cause = _catch_0;
        return fail_with(transport_code(cause), $$bc$str("Store RPC response is undecodable: ", cause), cause);
        break;
      }
    }
  } })();
return null; });
  } catch (_catch_1) {
    switch ($$bd$catch_dispatch(_catch_1, [Error])) {
      case 0: {
        const cause = _catch_1;
        return deferred.reject(store_rpc_transport_error_new_bang(transport_code(cause), $$bc$str("Store RPC connect failed: ", cause), false, op, 1, $$bh$js_obj("cause", cause)));
        break;
      }
    }
  } })();
  return null;
  } catch (_catch_2) {
    switch ($$bd$catch_dispatch(_catch_2, [Error])) {
      case 0: {
        const cause = _catch_2;
        return deferred.reject(store_rpc_transport_error_new_bang(transport_code(cause), $$bc$str("Store RPC request is not encodable: ", cause), false, op, 1, $$bh$js_obj("cause", cause)));
        break;
      }
    }
  } })();
  return deferred.promise;
}

const socketRoundTrip = socket_round_trip_bang;

function retry_pause_bang(state, attempt) {
  const jitter_ms = state.jitterMs;
  const jitter = ((jitter_ms > 0) ? Math.floor((Math.random() * (jitter_ms + 1))) : 0);
  return delay_bang(((attempt * state.retryDelayMs) + jitter));
}

async function request_bang(state, op, payload, raw_options) {
  if (((_truthy) => _truthy !== false && _truthy != null)(state.closed)) {
    (() => { throw new Error("Store RPC client is closed"); })();
  }
  const request = $$bh$js_obj("space", state.spaceId, "op", op, "expectedVersion", (((_truthy) => _truthy !== false && _truthy != null)(raw_options) ? ((_logical) => (_logical !== false && _logical != null ? _logical : null))(raw_options.expectedVersion) : null), "page", (((_truthy) => _truthy !== false && _truthy != null)(raw_options) ? ((_logical) => (_logical !== false && _logical != null ? _logical : null))(raw_options.page) : null), "timeoutMs", (((_truthy) => _truthy !== false && _truthy != null)(raw_options) ? ((_logical) => (_logical !== false && _logical != null ? _logical : null))(raw_options.timeoutMs) : null), "payload", payload);
  const mutation = MUTATION_OPS.has(op.name);
  return (async () => { let attempt = 1; while (true) {
    { let _loop_try_result_0; try {
    const transport = state.transport; const response = await transport($$bh$js_obj("host", state.host, "port", state.port, "requestId", next_request_id_bang(), "request", request, "connectTimeoutMs", state.connectTimeoutMs, "readTimeoutMs", Math.max(state.readTimeoutMs, (1000 + ((_logical) => (_logical !== false && _logical != null ? _logical : 0))(request.timeoutMs))))); const error = response.error; if ((!((_truthy) => _truthy !== false && _truthy != null)(error))) { _loop_try_result_0 = $$bh$js_obj("response", response, "attempts", attempt); } else { const code = error.code.name; const retryable = ((_logical) => (_logical !== false && _logical != null ? RETRYABLE__ERROR__CODES.has(code) : _logical))(error.retryable); if ((retryable && ((!((_truthy) => _truthy !== false && _truthy != null)(NEVER_AUTO_RETRIED.has(code))) && (attempt < state.maxAttempts)))) { await retry_pause_bang(state, attempt); const _recur_0 = (attempt + 1); attempt = _recur_0; continue; } else { _loop_try_result_0 = (() => { throw store_rpc_server_error_new_bang(code, error.message, error.retryable, response.servedVersion, error.detail, op.name, attempt); })(); } }
  } catch (_catch_3) {
    switch ($$bd$catch_dispatch(_catch_3, [Error])) {
      case 0: {
        const caught = _catch_3;
        const transport_error = (store_rpc_transport_error_p(caught) ? store_rpc_transport_error_new_bang(caught.code, caught.message, caught.requestSent, op.name, attempt, $$bh$js_obj("cause", caught.cause)) : caught); ((!store_rpc_transport_error_p(transport_error)) ? (() => { return (() => { throw transport_error; })(); })() : null); const resendable = ((!mutation) || (!((_truthy) => _truthy !== false && _truthy != null)(transport_error.requestSent))); if ((resendable && (attempt < state.maxAttempts))) { await retry_pause_bang(state, attempt); const _recur_0 = (attempt + 1); attempt = _recur_0; continue; } else { _loop_try_result_0 = (() => { throw transport_error; })(); }
        break;
      }
    }
  } return _loop_try_result_0; }
  } })();
}

async function version_bang(state) {
  const result = await request_bang(state, kw("rpc/version"), RPC_UNIT, null);
  return $$bh$js_obj("servedVersion", result.response.servedVersion, "attempts", result.attempts);
}

async function status_bang(state) {
  const result = await request_bang(state, kw("rpc/status"), RPC_UNIT, null);
  const response = result.response;
  const status = decode_status(response.payload);
  return $$bh$js_obj("state", status.state, "liveCount", status.liveCount, "engine", status.engine, "cache", status.cache, "spaceId", state.spaceId, "servedVersion", response.servedVersion, "attempts", result.attempts);
}

async function scan_bang(state, t1, t2, t3, options) {
  const result = await request_bang(state, kw("rpc/scan"), rpc_triple_pattern(t1, t2, t3), options);
  const response = result.response;
  return $$bh$js_obj("rows", decode_triples(response.payload), "page", response.page, "servedVersion", response.servedVersion, "attempts", result.attempts);
}

async function scan_all_bang(state, t1, t2, t3, raw_options) {
  const page_size = positive_integer_bang("page-size", (((_truthy) => _truthy !== false && _truthy != null)(raw_options) ? ((_logical) => (_logical !== false && _logical != null ? _logical : EFFECTIVE__PAGE__LIMIT))(raw_options.pageSize) : EFFECTIVE__PAGE__LIMIT));
  if ((page_size > EFFECTIVE__PAGE__LIMIT)) {
    (() => { throw new Error("Store RPC page size exceeds the current TermCodec-safe limit"); })();
  }
  return (async () => { let cursor = null; let rows = []; let pages = 0; let snapshot = null; let attempts = 0; while (true) {
    const page = await scan_bang(state, t1, t2, t3, $$bh$js_obj("page", $$bh$js_obj("limit", page_size, "cursor", cursor))); const metadata = page.page; ((!((_truthy) => _truthy !== false && _truthy != null)(metadata)) ? (() => { return (() => { throw new Error("Store RPC paged operation omitted page metadata"); })(); })() : null); const next_snapshot = ((_logical) => (_logical !== false && _logical != null ? _logical : page.servedVersion))(snapshot); ((!(next_snapshot === page.servedVersion)) ? (() => { return (() => { throw new Error($$bc$str("Store RPC page drain changed snapshot: ", next_snapshot, " -> ", page.servedVersion)); })(); })() : null); const next_rows = rows.concat(page.rows); const next_pages = (pages + 1); const next_attempts = (attempts + page.attempts); if (((_truthy) => _truthy !== false && _truthy != null)(metadata.done)) { return $$bh$js_obj("rows", next_rows, "servedVersion", next_snapshot, "pages", next_pages, "attempts", next_attempts); } else { const _recur_0 = metadata.cursor; const _recur_1 = next_rows; const _recur_2 = next_pages; const _recur_3 = next_snapshot; const _recur_4 = next_attempts; cursor = _recur_0; rows = _recur_1; pages = _recur_2; snapshot = _recur_3; attempts = _recur_4; continue; }
  } })();
}

async function batch_bang(state, actions, raw_options) {
  const result = await request_bang(state, kw("rpc/batch"), rpc_batch(actions, (((_truthy) => _truthy !== false && _truthy != null)(raw_options) ? ((_logical) => (_logical !== false && _logical != null ? _logical : null))(raw_options.fence) : null)), $$bh$js_obj("expectedVersion", (((_truthy) => _truthy !== false && _truthy != null)(raw_options) ? ((_logical) => (_logical !== false && _logical != null ? _logical : null))(raw_options.expectedVersion) : null), "page", (((_truthy) => _truthy !== false && _truthy != null)(raw_options) ? ((_logical) => (_logical !== false && _logical != null ? _logical : null))(raw_options.page) : null), "timeoutMs", (((_truthy) => _truthy !== false && _truthy != null)(raw_options) ? ((_logical) => (_logical !== false && _logical != null ? _logical : null))(raw_options.timeoutMs) : null)));
  const response = result.response;
  return $$bh$js_obj("results", decode_mutation_result(response.payload), "servedVersion", response.servedVersion, "attempts", result.attempts);
}

async function lease_acquire_bang(state, resource, holder, ttl_ms, options) {
  const result = await request_bang(state, kw("rpc/lease-acquire"), rpc_lease_acquire(resource, holder, ttl_ms), options);
  const response = result.response;
  const grant = decode_lease_grant(response.payload);
  return $$bh$js_obj("fence", grant.fence, "expires", grant.expires, "servedVersion", response.servedVersion, "attempts", result.attempts);
}

async function lease_renew_bang(state, fence, ttl_ms, options) {
  const result = await request_bang(state, kw("rpc/lease-renew"), rpc_lease_renew(fence, ttl_ms), options);
  const response = result.response;
  const grant = decode_lease_grant(response.payload);
  return $$bh$js_obj("fence", grant.fence, "expires", grant.expires, "servedVersion", response.servedVersion, "attempts", result.attempts);
}

async function lease_release_bang(state, fence, options) {
  const result = await request_bang(state, kw("rpc/lease-release"), fence, options);
  const response = result.response;
  return $$bh$js_obj("released", decode_lease_released(response.payload), "servedVersion", response.servedVersion, "attempts", result.attempts);
}

async function lease_check_bang(state, fence, options) {
  const result = await request_bang(state, kw("rpc/lease-check"), fence, options);
  const response = result.response;
  const check = decode_lease_check(response.payload);
  return $$bh$js_obj("valid", check.valid, "expires", check.expires, "servedVersion", response.servedVersion, "attempts", result.attempts);
}

function create_client_bang(raw_options) {
  const host = ((_logical) => (_logical !== false && _logical != null ? _logical : ((_logical) => (_logical !== false && _logical != null ? _logical : "127.0.0.1"))(process.env.NORTH_STORE_HOST)))((((_truthy) => _truthy !== false && _truthy != null)(raw_options) ? (() => { return raw_options.host; })() : null));
  const space_id = ((_logical) => (_logical !== false && _logical != null ? _logical : store_space_id()))((((_truthy) => _truthy !== false && _truthy != null)(raw_options) ? (() => { return raw_options.spaceId; })() : null));
  if ((host.length === 0)) {
    (() => { throw new Error("Store RPC host must be nonblank"); })();
  }
  if ((space_id.length === 0)) {
    (() => { throw new Error("Store RPC SpaceId must be nonblank"); })();
  }
  const state = $$bh$js_obj("closed", false, "host", host, "port", positive_integer_bang("port", (((_truthy) => _truthy !== false && _truthy != null)(raw_options) ? ((_logical) => (_logical !== false && _logical != null ? _logical : coord_port()))(raw_options.port) : coord_port())), "spaceId", space_id, "connectTimeoutMs", positive_integer_bang("connect-timeout-ms", (((_truthy) => _truthy !== false && _truthy != null)(raw_options) ? ((_logical) => (_logical !== false && _logical != null ? _logical : 2000))(raw_options.connectTimeoutMs) : 2000)), "readTimeoutMs", positive_integer_bang("read-timeout-ms", (((_truthy) => _truthy !== false && _truthy != null)(raw_options) ? ((_logical) => (_logical !== false && _logical != null ? _logical : 15000))(raw_options.readTimeoutMs) : 15000)), "maxAttempts", positive_integer_bang("max-attempts", (((_truthy) => _truthy !== false && _truthy != null)(raw_options) ? ((_logical) => (_logical !== false && _logical != null ? _logical : 3))(raw_options.maxAttempts) : 3)), "retryDelayMs", non_negative_integer_bang("retry-delay-ms", (((_truthy) => _truthy !== false && _truthy != null)(raw_options) ? ((_logical) => (_logical !== false && _logical != null ? _logical : 10))(raw_options.retryDelayMs) : 10)), "jitterMs", non_negative_integer_bang("jitter-ms", (((_truthy) => _truthy !== false && _truthy != null)(raw_options) ? ((_logical) => (_logical !== false && _logical != null ? _logical : 25))(raw_options.jitterMs) : 25)), "transport", (((_truthy) => _truthy !== false && _truthy != null)(raw_options) ? ((_logical) => (_logical !== false && _logical != null ? _logical : socketRoundTrip))(raw_options.transport) : socketRoundTrip));
  const client = $$bh$js_obj("host", state.host, "port", state.port, "spaceId", state.spaceId, "closed", false, "close", () => { (state.closed = true);
return null; }, "request", (op, payload, options) => request_bang(state, op, payload, options), "version", () => version_bang(state), "status", () => status_bang(state), "scan", (t1, t2, t3, options) => scan_bang(state, t1, t2, t3, options), "scanAll", (t1, t2, t3, options) => scan_all_bang(state, t1, t2, t3, options), "batch", (actions, options) => batch_bang(state, actions, options), "leaseAcquire", (resource, holder, ttl_ms, options) => lease_acquire_bang(state, resource, holder, ttl_ms, options), "leaseRenew", (fence, ttl_ms, options) => lease_renew_bang(state, fence, ttl_ms, options), "leaseRelease", (fence, options) => lease_release_bang(state, fence, options), "leaseCheck", (fence, options) => lease_check_bang(state, fence, options));
  Object.defineProperty(client, "closed", $$bh$js_obj("enumerable", true, "get", () => (((_truthy) => _truthy !== false && _truthy != null)(state.closed) ? true : false)));
  return client;
}

async function connect_client_bang(options) {
  const client = create_client_bang(options);
  const client_status = client.status;
  const client_close = client.close;
  return (async () => { try {
    await client_status();
  return client;
  } catch (_catch_4) {
    switch ($$bd$catch_dispatch(_catch_4, [Error])) {
      case 0: {
        const error = _catch_4;
        client_close();
        return (() => { throw error; })();
        break;
      }
    }
  } })();
}

const StoreRpcClient = Object.freeze($$bh$js_obj("create", create_client_bang, "connect", connect_client_bang));

export { EFFECTIVE__PAGE__LIMIT as "EFFECTIVE_PAGE_LIMIT" };
export { RETRYABLE__ERROR__CODES as "RETRYABLE_ERROR_CODES" };
export { StoreRpcClient as "StoreRpcClient" };
export { StoreRpcServerError as "StoreRpcServerError" };
export { StoreRpcTransportError as "StoreRpcTransportError" };
export { socketRoundTrip as "socketRoundTrip" };
export { storeRpcRequestId as "storeRpcRequestId" };
