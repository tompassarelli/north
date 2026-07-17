// Local TypeScript boundary for the vendored JavaScript implementation.

export interface CcrOptions {
  minItems?: number;
  maxItems?: number;
  firstFraction?: number;
  lastFraction?: number;
}

export interface CcrSentinel {
  _ccr: string;
}

export interface CrushResult<T> {
  view: Array<T | CcrSentinel>;
  hash: string | null;
  dropped: number;
}

export const SENTINEL_KEY: "_ccr";
export const DEFAULTS: Readonly<Required<CcrOptions>>;

export function isSentinel(item: unknown): item is CcrSentinel;
export function hashOf(array: readonly unknown[]): string;
export function crush<T>(array: T[], options?: CcrOptions): CrushResult<T>;
export function strip<T>(view: Array<T | CcrSentinel>): T[];
export function strip<T>(view: T): T;
