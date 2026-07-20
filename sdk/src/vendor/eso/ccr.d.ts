// Local TypeScript boundary for the vendored JavaScript implementation.

export interface CcrOptions {
  minItems?: number;
  maxItems?: number;
  firstFraction?: number;
  lastFraction?: number;
  /** Counts supplied arrays exactly as they would be sent, including CCR marker. */
  tokenCount?: (value: readonly unknown[]) => number;
  /** Both methods are required before CCR may return a lossy view. */
  cache?: CcrCache;
}

export interface CcrCache {
  put(hash: string, original: unknown[]): void;
  get(hash: string): unknown[] | undefined;
}

export interface CcrSentinel {
  _ccr: string;
}

export interface CrushResult<T> {
  view: Array<T | CcrSentinel>;
  hash: string | null;
  dropped: number;
}

export interface CacheObservation {
  /** Caller-supplied stable-prefix identity; evaluator never rewrites requests. */
  prefix: unknown;
  cacheReadTokens?: number;
  cachedInputTokens?: number;
}

export interface CacheStability {
  stablePrefix: boolean;
  cacheTokenDrift: number | undefined;
  sampleCount: number;
}

export const SENTINEL_KEY: "_ccr";
export const DEFAULTS: Readonly<Required<CcrOptions>>;

export function isSentinel(item: unknown): item is CcrSentinel;
export function hashOf(array: readonly unknown[]): string;
export function crush<T>(array: T[], options?: CcrOptions): CrushResult<T>;
export function replay<T>(result: CrushResult<T>, cache?: CcrCache): T[];
export function strip<T>(view: Array<T | CcrSentinel>): T[];
export function strip<T>(view: T): T;
export function evaluateCacheStability(observations: readonly CacheObservation[]): CacheStability;
