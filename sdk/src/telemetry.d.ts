export type Fact = Array<string>;

export type Facts = Array<Array<string>>;

export interface RecordedWireRunLedger {
  status: "recorded";
  summary: Record<string, unknown>;
}

export type RunPublicationStatus = "recorded" | "unavailable";

export interface WireRunTelemetryProjection {
  subject: string;
  facts: Array<[string, string]>;
}

export type WireRunTelemetryWriter = (arg0: WireRunTelemetryProjection, arg1: number) => Promise<RunPublicationStatus>;

export declare function applyTerminalCoordinatorReadTimeout(): null;
export declare function applyTerminalCoordinatorReadTimeout(arg0: Record<string, unknown>): null;

export declare function newRunId(arg0: string): string;

export declare function recordWireRunTelemetry(arg0: Record<string, unknown>, arg1: Record<string, unknown>, arg2: RecordedWireRunLedger, arg3: Record<string, unknown>): Promise<RunPublicationStatus>;
export declare function recordWireRunTelemetry(arg0: Record<string, unknown>, arg1: Record<string, unknown>, arg2: RecordedWireRunLedger, arg3: Record<string, unknown>, arg4: number): Promise<RunPublicationStatus>;
export declare function recordWireRunTelemetry(arg0: Record<string, unknown>, arg1: Record<string, unknown>, arg2: RecordedWireRunLedger, arg3: Record<string, unknown>, arg4: number, arg5: WireRunTelemetryWriter): Promise<RunPublicationStatus>;

export declare function recordWireRunTelemetryProjection(arg0: WireRunTelemetryProjection): Promise<RunPublicationStatus>;
export declare function recordWireRunTelemetryProjection(arg0: WireRunTelemetryProjection, arg1: number): Promise<RunPublicationStatus>;
export declare function recordWireRunTelemetryProjection(arg0: WireRunTelemetryProjection, arg1: number, arg2: Record<string, unknown>): Promise<RunPublicationStatus>;

export declare function wireRunTelemetryFacts(arg0: Record<string, unknown>, arg1: Record<string, unknown>, arg2: RecordedWireRunLedger, arg3: Record<string, unknown>): WireRunTelemetryProjection;
