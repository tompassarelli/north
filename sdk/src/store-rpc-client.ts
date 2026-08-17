// One-shot FRAMRPC v2 client, mirroring north:cli/store-rpc-client.clj. The server
// serves ONE request per connection. A mutation whose bytes reached the socket is
// never auto-retried: the caller gets `requestSent` and owns the resolution.
import { connect as netConnect, type Socket } from "node:net";
import { coordPort, storeSpaceId, kw, Keyword } from "./coord-wire";
import type {
  ActionResult, BatchAction, LeaseCheck, LeaseGrant, RpcPageRequest,
  RpcPageResponse, RpcRequest, RpcResponse, RpcStatus, Term,
} from "./store-rpc-codec";
import {
  decodeFrame, decodeFrameHeader, decodeLeaseCheck, decodeLeaseGrant,
  decodeLeaseReleased, decodeMutationResult, decodeStatus, decodeTriples,
  encodeRequestFrame, rpcBatch, rpcLeaseAcquire, rpcLeaseRenew,
  rpcTriplePattern, RPC_UNIT, RPC_V2_HEADER_BYTES,
} from "./store-rpc-codec";

/** A typed refusal the server put on the wire. */
export class FramRpcServerError extends Error {
  constructor(
    readonly code: string,
    message: string,
    readonly retryable: boolean,
    readonly servedVersion: number,
    readonly detail: Term | null,
    readonly op: string,
    readonly attempts: number,
  ) {
    super(message);
    this.name = "FramRpcServerError";
  }
}

/** A transport/framing failure. `requestSent` false means the bytes never left
 * this process — the ONLY case in which an unacknowledged mutation is provably
 * absent from the graph. */
export class FramRpcTransportError extends Error {
  constructor(
    readonly code: string,
    message: string,
    readonly requestSent: boolean,
    readonly op: string,
    readonly attempts: number,
    options?: { cause?: unknown },
  ) {
    super(message, options);
    this.name = "FramRpcTransportError";
  }
}

/** The canonical FRAMRPC retryable set; a code the
 * server marks retryable but the client omits strands a caller the server
 * expected to ask again. */
export const RETRYABLE_ERROR_CODES: ReadonlySet<string> = new Set([
  "rpc/conflict", "rpc/cancelled", "query-cancelled", "query-time-limit",
  "query-work-limit", "query/archive-unavailable", "durability-ambiguous",
]);

/** Codes that are an ANSWER, never a reason to ask the same question again. */
const NEVER_AUTO_RETRIED: ReadonlySet<string> = new Set([
  "rpc/conflict", "durability-ambiguous",
]);

const MUTATION_OPS: ReadonlySet<string> = new Set([
  "rpc/assert", "rpc/retract", "rpc/batch",
  "rpc/lease-acquire", "rpc/lease-renew", "rpc/lease-release",
]);

/** rpc/scan pages nest one Triple per row, so a page above this bound cannot be
 * encoded inside the shared Term depth limit. */
export const EFFECTIVE_PAGE_LIMIT = 200;

export interface FramRpcClientOptions {
  host?: string;
  port?: number;
  spaceId?: string;
  connectTimeoutMs?: number;
  readTimeoutMs?: number;
  maxAttempts?: number;
  retryDelayMs?: number;
  jitterMs?: number;
  /** Test/bench seam; defaults to the one-shot socket exchange. */
  transport?: FramRpcTransport;
}

export interface FramRpcTransportInput {
  host: string;
  port: number;
  requestId: number;
  request: RpcRequest;
  connectTimeoutMs: number;
  readTimeoutMs: number;
}

export type FramRpcTransport = (input: FramRpcTransportInput) => Promise<RpcResponse>;

export interface RequestOptions {
  expectedVersion?: number | null;
  page?: RpcPageRequest | null;
  timeoutMs?: number | null;
}

export interface ServedResult {
  servedVersion: number;
  attempts: number;
}

let requestSequence = 0;

function nextRequestId(): number {
  requestSequence += 1;
  if (requestSequence > Number.MAX_SAFE_INTEGER - 1) requestSequence = 1;
  return requestSequence;
}

function positiveInteger(label: string, value: number): number {
  if (!Number.isSafeInteger(value) || value <= 0)
    throw new Error(`FRAMRPC ${label} must be a positive integer`);
  return value;
}

function nonNegativeInteger(label: string, value: number): number {
  if (!Number.isSafeInteger(value) || value < 0)
    throw new Error(`FRAMRPC ${label} must be a non-negative integer`);
  return value;
}

const delay = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

function transportCode(cause: unknown): string {
  const code = (cause as { code?: unknown } | null)?.code;
  return typeof code === "string" ? code : "rpc-transport-failed";
}

/**
 * One unary socket exchange. Resolves with the decoded response; rejects with a
 * FramRpcTransportError whose `requestSent` distinguishes "never left this
 * process" from "on the wire, outcome unknown".
 */
export function socketRoundTrip(input: FramRpcTransportInput): Promise<RpcResponse> {
  const op = input.request.op.name;
  return new Promise<RpcResponse>((resolvePromise, reject) => {
    let bytes: Uint8Array;
    try {
      bytes = encodeRequestFrame(input.requestId, input.request);
    } catch (cause) {
      reject(new FramRpcTransportError(
        transportCode(cause), `FRAMRPC request is not encodable: ${String(cause)}`,
        false, op, 1, { cause },
      ));
      return;
    }
    let sent = false;
    let settled = false;
    let socket: Socket;
    const chunks: Buffer[] = [];
    let received = 0;
    let bodyLength: number | null = null;
    const failWith = (code: string, message: string, cause?: unknown) => {
      if (settled) return;
      settled = true;
      socket?.destroy();
      reject(new FramRpcTransportError(code, message, sent, op, 1, { cause }));
    };
    const succeed = (response: RpcResponse) => {
      if (settled) return;
      settled = true;
      socket.destroy();
      resolvePromise(response);
    };
    try {
      socket = netConnect({ host: input.host, port: input.port });
    } catch (cause) {
      reject(new FramRpcTransportError(
        transportCode(cause), `FRAMRPC connect failed: ${String(cause)}`,
        false, op, 1, { cause },
      ));
      return;
    }
    socket.setTimeout(positiveInteger("connect-timeout-ms", input.connectTimeoutMs));
    socket.on("timeout", () => failWith(
      "rpc-timeout",
      sent ? "FRAMRPC read timed out" : "FRAMRPC connect timed out",
    ));
    socket.on("error", (cause) => failWith(
      transportCode(cause), `FRAMRPC transport failed: ${String(cause)}`, cause,
    ));
    socket.on("close", () => failWith(
      "rpc-truncated", "FRAMRPC connection closed before a complete response",
    ));
    socket.on("connect", () => {
      socket.setTimeout(input.readTimeoutMs);
      sent = true;
      socket.write(bytes);
    });
    socket.on("data", (chunk: Buffer) => {
      chunks.push(chunk);
      received += chunk.length;
      try {
        if (bodyLength === null && received >= RPC_V2_HEADER_BYTES) {
          bodyLength = decodeFrameHeader(
            Uint8Array.from(Buffer.concat(chunks).subarray(0, RPC_V2_HEADER_BYTES)),
          ).bodyLength;
        }
        if (bodyLength === null) return;
        const total = RPC_V2_HEADER_BYTES + bodyLength;
        if (received < total) return;
        if (received > total) {
          failWith("rpc-trailing-bytes",
                   "FRAMRPC response carried bytes beyond its declared body");
          return;
        }
        const frame = decodeFrame(Uint8Array.from(Buffer.concat(chunks)));
        if (frame.kind !== "response" || frame.response === null) {
          failWith("rpc-invalid-kind",
                   "FRAMRPC unary request received a non-response frame");
          return;
        }
        if (frame.requestId !== input.requestId) {
          failWith("rpc-request-id-mismatch",
                   "FRAMRPC response request-id does not match");
          return;
        }
        const response = frame.response;
        if (response.space !== input.request.space
            || response.op.name !== input.request.op.name) {
          failWith("rpc-response-mismatch",
                   "FRAMRPC response identity does not match its request");
          return;
        }
        succeed(response);
      } catch (cause) {
        failWith(transportCode(cause),
                 `FRAMRPC response is undecodable: ${String(cause)}`, cause);
      }
    });
  });
}

export class FramRpcClient {
  private closedFlag = false;

  private constructor(
    readonly host: string,
    readonly port: number,
    readonly spaceId: string,
    private readonly connectTimeoutMs: number,
    private readonly readTimeoutMs: number,
    private readonly maxAttempts: number,
    private readonly retryDelayMs: number,
    private readonly jitterMs: number,
    private readonly transport: FramRpcTransport,
  ) {}

  /** Build a client WITHOUT probing the server. */
  static create(options: FramRpcClientOptions = {}): FramRpcClient {
    const host = options.host ?? process.env.NORTH_FRAMRPC_HOST ?? "127.0.0.1";
    if (host.length === 0) throw new Error("FRAMRPC host must be nonblank");
    const spaceId = options.spaceId ?? storeSpaceId();
    if (spaceId.length === 0) throw new Error("FRAMRPC SpaceId must be nonblank");
    return new FramRpcClient(
      host,
      positiveInteger("port", options.port ?? coordPort()),
      spaceId,
      positiveInteger("connect-timeout-ms", options.connectTimeoutMs ?? 2000),
      positiveInteger("read-timeout-ms", options.readTimeoutMs ?? 15000),
      positiveInteger("max-attempts", options.maxAttempts ?? 3),
      nonNegativeInteger("retry-delay-ms", options.retryDelayMs ?? 10),
      nonNegativeInteger("jitter-ms", options.jitterMs ?? 25),
      options.transport ?? socketRoundTrip,
    );
  }

  /** Build a client and prove the server serves THIS SpaceId before any write. */
  static async connect(options: FramRpcClientOptions = {}): Promise<FramRpcClient> {
    const client = FramRpcClient.create(options);
    try {
      await client.status();
      return client;
    } catch (error) {
      client.close();
      throw error;
    }
  }

  close(): void { this.closedFlag = true; }

  get closed(): boolean { return this.closedFlag; }

  private async retryPause(attempt: number): Promise<void> {
    const jitter = this.jitterMs > 0 ? Math.floor(Math.random() * (this.jitterMs + 1)) : 0;
    await delay(attempt * this.retryDelayMs + jitter);
  }

  /** Send one request, applying the reference client's retry law: a read may be
   * re-asked, a mutation may be re-asked ONLY when its bytes provably never
   * left this process. */
  async request(
    op: Keyword, payload: Term, options: RequestOptions = {},
  ): Promise<{ response: RpcResponse; attempts: number }> {
    if (this.closedFlag) throw new Error("FRAMRPC client is closed");
    const request: RpcRequest = {
      space: this.spaceId,
      op,
      expectedVersion: options.expectedVersion ?? null,
      page: options.page ?? null,
      timeoutMs: options.timeoutMs ?? null,
      payload,
    };
    const mutation = MUTATION_OPS.has(op.name);
    for (let attempt = 1; ; attempt += 1) {
      let response: RpcResponse;
      try {
        response = await this.transport({
          host: this.host,
          port: this.port,
          requestId: nextRequestId(),
          request,
          connectTimeoutMs: this.connectTimeoutMs,
          readTimeoutMs: Math.max(
            this.readTimeoutMs, 1000 + (request.timeoutMs ?? 0),
          ),
        });
      } catch (error) {
        const transportError = error instanceof FramRpcTransportError
          ? new FramRpcTransportError(
            error.code, error.message, error.requestSent, op.name, attempt,
            { cause: error.cause },
          )
          : error;
        if (!(transportError instanceof FramRpcTransportError)) throw transportError;
        const resendable = !mutation || !transportError.requestSent;
        if (resendable && attempt < this.maxAttempts) {
          await this.retryPause(attempt);
          continue;
        }
        throw transportError;
      }
      const error = response.error;
      if (error === null) return { response, attempts: attempt };
      const code = error.code.name;
      const retryable = error.retryable && RETRYABLE_ERROR_CODES.has(code);
      if (retryable && !NEVER_AUTO_RETRIED.has(code) && attempt < this.maxAttempts) {
        await this.retryPause(attempt);
        continue;
      }
      throw new FramRpcServerError(
        code, error.message, error.retryable, response.servedVersion,
        error.detail, op.name, attempt,
      );
    }
  }

  async version(): Promise<ServedResult> {
    const { response, attempts } = await this.request(kw("rpc/version"), RPC_UNIT);
    return { servedVersion: response.servedVersion, attempts };
  }

  async status(): Promise<RpcStatus & ServedResult & { spaceId: string }> {
    const { response, attempts } = await this.request(kw("rpc/status"), RPC_UNIT);
    return {
      ...decodeStatus(response.payload),
      spaceId: this.spaceId,
      servedVersion: response.servedVersion,
      attempts,
    };
  }

  async scan(
    t1: Term | null, t2: Term | null, t3: Term | null,
    options: RequestOptions = {},
  ): Promise<ServedResult & { rows: Term[]; page: RpcPageResponse | null }> {
    const { response, attempts } = await this.request(
      kw("rpc/scan"), rpcTriplePattern(t1, t2, t3), options,
    );
    return {
      rows: decodeTriples(response.payload),
      page: response.page,
      servedVersion: response.servedVersion,
      attempts,
    };
  }

  /** Drain every page of one rpc/scan. A served-version change mid-drain means
   * the pages do not describe one snapshot, so it fails instead of returning a
   * torn read. */
  async scanAll(
    t1: Term | null, t2: Term | null, t3: Term | null,
    options: { pageSize?: number } = {},
  ): Promise<ServedResult & { rows: Term[]; pages: number }> {
    const pageSize = positiveInteger("page-size", options.pageSize ?? EFFECTIVE_PAGE_LIMIT);
    if (pageSize > EFFECTIVE_PAGE_LIMIT)
      throw new Error("FRAMRPC page size exceeds the current TermCodec-safe limit");
    let cursor: Term | null = null;
    let rows: Term[] = [];
    let pages = 0;
    let snapshot: number | null = null;
    let attempts = 0;
    for (;;) {
      const page = await this.scan(t1, t2, t3, {
        page: { limit: pageSize, cursor },
      });
      if (page.page === null)
        throw new Error("FRAMRPC paged operation omitted page metadata");
      if (snapshot === null) snapshot = page.servedVersion;
      if (snapshot !== page.servedVersion)
        throw new Error(
          `FRAMRPC page drain changed snapshot: ${snapshot} -> ${page.servedVersion}`,
        );
      rows = rows.concat(page.rows);
      pages += 1;
      attempts += page.attempts;
      if (page.page.done)
        return { rows, servedVersion: snapshot, pages, attempts };
      cursor = page.page.cursor;
    }
  }

  /** One fenced, version-checked transaction: every action commits or none does. */
  async batch(
    actions: readonly BatchAction[],
    options: RequestOptions & { fence?: Term | null } = {},
  ): Promise<ServedResult & { results: ActionResult[] }> {
    const { response, attempts } = await this.request(
      kw("rpc/batch"), rpcBatch(actions, options.fence ?? null), options,
    );
    return {
      results: decodeMutationResult(response.payload),
      servedVersion: response.servedVersion,
      attempts,
    };
  }

  async leaseAcquire(
    resource: Term, holder: Term, ttlMs: number, options: RequestOptions = {},
  ): Promise<ServedResult & LeaseGrant> {
    const { response, attempts } = await this.request(
      kw("rpc/lease-acquire"), rpcLeaseAcquire(resource, holder, ttlMs), options,
    );
    return {
      ...decodeLeaseGrant(response.payload),
      servedVersion: response.servedVersion,
      attempts,
    };
  }

  async leaseRenew(
    fence: Term, ttlMs: number, options: RequestOptions = {},
  ): Promise<ServedResult & LeaseGrant> {
    const { response, attempts } = await this.request(
      kw("rpc/lease-renew"), rpcLeaseRenew(fence, ttlMs), options,
    );
    return {
      ...decodeLeaseGrant(response.payload),
      servedVersion: response.servedVersion,
      attempts,
    };
  }

  async leaseRelease(
    fence: Term, options: RequestOptions = {},
  ): Promise<ServedResult & { released: boolean }> {
    const { response, attempts } = await this.request(
      kw("rpc/lease-release"), fence, options,
    );
    return {
      released: decodeLeaseReleased(response.payload),
      servedVersion: response.servedVersion,
      attempts,
    };
  }

  /** Diagnostic only: a mutation must still CARRY the fence to be fenced. */
  async leaseCheck(
    fence: Term, options: RequestOptions = {},
  ): Promise<ServedResult & LeaseCheck> {
    const { response, attempts } = await this.request(
      kw("rpc/lease-check"), fence, options,
    );
    return {
      ...decodeLeaseCheck(response.payload),
      servedVersion: response.servedVersion,
      attempts,
    };
  }
}
