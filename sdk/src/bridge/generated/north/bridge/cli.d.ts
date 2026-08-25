import type { Socket } from "node:net";
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

declare function parseBridgeLaunchArguments(args: string[]): BridgeLaunchArguments;

declare function parseBridgeAppLaunchArguments(args: string[]): BridgeAppLaunchArguments;

declare function readHello(socket: Socket, timeoutMs: number): Promise<BridgeHello | null>;

declare function verifiedSocket(
  path: string, output?: BridgeConnectionOutput, options?: VerifiedSocketOptions,
): Promise<BridgeConnection>;

declare function runBridgeRestart(path: string): Promise<number>;

declare function renderWireEvent(event: WireEvent): string;

declare function bridgeAppLaunchRecoveryAction(
  phase: "launch" | "attach",
  outcome: Pick<BridgeClientOutcome, "refused" | "errors">,
  state: Pick<ManagedBridgeAppLaunch, "providerEffectObserved" | "settled">,
): "complete" | "prove-unsent" | "reconnect";

declare function settleManagedAppLaunchRefusal(managed: ManagedBridgeAppLaunch): Promise<void>;

export {
  bridgeAppLaunchRecoveryAction as "bridge-app-launch-recovery-action",
  parseBridgeAppLaunchArguments as "parse-bridge-app-launch-arguments!",
  parseBridgeLaunchArguments as "parse-bridge-launch-arguments!",
  readHello as "read-hello!",
  renderWireEvent as "render-wire-event",
  runBridgeRestart as "run-bridge-restart!",
  settleManagedAppLaunchRefusal as "settle-managed-app-launch-refusal!",
  verifiedSocket as "verified-socket!",
};
