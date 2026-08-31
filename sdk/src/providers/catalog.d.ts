export interface CalibratedModelDeltaDescriptor {
  kind: "calibrated";
  path: string;
}

export type CapabilityFloor = "baseline" | "standard" | "advanced" | "frontier";

export type CatalogFileIdentity = Record<string, unknown>;

export type Effort = "low" | "medium" | "high" | "xhigh" | "max";

export type ModelDeltaDescriptor = CalibratedModelDeltaDescriptor | NoModelDeltaDescriptor;

export interface NoModelDeltaDescriptor {
  kind: "none";
  reason: string;
}

export interface ProviderCatalogFileCache<T> {
  load: (arg0: string, arg1: (arg0: string) => T) => T;
}

export interface ProviderCatalogFileReader {
  identity: (arg0: string) => CatalogFileIdentity;
  read: (arg0: string) => string;
}

export interface ProviderContextWindowObservation {
  provider: ProviderId;
  model: string;
  tokens: number;
  effectiveFrom: string;
  source: "orchestration-provider-catalog";
}

export type ProviderId = "anthropic" | "openai";

export interface ResolvedCalibratedModelDelta {
  provider: ProviderId;
  model: string;
  kind: "calibrated";
  path: string;
  absolutePath: string;
}

export type ResolvedModelDelta = ResolvedCalibratedModelDelta | ResolvedNoModelDelta;

export interface ResolvedNoModelDelta {
  provider: ProviderId;
  model: string;
  kind: "none";
  reason: string;
}

export interface ResolvedRoute {
  capabilityFloor: CapabilityFloor;
  serviceClass: ServiceClass;
  model?: string;
  effort?: Effort;
}

export type ServiceClass = "economy" | "fast" | "balanced" | "premium";

export declare const ProviderCatalogFileCache: {
  new<T>(arg0?: ProviderCatalogFileReader, arg1?: number): ProviderCatalogFileCache<T>;
};

export declare function canonicalWriteModel(arg0?: string, arg1?: string): string | undefined;

export declare function modelFamilies(arg0: string): Array<string>;

export declare function modelFamily(arg0: string, arg1?: string): string | undefined;

export declare function observeProviderContextWindow(arg0: string, arg1?: string): ProviderContextWindowObservation | undefined;

export declare function providerSupportsModel(arg0: string, arg1?: string): boolean;

export declare function providerSupportsRoute(arg0: string, arg1: string, arg2: string, arg3: string): boolean;
export declare function providerSupportsRoute(arg0: string, arg1: string, arg2: string, arg3: string, arg4?: string): boolean;

export declare function resolveModelAlias(arg0: string, arg1?: string): string | undefined;

export declare function resolveModelDelta(arg0: string, arg1: string): ResolvedModelDelta;

export declare function resolveRoute(arg0: string, arg1: string, arg2: string): ResolvedRoute;
export declare function resolveRoute(arg0: string, arg1: string, arg2: string, arg3?: string): ResolvedRoute;
export declare function resolveRoute(arg0: string, arg1: string, arg2: string, arg3?: string, arg4?: string): ResolvedRoute;

export declare function supportedReasoning(arg0: string, arg1: string): Array<Effort>;
