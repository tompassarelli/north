// Local TypeScript boundary for the vendored JavaScript implementation.

export type EsoJsonValue =
  | null
  | boolean
  | number
  | string
  | EsoJsonValue[]
  | { [key: string]: EsoJsonValue };

export type EsoCellValue = EsoJsonValue | bigint;
export type EsoDocument = Record<string, EsoCellValue>;

export type TryDecodeResult<T extends Record<string, unknown>> =
  | { ok: true; value: T; error?: never }
  | { ok: false; error: unknown; value?: never };

export function isRecord(value: unknown): value is Record<string, unknown>;
export function encode(input: EsoDocument): string;
export function decode<T extends Record<string, unknown> = Record<string, unknown>>(source: string): T;
export function tryDecode<T extends Record<string, unknown> = Record<string, unknown>>(
  source: string,
): TryDecodeResult<T>;
