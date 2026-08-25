const [moduleName, javascriptPath, outputPath] = Bun.argv.slice(2);

if (moduleName === undefined || javascriptPath === undefined || outputPath === undefined) {
  throw new Error("usage: generate-bridge-declarations MODULE JS OUTPUT");
}

const appTypedDeclarations = new Map<string, string>([
  ["run-northbridge-app!", `declare function runNorthbridgeApp(options: {
  viewId?: string;
  sourceIdentity?: string;
}): Promise<unknown>;`],
  ["boot!", `declare function boot<T>(
  runtime: T,
  launch: (prompt: string, role: string) => Promise<unknown>,
): T;`],
  ["bridge-app-launch-argv!", `declare function bridgeAppLaunchArgv(
  runtime: unknown,
  prompt: string,
  role: string,
): string[];`],
  ["launch-thread-id", `declare function launchThreadId(
  runtime: unknown,
  role: string,
): string;`],
  ["handle-local-command!", `declare function handleLocalCommand(
  runtime: unknown,
  ui: unknown,
  input: string,
): boolean;`],
  ["palette-options", "declare function paletteOptions(frame: string, query: string): Array<{ name: string }>;"],
  ["parse-bridge-stream!", `declare function parseBridgeStream(
  runtime: unknown,
  streamState: unknown,
  chunk: string,
): unknown;`],
  ["launch-route-flags", `declare function launchRouteFlags(
  provider: unknown,
  tier: unknown,
  model: unknown,
  effort: unknown,
): string[];`],
  ["set-launch-route!", `declare function setLaunchRoute(
  runtime: Record<string, unknown>,
  name: string,
  value: string,
): unknown;`],
  ["take-launch-route-flags!", `declare function takeLaunchRouteFlags(
  runtime: Record<string, unknown>,
): string[];`],
  ["project-conversation", `declare function projectConversation(
  items: unknown[],
  executionId: string,
  aggregate: boolean,
): unknown[];`],
  ["suspend-runtime!", `declare function suspendRuntime(
  runtime: unknown,
  platform: string,
  processApi: unknown,
): boolean;`],
  ["cleanup-suspend!", `declare function cleanupSuspend(
  runtime: unknown,
  processApi: unknown,
): boolean;`],
]);

const protocolPrelude = `import type { WireEvent } from "../../../../wire/events";
import type { JournalRecord, TornTail } from "../../../journal";
import type { ReasoningLevel, RoutingTier } from "../../../../routing-metadata";

export interface BridgeHello {
  type: "hello";
  identity?: string;
  liveExecutions: number;
  pinningExecutions?: number;
  pid: number;
}

export type BridgeServerMessage =
  | BridgeHello
  | { type: "launched"; executionId: string }
  | { type: "controlled"; executionId: string; control: string; delivery: string }
  | { type: "event"; record: JournalRecord }
  | { type: "wire"; event: WireEvent }
  | { type: "barrier"; executionId: string; cursor: number; tornTail?: TornTail }
  | { type: "error"; message: string };

export type BridgeLaunchRole = "director" | "implementer";
export type BridgeLaunchProvider = "anthropic" | "openai";

export interface BridgeLaunchSelection {
  provider?: BridgeLaunchProvider;
  tier?: RoutingTier;
  model?: string;
  effort?: ReasoningLevel;
}

export type BridgeRequest =
  | { op: "launch"; prompt: string; cwd: string; role: BridgeLaunchRole;
      attemptId: string; executionId?: string; provider?: BridgeLaunchProvider;
      tier?: RoutingTier; model?: string; effort?: ReasoningLevel }
  | { op: "attach"; executionId: string; cursor: number }
  | { op: "submitInput"; executionId: string; input: string }
  | { op: "interruptTurn"; executionId: string }
  | { op: "redirectNow"; executionId: string; input: string }
  | { op: "terminateSession"; executionId: string }
  | { op: "retire" };
`;

const protocolTypedDeclarations = new Map<string, string>([
  ["bridge-source-identity", "declare function bridgeSourceIdentity(): string | undefined;"],
  ["pinning-executions", "declare function pinningExecutions(hello: BridgeHello): number;"],
  ["parse-bridge-launch-provider!", "declare function parseBridgeLaunchProvider(value: unknown): BridgeLaunchProvider | undefined;"],
  ["parse-bridge-launch-tier!", "declare function parseBridgeLaunchTier(value: unknown): RoutingTier | undefined;"],
  ["parse-bridge-launch-model!", "declare function parseBridgeLaunchModel(value: unknown): string | undefined;"],
  ["parse-bridge-launch-effort!", "declare function parseBridgeLaunchEffort(value: unknown): ReasoningLevel | undefined;"],
  ["parse-bridge-launch-role!", "declare function parseBridgeLaunchRole(value: unknown): BridgeLaunchRole;"],
  ["bridge-state-directory", "declare function bridgeStateDirectory(env?: NodeJS.ProcessEnv): string;"],
  ["bridge-socket-path", "declare function bridgeSocketPath(env?: NodeJS.ProcessEnv): string;"],
  ["bridge-journal-root", "declare function bridgeJournalRoot(env?: NodeJS.ProcessEnv): string;"],
  ["parse-bridge-request!", "declare function parseBridgeRequest(value: unknown): BridgeRequest;"],
  ["parse-bridge-launch-attempt-id!", "declare function parseBridgeLaunchAttemptId(value: unknown): string;"],
  ["parse-bridge-launch-execution-id!", "declare function parseBridgeLaunchExecutionId(value: unknown): string;"],
]);

const appLaunchPrelude = `import type {
  DeliveryAttemptLaunchIntent, DeliveryAttemptProviderStart, DeliveryReservation,
} from "../../../../delivery-evidence";
import type { Fact } from "../../../../north-client";
import type { BridgeCommandReceipts } from "../../../command-receipts";
import type { BridgeLaunchRole, BridgeLaunchSelection } from "./protocol.js";
import type { WireEvent } from "../../../../wire/events";

export interface BridgeAppLaunchRequest extends BridgeLaunchSelection {
  role: BridgeLaunchRole;
  prompt: string;
  cwd: string;
  selectedThreadId?: string;
}

export type BridgeAppLaunchUnsentReason =
  | "attempt-binding-refused" | "daemon-not-contacted" | "daemon-launch-refused";

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
  proveUnsent(reason: Exclude<BridgeAppLaunchUnsentReason, "attempt-binding-refused">): Promise<void>;
}

export interface BridgeAppLaunchDependencies {
  env?: NodeJS.ProcessEnv;
  loadThreadFacts?: (threadId: string) => readonly Fact[];
  selectProvider?: (...args: any[]) => Promise<any>;
  acquireLeases?: (...args: any[]) => Promise<any>;
  reserve?: (...args: any[]) => DeliveryReservation;
  launchIntent?: (...args: any[]) => DeliveryAttemptLaunchIntent;
  providerStart?: (...args: any[]) => DeliveryAttemptProviderStart;
  provedUnsent?: (...args: any[]) => unknown;
  terminal?: (...args: any[]) => unknown;
  commandReceipts?: BridgeCommandReceipts;
  executionId?: string;
  leaseRenewIntervalMs?: number;
}
`;

const appLaunchTypedDeclarations = new Map<string, string>([
  ["prepare-managed-bridge-app-launch!", `declare function prepareManagedBridgeAppLaunch(
  request: BridgeAppLaunchRequest,
  dependencies?: BridgeAppLaunchDependencies,
): Promise<ManagedBridgeAppLaunch>;`],
]);

const cliPrelude = `import type { Socket } from "node:net";
import type { WireEvent } from "../../../../wire/events";
import type { ManagedBridgeAppLaunch } from "./app-launch-reservation.js";
import type {
  BridgeHello, BridgeLaunchProvider, BridgeLaunchRole, BridgeLaunchSelection,
} from "./protocol.js";

export interface BridgeLaunchArguments extends BridgeLaunchSelection {
  role: BridgeLaunchRole;
  attemptId: string;
  promptArguments: string[];
}

export interface BridgeAppLaunchArguments extends BridgeLaunchSelection {
  role: BridgeLaunchRole;
  promptArguments: string[];
  selectedThreadId: string;
}

export interface BridgeConnection { socket: Socket; hello: BridgeHello | null }
export interface BridgeConnectionOutput {
  info(message: string): void;
  error(message: string): void;
}
export interface VerifiedSocketOptions { replacePinned?: boolean }
export interface BridgeClientOutcome {
  code: number; launched: boolean; refused: boolean; errors: string[]; cursor: number;
}
`;

const cliTypedDeclarations = new Map<string, string>([
  ["parse-bridge-launch-arguments!", "declare function parseBridgeLaunchArguments(args: string[]): BridgeLaunchArguments;"],
  ["parse-bridge-app-launch-arguments!", "declare function parseBridgeAppLaunchArguments(args: string[]): BridgeAppLaunchArguments;"],
  ["read-hello!", "declare function readHello(socket: Socket, timeoutMs: number): Promise<BridgeHello | null>;"],
  ["verified-socket!", `declare function verifiedSocket(
  path: string, output?: BridgeConnectionOutput, options?: VerifiedSocketOptions,
): Promise<BridgeConnection>;`],
  ["run-bridge-restart!", "declare function runBridgeRestart(path: string): Promise<number>;"],
  ["render-wire-event", "declare function renderWireEvent(event: WireEvent): string;"],
  ["bridge-app-launch-recovery-action", `declare function bridgeAppLaunchRecoveryAction(
  phase: "launch" | "attach",
  outcome: Pick<BridgeClientOutcome, "refused" | "errors">,
  state: Pick<ManagedBridgeAppLaunch, "providerEffectObserved" | "settled">,
): "complete" | "prove-unsent" | "reconnect";`],
  ["settle-managed-app-launch-refusal!", "declare function settleManagedAppLaunchRefusal(managed: ManagedBridgeAppLaunch): Promise<void>;"],
]);

const configurations = new Map<string, {
  prelude: string;
  declarations: Map<string, string>;
}>([
  ["model", { prelude: "", declarations: new Map() }],
  ["app", { prelude: "", declarations: appTypedDeclarations }],
  ["protocol", { prelude: protocolPrelude, declarations: protocolTypedDeclarations }],
  ["app-launch-reservation", { prelude: appLaunchPrelude, declarations: appLaunchTypedDeclarations }],
  ["cli", { prelude: cliPrelude, declarations: cliTypedDeclarations }],
]);

const configuration = configurations.get(moduleName);
if (!configuration) throw new Error(`unknown Bridge declaration module: ${moduleName}`);
const typedDeclarations = configuration.declarations;

function bindingName(alias: string): string {
  if (!/^[a-z][a-z0-9]*(?:-[a-z0-9]+)*[!?]?$/u.test(alias)) {
    throw new Error(`Bridge export cannot map to a TypeScript binding: ${alias}`);
  }
  const words = alias.replace(/[!?]$/u, "").split("-");
  return words[0]! + words.slice(1)
    .map((word) => word[0]!.toUpperCase() + word.slice(1))
    .join("");
}

const javascript = await Bun.file(javascriptPath).text();
const aliases = [...javascript.matchAll(
  /^export \{ [A-Za-z0-9_$]+ as "([^"]+)" \};$/gmu,
)].map((match) => match[1]!);
const emitted = new Set(aliases);
if (aliases.length === 0 || emitted.size !== aliases.length) {
  throw new Error("generated Bridge JavaScript has no exact unique export mapping");
}

const staleTyped = [...typedDeclarations.keys()].filter((alias) => !emitted.has(alias));
if (staleTyped.length > 0) {
  throw new Error(`typed Bridge declarations are stale: ${staleTyped.join(",")}`);
}

const genericDeclarations = aliases
  .filter((alias) => !typedDeclarations.has(alias))
  .map((alias) => `declare const ${bindingName(alias)}: (...args: any[]) => any;`);
const exportMappings = aliases
  .map((alias) => `  ${bindingName(alias)} as ${JSON.stringify(alias)},`);
const output = `${[
  configuration.prelude.trim(),
  [...typedDeclarations.values()].join("\n\n"),
  genericDeclarations.join("\n"),
  `export {\n${exportMappings.join("\n")}\n};`,
].filter(Boolean).join("\n\n")}\n`;

await Bun.write(outputPath, output);
