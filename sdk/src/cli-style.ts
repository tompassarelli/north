export type ColorEnvironment = Readonly<Record<string, string | undefined>>;

export interface TerminalStyleOptions {
  isTTY?: boolean;
  env?: ColorEnvironment;
}

export interface FormatAmountOptions {
  exact?: boolean;
}

export interface CliStyle {
  readonly enabled: boolean;
  ok(value: string): string;
  warn(value: string): string;
  crit(value: string): string;
  accent(value: string): string;
  dim(value: string): string;
  section(value: string): string;
  pairs(entries: ReadonlyArray<readonly [string, string]>, indent?: string): string;
  table(headers: readonly string[], rows: ReadonlyArray<readonly string[]>, indent?: string): string;
}

const ANSI = {
  reset: "\u001b[0m",
  ok: "\u001b[32m",
  warn: "\u001b[33m",
  crit: "\u001b[31m",
  accent: "\u001b[36m",
  dim: "\u001b[2m",
} as const;

export function terminalStyleEnabled({
  isTTY = Boolean(process.stdout.isTTY),
  env = process.env,
}: TerminalStyleOptions = {}): boolean {
  if (Object.prototype.hasOwnProperty.call(env, "FORCE_COLOR")) {
    const forced = env.FORCE_COLOR?.toLowerCase();
    return forced !== "0" && forced !== "false";
  }
  if (Object.prototype.hasOwnProperty.call(env, "NO_COLOR")) return false;
  return isTTY;
}

function paint(enabled: boolean, code: string, value: string): string {
  return enabled ? `${code}${value}${ANSI.reset}` : value;
}

export function alignedPairs(
  entries: ReadonlyArray<readonly [string, string]>,
  indent = "",
): string {
  if (!entries.length) return "";
  const width = Math.max(...entries.map(([key]) => key.length));
  return entries.map(([key, value]) => `${indent}${`${key}:`.padEnd(width + 1)} ${value}`).join("\n");
}

export function simpleTable(
  headers: readonly string[],
  rows: ReadonlyArray<readonly string[]>,
  indent = "",
): string {
  const allRows = headers.length ? [headers, ...rows] : rows;
  if (!allRows.length) return "";
  const columns = Math.max(...allRows.map((row) => row.length));
  const widths = Array.from({ length: columns }, (_, column) =>
    Math.max(...allRows.map((row) => row[column]?.length ?? 0)));
  const render = (row: readonly string[]) => `${indent}${Array.from({ length: columns }, (_, column) => {
    const value = row[column] ?? "";
    return column === columns - 1 ? value : value.padEnd(widths[column]!);
  }).join("  ").trimEnd()}`;
  return allRows.map(render).join("\n");
}

export function percentageGauge(percent: number): string {
  const bounded = Math.min(100, Math.max(0, percent));
  const filled = Math.round(bounded / 10);
  return `${"▓".repeat(filled)}${"░".repeat(10 - filled)} ${bounded}%`;
}

export function formatHumanAmount(value: number, { exact = false }: FormatAmountOptions = {}): string {
  if (exact || Math.abs(value) < 1_000) return value.toLocaleString("en-US");
  const units = ["K", "M", "B", "T"] as const;
  let scaled = Math.abs(value);
  let unit: (typeof units)[number] = units[0];
  for (const candidate of units) {
    scaled /= 1_000;
    unit = candidate;
    if (scaled < 1_000 || candidate === units.at(-1)) break;
  }
  const digits = scaled >= 100 ? 0 : scaled >= 10 ? 1 : 2;
  const compact = scaled.toFixed(digits).replace(/\.0+$|(?<=\.[0-9])0+$/, "");
  return `${value < 0 ? "-" : ""}${compact}${unit}`;
}

export const formatBytes = formatHumanAmount;
export const formatTokens = formatHumanAmount;

export function createCliStyle(options: TerminalStyleOptions = {}): CliStyle {
  const enabled = terminalStyleEnabled(options);
  return {
    enabled,
    ok: (value) => paint(enabled, ANSI.ok, value),
    warn: (value) => paint(enabled, ANSI.warn, value),
    crit: (value) => paint(enabled, ANSI.crit, value),
    accent: (value) => paint(enabled, ANSI.accent, value),
    dim: (value) => paint(enabled, ANSI.dim, value),
    section: (value) => paint(enabled, ANSI.accent, value),
    pairs: alignedPairs,
    table: simpleTable,
  };
}
