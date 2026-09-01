export type AppServerRequestHandler = (arg0: string | number, arg1: string, arg2: Record<string, unknown> | Array<unknown> | string | number | boolean | null) => null | TypeScriptAnonymousObjectV32;

export interface AppServerRpc {
  nextId: Atom<number>;
  pending: Atom<Map<string | number, Pending>>;
  messages: Atom<StrictJsonlMessages>;
  terminal: Atom<null | Error>;
  terminalFromProcessDeath: Atom<boolean>;
  closed: Atom<boolean>;
  terminalListeners: Atom<Set<(arg0: Error) => null>>;
  unsupported: Atom<Map<string, number>>;
  stderr: Atom<ManagedProviderStderrRing>;
  inboundQueue: Atom<Array<string>>;
  inboundDraining: Atom<boolean>;
  inboundIdle: Atom<null | TypeScriptAnonymousObjectV13>;
  deferredInboundFailure: Atom<null | TypeScriptAnonymousObjectV5>;
  stdoutEnded: Atom<boolean>;
  child: Atom<Record<string, unknown>>;
  timeoutMs: Atom<number>;
  onNotification: Atom<(arg0: string, arg1: Record<string, unknown> | Array<unknown> | string | number | boolean | null) => null | Promise<null>>;
  onServerRequest: Atom<(arg0: string | number, arg1: string, arg2: Record<string, unknown> | Array<unknown> | string | number | boolean | null) => null | TypeScriptAnonymousObjectV32>;
  writeLine: Atom<(arg0: string, arg1: (arg0: null | Error) => null) => null>;
  ownsStderr: Atom<boolean>;
}

export type AppServerWriter = (arg0: string, arg1: (arg0: null | Error) => null) => null;

export type ChildProcessWithoutNullStreams = Record<string, unknown>;

export interface EffectiveMcpConfig {
  environment_id: string;
  tool_timeout_sec: null | number;
}

export interface ExpectedMcpServer {
  name: string;
  tools: Array<string>;
  version: null | string;
}

export type InvocationObservation = Record<string, unknown>;

export type JsonObject = Map<string, Record<string, unknown> | Array<unknown> | string | number | boolean | null>;

export interface LaunchContract {
  args: Array<string>;
  expectedSessionConfig: Map<string, Record<string, unknown> | Array<unknown> | string | number | boolean | null>;
  executable: string;
  codexHome: string;
  sqliteHome: string;
  cwd: string;
  projectRoot: string;
  writableRoots: Array<string>;
  network: ManagedCodexNetworkPolicy;
  installedManagedHookFailureMode: null | string;
}

export interface ManagedAppServerStdin {
  write: (arg0: string, arg1: (arg0: null | Error) => null) => boolean;
}

export interface ManagedBufferSlice {
  toString: (arg0: string) => string;
}

export interface ManagedCodexAppServerOptions {
  command: string;
  commandPrefix: null | Array<string>;
  useSupervisor: null | boolean;
  spawnProcess: null | ((arg0: string, arg1: Array<string>, arg2: Record<string, unknown>) => Record<string, unknown>);
  testExpectedExecutable: null | string;
  env: Record<string, unknown>;
  cwd: string;
  prompt: string;
  model: string;
  effort: null | string;
  developerInstructions: string;
  surface: Record<string, unknown>;
  north: ManagedCodexNorthServer;
  timeoutMs: null | number;
  turnDeadlineMs: null | number;
  turnDeadlineInactivityMs: null | number;
  inFlightItemCeilingMs: null | number;
  postToolQuietMs: null | number;
  maxRespawns: null | number;
  onActivity: null | ((arg0: string) => null);
  onEvent: null | ((arg0: string, arg1: Record<string, unknown> | Array<unknown> | string | number | boolean | null) => null | Promise<null>);
  onRespawn: null | (() => null | Promise<null>);
  beforeLaunch: null | (() => Promise<null>);
}

export interface ManagedCodexAppServerRun {
  mcpActivity: () => Record<string, unknown>;
  nativeCommandActivity: () => Record<string, unknown>;
  respawnRecord: () => ManagedCodexRespawnRecord;
  interrupt: () => Promise<void>;
  interruptTurn: () => Promise<void>;
  execute: () => Promise<ManagedCodexResult>;
  session: (arg0: () => Promise<null | string>) => AsyncIterable<ManagedCodexResult>;
}

export interface ManagedCodexAppServerRunState {
  child: Atom<null | Record<string, unknown>>;
  rpc: Atom<null | AppServerRpc>;
  control: Atom<null | SupervisorControl>;
  threadStarted: Atom<boolean>;
  mcp: McpActivityAccumulator;
  nativeCommands: Atom<null | NativeCommandActivityAccumulator>;
  respawns: Array<ManagedCodexRespawnAttempt>;
  "private-retainedPendingItemCount": Atom<number>;
  "private-retainedPendingItems": Atom<Array<ManagedCodexPendingItemSummary>>;
  laneCompletedTurns: Atom<number>;
  attemptDeath: Atom<null | TypeScriptAnonymousObjectV15>;
  attemptFailure: Atom<null | Error>;
  interrupted: Atom<boolean>;
  activeTurnInterrupt: Atom<null | (() => Promise<null>)>;
  "private-replacementTurnPending": Atom<boolean>;
  "private-pendingReplacementTurnInterrupt": Atom<null | PendingReplacementTurnInterrupt>;
  options: Atom<ManagedCodexAppServerOptions>;
}

export interface ManagedCodexDiagnostics {
  stderrTail: Array<string>;
  exitCode: null | number;
  exitSignal: null | string;
  providerAlive: null | boolean;
}

export interface ManagedCodexHarvest {
  threadId: null | string;
  turnIds: Array<string>;
  completedTurns: number;
  text: string;
  toolItems: null | number;
  pendingItemCount: null | number;
  pendingItems: null | Array<ManagedCodexPendingItemSummary>;
  usage: null | TypeScriptAnonymousObjectV1;
  invocationObservations: null | Array<ManagedCodexInvocationObservation>;
  mcp: Record<string, unknown>;
  nativeCommands: Record<string, unknown>;
  unsupportedNotifications: Map<string, number>;
  landedWork: boolean;
  stderrTail: null | Array<string>;
  exitCode: null | number;
  exitSignal: null | string;
  respawnCount: null | number;
  respawns: null | Array<ManagedCodexRespawnAttempt>;
  interrupt: null | ManagedCodexInterruptEvidence;
}

export interface ManagedCodexHarvestError {
  message: string;
  name: string;
  diagnostics?: ManagedCodexDiagnostics;
  harvest: ManagedCodexHarvest;
  cause?: Error | ManagedCodexPreThreadError | ManagedCodexHarvestError | Record<string, unknown> | Array<unknown> | string | number | boolean | null;
}

export interface ManagedCodexInterruptEvidence {
  reason: ManagedCodexInterruptReason;
  deadlineMs: number;
  inactivityThresholdMs: number;
  lastActivityAgeMs: number;
  openItemCount: number;
  openItem: null | TypeScriptAnonymousObjectV2;
  eventCount: number;
  eventCounts: Map<string, number>;
}

export type ManagedCodexInterruptReason = "turn_deadline" | "post_tool_silence" | "in_flight_item_ceiling";

export interface ManagedCodexInvocationObservation {
  count: number;
  schema: string;
  hook: string;
  operation: TypeScriptStringLiteralV8;
  classification: TypeScriptStringLiteralV6;
  decision: TypeScriptStringLiteralV7;
}

export interface ManagedCodexIteratorStepV1 {
  done: boolean | null;
  value: ManagedCodexResult | null;
}

export interface ManagedCodexNetworkPolicy {
  networkAccess: boolean;
  networkProxyEnabled: boolean;
  domains: Map<string, string>;
}

export type ManagedCodexNextInput = () => Promise<null | string>;

export interface ManagedCodexNorthServer {
  command: string;
  args: Array<string>;
  env: Map<string, string>;
}

export type ManagedCodexPendingItemKind = "commandExecution" | "mcpToolCall" | "fileChange" | "webSearch" | "todoList" | "unknown";

export interface ManagedCodexPendingItemSummary {
  kind: string;
  name: string;
  count: number;
}

export interface ManagedCodexPreThreadError {
  message: string;
  name: string;
  diagnostics?: ManagedCodexDiagnostics;
  cause?: Error | ManagedCodexPreThreadError | ManagedCodexHarvestError | Record<string, unknown> | Array<unknown> | string | number | boolean | null;
}

export interface ManagedCodexRespawnAttempt {
  attempt: number;
  reason: string;
  threadId: null | string;
  completedTurns: number;
  stderrTail: null | Array<string>;
  exitCode: null | number;
  exitSignal: null | string;
}

export interface ManagedCodexRespawnRecord {
  respawnCount: number;
  completedTurns: number;
  respawns: Array<ManagedCodexRespawnAttempt>;
}

export interface ManagedCodexResult {
  text: string;
  usage: TypeScriptAnonymousObjectV1;
  providerDurationMs: number;
  providerJoin: Record<string, unknown>;
  toolItems: number;
  invocationObservations: null | Array<ManagedCodexInvocationObservation>;
}

export type ManagedCommandHook = Record<string, unknown>;

export interface ManagedEventEmitter {
  on: (arg0: string, arg1: (() => null) | ((arg0: Buffer<ArrayBuffer | SharedArrayBuffer>) => null) | ((arg0: string | Buffer<ArrayBuffer | SharedArrayBuffer>) => null)) => ManagedEventEmitter;
  once: (arg0: string, arg1: (() => null) | ((arg0: Buffer<ArrayBuffer | SharedArrayBuffer>) => null) | ((arg0: string | Buffer<ArrayBuffer | SharedArrayBuffer>) => null)) => ManagedEventEmitter;
}

export type ManagedEventListener = (() => null) | ((arg0: Buffer<ArrayBuffer | SharedArrayBuffer>) => null) | ((arg0: string | Buffer<ArrayBuffer | SharedArrayBuffer>) => null);

export type ManagedFailureReason = Error | ManagedCodexPreThreadError | ManagedCodexHarvestError | Record<string, unknown> | Array<unknown> | string | number | boolean | null;

export interface ManagedHookRow {
  eventName: Record<string, unknown> | Array<unknown> | string | number | boolean | null;
  handlerType: Record<string, unknown> | Array<unknown> | string | number | boolean | null;
  matcher: Record<string, unknown> | Array<unknown> | string | number | boolean | null;
  command: Record<string, unknown> | Array<unknown> | string | number | boolean | null;
  timeoutSec: Record<string, unknown> | Array<unknown> | string | number | boolean | null;
  sourcePath: Record<string, unknown> | Array<unknown> | string | number | boolean | null;
  source: Record<string, unknown> | Array<unknown> | string | number | boolean | null;
  enabled: Record<string, unknown> | Array<unknown> | string | number | boolean | null;
  isManaged: Record<string, unknown> | Array<unknown> | string | number | boolean | null;
  trustStatus: Record<string, unknown> | Array<unknown> | string | number | boolean | null;
}

export type ManagedMatcher = Record<string, unknown>;

export interface ManagedProviderStderrRing {
  push: (arg0: string) => Array<string>;
  finish: () => Array<string>;
  add: (arg0: string) => string;
  tail: (arg0: number) => Array<string>;
}

export interface ManagedStrictJsonlMessages {
  push: (arg0: Buffer<ArrayBuffer | SharedArrayBuffer>) => Array<string>;
}

export interface ManagedSupervisorStatusStream {
  on: (arg0: string, arg1: (() => null) | ((arg0: Buffer<ArrayBuffer | SharedArrayBuffer>) => null) | ((arg0: string | Buffer<ArrayBuffer | SharedArrayBuffer>) => null)) => ManagedSupervisorStatusStream;
  removeListener: (arg0: string, arg1: (() => null) | ((arg0: Buffer<ArrayBuffer | SharedArrayBuffer>) => null) | ((arg0: string | Buffer<ArrayBuffer | SharedArrayBuffer>) => null)) => ManagedSupervisorStatusStream;
  resume: () => ManagedSupervisorStatusStream;
}

export interface ManagedTimer {
  unref: () => ManagedTimer;
}

export interface ManagedUtf8Buffer {
  subarray: (arg0: number, arg1: number) => ManagedBufferSlice;
}

export type McpActivityObservation = Record<string, unknown>;

export type NativeCommandActivityObservation = Record<string, unknown>;

export type NativeCommandCompletion = Record<string, unknown>;

export type NativeCommandStatus = string;

export type OpenAIAuthoritySurface = Record<string, unknown>;

export type OpenAIWireSemanticToolKind = string;

export type OpenAIWireToolIdentity = Record<string, unknown>;

export interface Pending {
  method: string;
  timer: number;
  resolve: (arg0: Record<string, unknown> | Array<unknown> | string | number | boolean | null) => null;
  reject: (arg0: Error) => null;
}

export interface PendingReplacementTurnInterrupt {
  settlement: TypeScriptAnonymousObjectV13;
  dispatched: boolean;
}

export type ProviderStderrRing = ManagedProviderStderrRing;

export type RpcId = string | number;

export interface RuntimeNotificationState {
  threadId: string;
  cwd: string;
  model: string;
  turnId: null | string;
  hookRuns: Set<string>;
  text: string;
  usage: null | TypeScriptAnonymousObjectV1;
  providerDurationMs: null | number;
  terminalSeen: boolean;
  toolItems: number;
  invocationObservations: Map<string, ManagedCodexInvocationObservation>;
  openItems: Map<string, TypeScriptAnonymousObjectV8>;
  mcpActivity: McpActivityAccumulator;
  nativeCommands: NativeCommandActivityAccumulator;
  mcpServerNames: Array<string>;
}

export interface SupervisorControl {
  path: string;
  connected: Promise<null>;
  writeLine: (arg0: string, arg1: (arg0: null | Error) => null) => null;
  close: () => null;
}

export interface SupervisorRemovalOptions {
  recursive: boolean;
  force: boolean;
}

export interface SupervisorStatusChannel {
  failure: Promise<Never>;
  settled: () => null;
  stderrTail: (() => Array<string>) | ((arg0: number) => Array<string>);
  exitCode: () => null | number;
  close: () => null;
}

export interface TypeScriptAnonymousObjectV1 {
  cached_input_tokens: number;
  input_tokens: number;
  output_tokens: number;
  reasoning_output_tokens: number;
}

export interface TypeScriptAnonymousObjectV10 {
  pendingItemCount: number;
  pendingItems: Array<ManagedCodexPendingItemSummary>;
}

export interface TypeScriptAnonymousObjectV11 {
  promise: Promise<Never>;
  reject: (arg0: Error | ManagedCodexPreThreadError | ManagedCodexHarvestError | Record<string, unknown> | Array<unknown> | string | number | boolean | null) => null;
  resolve: (arg0: Never) => null;
}

export interface TypeScriptAnonymousObjectV12 {
  promise: Promise<boolean>;
  reject: (arg0: Error | ManagedCodexPreThreadError | ManagedCodexHarvestError | Record<string, unknown> | Array<unknown> | string | number | boolean | null) => null;
  resolve: (arg0: boolean) => null;
}

export interface TypeScriptAnonymousObjectV13 {
  promise: Promise<null>;
  reject: (arg0: Error | ManagedCodexPreThreadError | ManagedCodexHarvestError | Record<string, unknown> | Array<unknown> | string | number | boolean | null) => null;
  resolve: () => null;
}

export interface TypeScriptAnonymousObjectV14 {
  alive: boolean;
  exitCode: null | number;
  exitSignal: null | string;
}

export interface TypeScriptAnonymousObjectV15 {
  diagnostics: ManagedCodexDiagnostics;
  reason: string;
}

export interface TypeScriptAnonymousObjectV18 {
  pendingItemCount: number;
  pendingItems: Array<ManagedCodexPendingItemSummary>;
}

export interface TypeScriptAnonymousObjectV2 {
  ageMs: number;
  id: string;
  kind: string;
}

export interface TypeScriptAnonymousObjectV22 {
  promise: Promise<null>;
  reject: (() => null) | ((arg0: Error | ManagedCodexPreThreadError | ManagedCodexHarvestError | Record<string, unknown> | Array<unknown> | string | number | boolean | null) => null);
  resolve: () => null;
}

export interface TypeScriptAnonymousObjectV24 {
  exitCode: number;
}

export interface TypeScriptAnonymousObjectV25 {
  exitSignal: string;
}

export interface TypeScriptAnonymousObjectV28 {
  threadId: string;
}

export interface TypeScriptAnonymousObjectV29 {
  stderrTail: Array<string>;
}

export interface TypeScriptAnonymousObjectV3 {
  cause: Record<string, unknown> | Array<unknown> | string | number | boolean | null;
}

export interface TypeScriptAnonymousObjectV30 {
  method: string;
  value: Record<string, unknown> | Array<unknown> | string | number | boolean | null;
}

export interface TypeScriptAnonymousObjectV31 {
  id: string;
  kind: string;
  observedAtMs: number;
}

export interface TypeScriptAnonymousObjectV32 {
  answers: Map<string, TypeScriptAnonymousObjectV33>;
}

export interface TypeScriptAnonymousObjectV33 {
  answers: Array<string>;
}

export interface TypeScriptAnonymousObjectV34 {
  toolItems: number;
}

export interface TypeScriptAnonymousObjectV35 {
  invocationObservations: Array<ManagedCodexInvocationObservation>;
}

export interface TypeScriptAnonymousObjectV36 {
  interrupt: ManagedCodexInterruptEvidence;
}

export interface TypeScriptAnonymousObjectV37 {
  providerAlive: boolean;
}

export interface TypeScriptAnonymousObjectV4 {
  domains: Map<string, string>;
  networkAccess: boolean;
  networkProxyEnabled: boolean;
}

export interface TypeScriptAnonymousObjectV5 {
  error: Error;
  processDeath: boolean;
}

export interface TypeScriptAnonymousObjectV6 {
  promise: Promise<Record<string, unknown> | Array<unknown> | string | number | boolean | null>;
  reject: (arg0: Error | ManagedCodexPreThreadError | ManagedCodexHarvestError | Record<string, unknown> | Array<unknown> | string | number | boolean | null) => null;
  resolve: (arg0: Record<string, unknown> | Array<unknown> | string | number | boolean | null) => null;
}

export type TypeScriptAnonymousObjectV7 = Record<string, unknown>;

export interface TypeScriptAnonymousObjectV8 {
  kind: string;
  observedAtMs: number;
  pending: null | Record<string, unknown> | TypeScriptAnonymousObjectV9;
  startedAtMs: number;
}

export interface TypeScriptAnonymousObjectV9 {
  kind: string;
  name: string;
}

export type TypeScriptObjectV1 = Record<string, unknown>;

export interface TypeScriptProcessV1 {
  stdout: TypeScriptStdoutV1;
  env: Record<string, unknown>;
  pid: number;
  platform: string;
  execPath: string;
  getBuiltinModule: (arg0: string) => Record<string, unknown>;
}

export interface TypeScriptStdoutV1 {
  isTTY: boolean | null;
}

export type TypeScriptStringLiteralV1 = string;

export type TypeScriptStringLiteralV11 = string;

export type TypeScriptStringLiteralV2 = string;

export type TypeScriptStringLiteralV3 = string;

export type TypeScriptStringLiteralV5 = string;

export type TypeScriptStringLiteralV6 = string;

export type TypeScriptStringLiteralV7 = string;

export type TypeScriptStringLiteralV8 = string;

export type TypeScriptStringLiteralV9 = string;

export interface TypeScriptStructuralObjectV1 {
  configurable: null | boolean;
  enumerable: null | boolean;
  get: null | (() => Record<string, unknown> | Array<unknown> | string | number | boolean | null);
  set: null | ((arg0: Record<string, unknown> | Array<unknown> | string | number | boolean | null) => null);
  value: Record<string, unknown> | Array<unknown> | string | number | boolean | null | null;
  writable: null | boolean;
}

export interface TypeScriptStructuralObjectV11 {
  completedTurns: number;
  exitCode: null | number;
  exitSignal: null | string;
  interrupt: null | ManagedCodexInterruptEvidence;
  invocationObservations: null | Array<ManagedCodexInvocationObservation>;
  pendingItemCount: null | number;
  pendingItems: null | Array<ManagedCodexPendingItemSummary>;
  respawnCount: null | number;
  respawns: null | Array<ManagedCodexRespawnAttempt>;
  stderrTail: null | Array<string>;
  text: string;
  threadId: null | string;
  toolItems: null | number;
  turnIds: Array<string>;
  unsupportedNotifications: Map<string, number>;
  usage: null | TypeScriptAnonymousObjectV1;
}

export interface TypeScriptStructuralObjectV2 {
  cause: Error | ManagedCodexPreThreadError | ManagedCodexHarvestError | Record<string, unknown> | Array<unknown> | string | number | boolean | null;
}

export interface TypeScriptStructuralObjectV3 {
  disabledReason: null | Record<string, unknown>;
  name: Record<string, unknown> | Array<unknown> | string | number | boolean | null;
  version: Record<string, unknown> | Array<unknown> | string | number | boolean | null;
  config: Record<string, unknown> | Array<unknown> | string | number | boolean | null;
}

export type TypeScriptUnknownV1 = Record<string, unknown> | Array<unknown> | string | number | boolean | null;

export type WireProviderJoinEvidence = Record<string, unknown>;

export declare const MANAGED_CODEX_DISABLED_FEATURES: [TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2, TypeScriptStringLiteralV2];

export declare const MANAGED_CODEX_ENABLED_FEATURES: [TypeScriptStringLiteralV1, TypeScriptStringLiteralV1, TypeScriptStringLiteralV1];

export declare const MANAGED_CODEX_VERSION: string;

export declare const ManagedCodexAppServerRun: {
  new(arg0: ManagedCodexAppServerOptions): ManagedCodexAppServerRun;
};

export declare const ManagedCodexHarvestError: {
  new(arg0: ManagedCodexHarvest, arg1?: TypeScriptStructuralObjectV2): ManagedCodexHarvestError;
};

export declare const ManagedCodexPreThreadError: {
  new(arg0: string, arg1?: TypeScriptStructuralObjectV2): ManagedCodexPreThreadError;
};

export declare function managedCodexAppServerLaunch(arg0: ManagedCodexAppServerOptions): LaunchContract;

export declare function managedCodexRecoveredContext(arg0: string, arg1: Array<string>, arg2: ManagedCodexHarvest): string;

export declare function managedCodexWritableRoots(arg0: string): Array<string>;
