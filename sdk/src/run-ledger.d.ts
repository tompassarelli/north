export type WireEvent = import("./wire/events.js").WireEvent;

export interface WireEventProjection {
  subject: string;
  facts: Array<[string, string]>;
}

export interface WireEventStorePublisher {
  publish: (arg0: ReadonlyArray<WireEvent>) => Promise<void>;
}

export interface WireEventStorePublisherOptions {
  timeoutMs?: number;
  writer?: (arg0: Array<WireEventProjection>, arg1: number) => Promise<WireLedgerPublicationStatus>;
}

export type WireLedgerBatchWriter = (arg0: Array<WireEventProjection>, arg1: number) => Promise<WireLedgerPublicationStatus>;

export interface WireLedgerBounds {
  maxEventsPerRun: number;
  maxCanonicalEventBytes: number;
  maxBatchEvents: number;
  maxProjectionBatchBytes: number;
  maxTelemetryProjectionBytes: number;
}

export interface WireLedgerContract {
  version: string;
  wireVersion: string;
  digest: WireLedgerDigestContract;
  bounds: WireLedgerBounds;
  telemetry: WireLedgerTelemetryContract;
  predicates: Array<string>;
}

export interface WireLedgerDigestContract {
  algorithm: string;
  eventInput: string;
  ledgerInput: string;
}

export interface WireLedgerError {
  name: string;
  message: string;
  code: WireLedgerErrorCode;
  cause?: Record<string, unknown>;
}

export type WireLedgerErrorCode = "invalid_identity" | "invalid_event" | "invalid_batch" | "invalid_summary";

export interface WireLedgerEstimateRatio {
  scale: number;
  rounding: string;
  trailingFractionZeros: string;
}

export type WireLedgerPublicationStatus = "recorded" | "unavailable";

export interface WireLedgerTelemetryContract {
  estimateRatio: WireLedgerEstimateRatio;
}

export interface WireRunLedgerIdentity {
  thread: string;
  agent: string;
  parentThread?: string;
  coordinator?: string;
}

export interface WireRunLedgerSummary {
  version: string;
  wireVersion: string;
  runId: string;
  eventCount: number;
  firstSequence: number;
  lastSequence: number;
  terminalEventId: string;
  digest: string;
}

export declare const AGENT_RUN_LEDGER_CONTRACT: WireLedgerContract;

export declare const AGENT_RUN_LEDGER_VERSION: string;

export declare const WireLedgerError: {
  new(arg0: WireLedgerErrorCode, arg1: string, arg2?: Record<string, unknown>): WireLedgerError;
};

export declare function createWireEventStorePublisher(arg0: WireRunLedgerIdentity): WireEventStorePublisher;
export declare function createWireEventStorePublisher(arg0: WireRunLedgerIdentity, arg1: WireEventStorePublisherOptions): WireEventStorePublisher;

export declare function isWireRunLedgerSummary(arg0: Record<string, unknown>): boolean;

export declare function publishWireEvents(arg0: WireRunLedgerIdentity, arg1: Array<Record<string, unknown>>): Promise<WireLedgerPublicationStatus>;
export declare function publishWireEvents(arg0: WireRunLedgerIdentity, arg1: Array<Record<string, unknown>>, arg2: number): Promise<WireLedgerPublicationStatus>;
export declare function publishWireEvents(arg0: WireRunLedgerIdentity, arg1: Array<Record<string, unknown>>, arg2: number, arg3: (arg0: Array<WireEventProjection>, arg1: number) => Promise<WireLedgerPublicationStatus>): Promise<WireLedgerPublicationStatus>;

export declare function recordWireEventProjections(arg0: Array<WireEventProjection>): Promise<WireLedgerPublicationStatus>;
export declare function recordWireEventProjections(arg0: Array<WireEventProjection>, arg1: number): Promise<WireLedgerPublicationStatus>;
export declare function recordWireEventProjections(arg0: Array<WireEventProjection>, arg1: number, arg2: Record<string, unknown>): Promise<WireLedgerPublicationStatus>;

export declare function wireEventFacts(arg0: WireRunLedgerIdentity, arg1: Record<string, unknown>): WireEventProjection;

export declare function wireLedgerSummary(arg0: Array<Record<string, unknown>>): WireRunLedgerSummary;

export declare function wireRunLedgerIdentity(arg0: WireRunLedgerIdentity): WireRunLedgerIdentity;
