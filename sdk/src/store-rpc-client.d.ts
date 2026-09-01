export type ActionResult = import("./store-rpc-codec.js").ActionResult;

export type BatchAction = import("./store-rpc-codec.js").BatchAction;

export interface BatchRequestOptions {
  expectedVersion?: number | null;
  page?: RpcPageRequest | null;
  timeoutMs?: number | null;
  fence?: Term | null;
}

export type Keyword = import("./coord-wire.js").Keyword;

export interface ReadonlyStringSet {
  has: (arg0: string) => boolean;
}

export interface RequestOptions {
  expectedVersion?: number | null;
  page?: RpcPageRequest | null;
  timeoutMs?: number | null;
}

export type RpcPageRequest = import("./store-rpc-codec.js").RpcPageRequest;

export type RpcPageResponse = import("./store-rpc-codec.js").RpcPageResponse;

export type RpcRequest = import("./store-rpc-codec.js").RpcRequest;

export type RpcResponse = import("./store-rpc-codec.js").RpcResponse;

export interface ScanAllOptions {
  pageSize?: number;
}

export interface ServedResult {
  servedVersion: number;
  attempts: number;
}

export type StoreInstant = import("./store-rpc-codec.js").StoreInstant;

export interface StoreRpcBatchResult {
  results: Array<ActionResult>;
  servedVersion: number;
  attempts: number;
}

export interface StoreRpcCacheStats {
  hits: number;
  misses: number;
  bytes: number;
  evictions: number;
}

export interface StoreRpcClient {
  host: string;
  port: number;
  spaceId: string;
  closed: boolean;
  close: () => void;
  request: (arg0: Keyword, arg1: Term, arg2?: RequestOptions) => Promise<StoreRpcRequestResult>;
  version: () => Promise<ServedResult>;
  status: () => Promise<StoreRpcStatusResult>;
  scan: (arg0: Term | null, arg1: Term | null, arg2: Term | null, arg3?: RequestOptions) => Promise<StoreRpcScanResult>;
  scanAll: (arg0: Term | null, arg1: Term | null, arg2: Term | null, arg3?: ScanAllOptions) => Promise<StoreRpcScanAllResult>;
  batch: (arg0: ReadonlyArray<BatchAction>, arg1?: BatchRequestOptions) => Promise<StoreRpcBatchResult>;
  leaseAcquire: (arg0: Term, arg1: Term, arg2: number, arg3?: RequestOptions) => Promise<StoreRpcLeaseGrantResult>;
  leaseRenew: (arg0: Term, arg1: number, arg2?: RequestOptions) => Promise<StoreRpcLeaseGrantResult>;
  leaseRelease: (arg0: Term, arg1?: RequestOptions) => Promise<StoreRpcLeaseReleaseResult>;
  leaseCheck: (arg0: Term, arg1?: RequestOptions) => Promise<StoreRpcLeaseCheckResult>;
}

export interface StoreRpcClientFactory {
  create: (arg0?: StoreRpcClientOptions) => StoreRpcClient;
  connect: (arg0?: StoreRpcClientOptions) => Promise<StoreRpcClient>;
}

export interface StoreRpcClientOptions {
  host?: string;
  port?: number;
  spaceId?: string;
  connectTimeoutMs?: number;
  readTimeoutMs?: number;
  maxAttempts?: number;
  retryDelayMs?: number;
  jitterMs?: number;
  transport?: StoreRpcTransport;
}

export interface StoreRpcLeaseCheckResult {
  valid: boolean;
  expires: StoreInstant | null;
  servedVersion: number;
  attempts: number;
}

export interface StoreRpcLeaseGrantResult {
  fence: StoreTriple;
  expires: StoreInstant;
  servedVersion: number;
  attempts: number;
}

export interface StoreRpcLeaseReleaseResult {
  released: boolean;
  servedVersion: number;
  attempts: number;
}

export interface StoreRpcRequestResult {
  response: RpcResponse;
  attempts: number;
}

export interface StoreRpcScanAllResult {
  rows: Array<Term>;
  servedVersion: number;
  pages: number;
  attempts: number;
}

export interface StoreRpcScanResult {
  rows: Array<Term>;
  page: RpcPageResponse | null;
  servedVersion: number;
  attempts: number;
}

export interface StoreRpcServerError {
  name: string;
  message: string;
  code: string;
  retryable: boolean;
  servedVersion: number;
  detail: Term | null;
  op: string;
  attempts: number;
}

export interface StoreRpcStatusResult {
  state: Keyword;
  liveCount: number;
  engine: Keyword;
  cache: StoreRpcCacheStats;
  spaceId: string;
  servedVersion: number;
  attempts: number;
}

export type StoreRpcTransport = (arg0: StoreRpcTransportInput) => Promise<RpcResponse>;

export interface StoreRpcTransportError {
  name: string;
  message: string;
  code: string;
  requestSent: boolean;
  op: string;
  attempts: number;
  cause?: Record<string, unknown>;
}

export interface StoreRpcTransportInput {
  host: string;
  port: number;
  requestId: number;
  request: RpcRequest;
  connectTimeoutMs: number;
  readTimeoutMs: number;
}

export type StoreTriple = import("./store-rpc-codec.js").StoreTriple;

export type Term = import("./store-rpc-codec.js").Term;

export declare const EFFECTIVE_PAGE_LIMIT: number;

export declare const RETRYABLE_ERROR_CODES: ReadonlyStringSet;

export declare const StoreRpcClient: StoreRpcClientFactory;

export declare const StoreRpcServerError: {
  new(arg0: string, arg1: string, arg2: boolean, arg3: number, arg4: Term | null, arg5: string, arg6: number): StoreRpcServerError;
};

export declare const StoreRpcTransportError: {
  new(arg0: string, arg1: string, arg2: boolean, arg3: string, arg4: number, arg5?: Record<string, unknown>): StoreRpcTransportError;
};

export declare function socketRoundTrip(arg0: StoreRpcTransportInput): Promise<RpcResponse>;

export declare function storeRpcRequestId(arg0: number, arg1: number): number;
