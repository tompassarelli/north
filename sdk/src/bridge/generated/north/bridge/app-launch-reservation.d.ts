import type {
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

declare function prepareManagedBridgeAppLaunch(
  request: BridgeAppLaunchRequest,
  dependencies?: BridgeAppLaunchDependencies,
): Promise<ManagedBridgeAppLaunch>;

export {
  prepareManagedBridgeAppLaunch as "prepare-managed-bridge-app-launch!",
};
