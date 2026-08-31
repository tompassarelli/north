export type ProviderId = "anthropic" | "openai";

export type ProviderJoinCoverage = "exact" | "partial" | "unknown";

export interface ProviderJoinInput {
  sessionId?: string;
  turnIds?: Array<string>;
  sessionPersistence: SessionPersistence;
}

export type SessionPersistence = "persisted" | "ephemeral" | "unknown";

export interface WireProviderJoinEvidence {
  version: "north-provider-join:v1";
  sessionKey?: string;
  turnKeys: ReadonlyArray<string>;
  sessionPersistence: SessionPersistence;
  coverage: ProviderJoinCoverage;
}

export declare const PROVIDER_JOIN_KEY_VERSION: string;

export declare function foldProviderJoinEvidence(arg0: Array<WireProviderJoinEvidence>): WireProviderJoinEvidence | undefined;

export declare function providerJoinEvidence(arg0: string, arg1: ProviderJoinInput): WireProviderJoinEvidence;

export declare function providerJoinEvidenceEqual(arg0: WireProviderJoinEvidence, arg1: WireProviderJoinEvidence): boolean;

export declare function providerSessionKey(arg0: string): string;

export declare function providerTurnKey(arg0: string, arg1: string): string;
