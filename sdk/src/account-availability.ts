import {
  DEFAULT_PROVIDER_OBSERVATIONS_PATH,
  loadProviderUsageObservations,
} from "./resource-policy";
import type {
  ProviderId,
  ProviderUsageObservation,
  ProviderUsageObservationStore,
  ProviderUsageWindow,
} from "./providers/types";

const FIVE_HOURS_MS = 5 * 60 * 60 * 1_000;
const SEVEN_DAYS_MS = 7 * 24 * 60 * 60 * 1_000;

export interface AccountAvailabilityThresholds {
  warn: number;
  cooked: number;
}

export const DEFAULT_ACCOUNT_AVAILABILITY_THRESHOLDS: Readonly<AccountAvailabilityThresholds> =
  Object.freeze({ warn: 95, cooked: 98 });

export interface AccountAvailabilityWindowRung {
  name: string;
  pct: number;
  resetsAt: string;
}

export interface AccountAvailabilityRung {
  pct: number;
  resetsAt: string;
}

export interface AccountAvailabilityRow {
  account: string;
  provider: ProviderId;
  observedAt: string;
  stale: boolean;
  rungs: {
    window: AccountAvailabilityWindowRung | null;
    week: AccountAvailabilityRung | null;
    models: Record<string, AccountAvailabilityRung>;
  };
  verdict: "available" | "cooked-week" | "cooked-window" | `model-cooked[${string}]`;
  usableModels: string[];
}

export interface NormalizeAccountAvailabilityOptions {
  accounts?: ReadonlyArray<{ id: string; provider: ProviderId }>;
  model?: string;
  now?: Date;
  thresholds?: Partial<AccountAvailabilityThresholds>;
}

export interface ReadAccountAvailabilityOptions extends NormalizeAccountAvailabilityOptions {
  storePath?: string;
}

export type AccountAvailabilityBand = "available" | "warn" | "cooked";

function thresholds(
  overrides: Partial<AccountAvailabilityThresholds> | undefined,
): AccountAvailabilityThresholds {
  const value = { ...DEFAULT_ACCOUNT_AVAILABILITY_THRESHOLDS, ...overrides };
  if (!Number.isFinite(value.warn) || !Number.isFinite(value.cooked)
      || value.warn < 0 || value.cooked > 100 || value.warn >= value.cooked)
    throw new Error("availability thresholds must satisfy 0 <= warn < cooked <= 100");
  return value;
}

export function accountAvailabilityBand(
  pct: number,
  overrides?: Partial<AccountAvailabilityThresholds>,
): AccountAvailabilityBand {
  const value = thresholds(overrides);
  if (pct >= value.cooked) return "cooked";
  if (pct >= value.warn) return "warn";
  return "available";
}

function modelName(value: string): string {
  const normalized = value.trim().toLowerCase();
  return normalized.startsWith("claude:model:")
    ? normalized.slice("claude:model:".length)
    : normalized;
}

function sourceBelongsToCachedUsage(observation: ProviderUsageObservation): boolean {
  return observation.provider === "anthropic"
    ? observation.source === "claude-agent-sdk:usage-control-experimental"
    : observation.source === "codex-app-server:account-rate-limits";
}

function newestCachedUsage(
  store: ProviderUsageObservationStore | undefined,
): ProviderUsageObservation[] {
  const newest = new Map<string, ProviderUsageObservation>();
  for (const observation of store?.observations ?? []) {
    if (!sourceBelongsToCachedUsage(observation)) continue;
    const previous = newest.get(observation.targetId);
    if (!previous || Date.parse(observation.observedAt) > Date.parse(previous.observedAt))
      newest.set(observation.targetId, observation);
  }
  return [...newest.values()];
}

function windowById(
  observation: ProviderUsageObservation,
  limitId: string,
): ProviderUsageWindow | undefined {
  return observation.windows?.find((window) => window.limitId === limitId);
}

function rung(window: ProviderUsageWindow | undefined): AccountAvailabilityRung | null {
  return window
    ? { pct: window.usedPercent, resetsAt: window.resetsAt }
    : null;
}

function observationIsStale(
  observation: ProviderUsageObservation,
  relevantWindows: ProviderUsageWindow[],
  now: Date,
): boolean {
  const ageMs = now.getTime() - Date.parse(observation.observedAt);
  return relevantWindows.some((window) => {
    const freshnessMs = window.limitId === "claude:five_hour"
      ? FIVE_HOURS_MS
      : SEVEN_DAYS_MS;
    return ageMs > freshnessMs;
  });
}

function accountMatches(
  observation: ProviderUsageObservation,
  accounts: NormalizeAccountAvailabilityOptions["accounts"],
): boolean {
  return !accounts || accounts.some(({ id, provider }) =>
    id === observation.targetId && provider === observation.provider);
}

export function normalizeAccountAvailability(
  store: ProviderUsageObservationStore | undefined,
  options: NormalizeAccountAvailabilityOptions = {},
): AccountAvailabilityRow[] {
  const limits = thresholds(options.thresholds);
  const selectedModel = options.model === undefined ? undefined : modelName(options.model);
  if (selectedModel !== undefined && !selectedModel)
    throw new Error("availability model must be non-empty");
  const now = options.now ?? new Date();
  const rows: AccountAvailabilityRow[] = [];

  for (const observation of newestCachedUsage(store)) {
    if (!accountMatches(observation, options.accounts)) continue;
    const window = observation.provider === "anthropic"
      ? windowById(observation, "claude:five_hour")
      : windowById(observation, "codex:primary");
    const week = observation.provider === "anthropic"
      ? windowById(observation, "claude:seven_day")
      : undefined;
    const modelWindows = (observation.windows ?? [])
      .filter(({ limitId }) => limitId?.startsWith("claude:model:"))
      .sort((left, right) => left.limitId!.localeCompare(right.limitId!));
    const models = Object.fromEntries(modelWindows.map((entry) => [
      modelName(entry.limitId!),
      { pct: entry.usedPercent, resetsAt: entry.resetsAt },
    ]));
    if (selectedModel !== undefined && models[selectedModel] === undefined) continue;

    const consideredModels = selectedModel === undefined
      ? Object.keys(models)
      : [selectedModel];
    const cookedModels = consideredModels
      .filter((name) => models[name]!.pct >= limits.cooked)
      .sort();
    const generalCooked = (week?.usedPercent ?? 0) >= limits.cooked
      || (window?.usedPercent ?? 0) >= limits.cooked;
    const usableModels = generalCooked ? [] : consideredModels
      .filter((name) => models[name]!.pct < limits.cooked)
      .sort();
    const verdict: AccountAvailabilityRow["verdict"] =
      (week?.usedPercent ?? 0) >= limits.cooked ? "cooked-week"
        : (window?.usedPercent ?? 0) >= limits.cooked ? "cooked-window"
          : cookedModels.length ? `model-cooked[${cookedModels.join(",")}]`
            : "available";
    const relevantWindows = [
      ...(window ? [window] : []),
      ...(week ? [week] : []),
      ...modelWindows.filter((entry) =>
        selectedModel === undefined || modelName(entry.limitId!) === selectedModel),
    ];

    rows.push({
      account: observation.targetId,
      provider: observation.provider,
      observedAt: observation.observedAt,
      stale: observationIsStale(observation, relevantWindows, now),
      rungs: {
        window: window
          ? {
              name: window.limitId === "claude:five_hour" ? "five_hour" : "primary",
              pct: window.usedPercent,
              resetsAt: window.resetsAt,
            }
          : null,
        week: rung(week),
        models,
      },
      verdict,
      usableModels,
    });
  }
  return rows.sort((left, right) => left.account.localeCompare(right.account));
}

/** Read and normalize persisted usage observations without invoking a provider control surface. */
export function readAccountAvailability(
  options: ReadAccountAvailabilityOptions = {},
): AccountAvailabilityRow[] {
  const storePath = options.storePath
    ?? process.env.NORTH_PROVIDER_OBSERVATIONS
    ?? DEFAULT_PROVIDER_OBSERVATIONS_PATH;
  return normalizeAccountAvailability(loadProviderUsageObservations(storePath), options);
}

export function accountAvailabilityRowIsUsable(
  row: AccountAvailabilityRow,
  model?: string,
): boolean {
  if (row.verdict === "cooked-week" || row.verdict === "cooked-window") return false;
  if (model === undefined) return true;
  return row.verdict === "available"
    && row.usableModels.includes(modelName(model));
}
