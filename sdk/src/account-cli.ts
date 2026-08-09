import {
  addProviderAccount,
  listProviderAccounts,
  liveStatusProviderAccount,
  loginProviderAccount,
  requireProviderAccount,
  statusProviderAccount,
  type AccountAuthState,
  type ProviderAccount,
} from "./accounts";
import { refreshAccountUsages, type AccountUsageReport } from "./account-usage";
import {
  accountAvailabilityBand,
  accountAvailabilityRowIsUsable,
  readAccountAvailability,
  type AccountAvailabilityRow,
} from "./account-availability";
import { automatedPressure } from "./resource-policy";
import {
  DEFAULT_SESSION_ACTIVITY_HOURS,
  readOpenAISessionActivity,
  type OpenAISessionActivity,
} from "./openai-session-activity";
import type { ProviderUsageWindow } from "./providers/types";
import {
  createCliStyle,
  formatTokens,
  percentageGauge,
  type CliStyle,
} from "./cli-style";

const USAGE = `usage: north account <command>

  north account add <safe-id> <anthropic|openai>
  north account login <id>
  north account status [id]
  north account usage [id] [--refresh] [--hours N]  subscription windows + live session activity
  north account availability [--model M] [--json]  cached account headroom verdicts
  north account list [--verbose]   grouped accounts + live login state

Options:
  --model M  restrict usability to one cached model-scoped rung
  --json     emit the stable account availability row array
  --refresh  bypass the five-minute authoritative usage cache
  --hours N  session activity lookback in hours (default: 24)
  --verbose  include provider, profile, and storage root diagnostics`;

const ACCOUNT_GROUPS = [
  { provider: "anthropic", label: "Claude / Anthropic" },
  { provider: "openai", label: "Codex / OpenAI" },
] as const;

function authLabel(state: AccountAuthState): string {
  switch (state) {
    case "logged-in": return "logged in";
    case "not-logged-in": return "not logged in";
    case "auth-required": return "auth required";
    case "unverifiable": return "auth unverifiable";
    case "unavailable": return "CLI unavailable";
    case "error": return "auth check failed";
  }
}

function accountStates(accounts: ProviderAccount[]): Map<string, AccountAuthState> {
  return new Map(accounts.map((account) => [account.id, statusProviderAccount(account)]));
}

async function liveAccountStates(accounts: ProviderAccount[]): Promise<Map<string, AccountAuthState>> {
  const states = await Promise.all(accounts.map(async (account) => [
    account.id,
    await liveStatusProviderAccount(account),
  ] as const));
  return new Map(states);
}

export async function runAccountStatus(
  accounts: ProviderAccount[],
  statesFor = liveAccountStates,
): Promise<number> {
  const states = await statesFor(accounts);
  printAccountList(accounts, false, states);
  return accounts.every((account) => states.get(account.id) === "logged-in") ? 0 : 1;
}

function printAccountList(
  accounts: ProviderAccount[],
  verbose: boolean,
  states = accountStates(accounts),
): void {
  let firstGroup = true;
  for (const group of ACCOUNT_GROUPS) {
    const grouped = accounts.filter((account) => account.provider === group.provider);
    if (!grouped.length) continue;
    if (!firstGroup) console.log();
    firstGroup = false;
    console.log(group.label);
    const width = Math.max(...grouped.map((account) => account.id.length));
    for (const account of grouped) {
      console.log(`  ${account.id.padEnd(width)}  ${authLabel(states.get(account.id)!)}`);
      if (verbose) {
        console.log(`    provider: ${account.provider}`);
        console.log(`    profile:  ${account.profile}`);
        console.log(`    root:     ${account.root}`);
      }
    }
  }
}

// Reasons that mean "the codex probe's dependency could not be reached at
// all" (process missing, transport refused/died, or the probe never got a
// response inside its deadline) rather than a meaningful provider-side
// diagnostic (auth, schema). When every configured Codex account hits one of
// these in the same refresh, the user is in a blackout, not looking at N
// independent incidents — render one calm line instead of N noisy blocks.
const CODEX_COLLECTORS_OFFLINE_REASONS = new Set<AccountUsageReport["reason"]>([
  "codex_usage_command_unavailable",
  "codex_usage_transport_failed",
  "codex_usage_probe_timed_out",
]);

function codexCollectorsOffline(
  group: typeof ACCOUNT_GROUPS[number],
  grouped: ProviderAccount[],
  reports: AccountUsageReport[],
): boolean {
  if (group.provider !== "openai" || !grouped.length) return false;
  return grouped.every((account) => {
    const reason = reports.find(({ accountId }) => accountId === account.id)?.reason;
    return reason !== undefined && CODEX_COLLECTORS_OFFLINE_REASONS.has(reason);
  });
}

function usageReasonLabel(reason: AccountUsageReport["reason"]): string {
  switch (reason) {
    case "anthropic_usage_capability_unavailable": return "Claude SDK usage control is unavailable";
    case "anthropic_usage_probe_failed": return "Claude usage control probe failed";
    case "anthropic_usage_probe_timed_out": return "Claude usage control probe timed out";
    case "anthropic_usage_rate_limits_unavailable": return "Claude subscription rate limits are unavailable";
    case "anthropic_usage_response_schema_changed": return "Claude experimental usage response changed";
    case "anthropic_usage_windows_unavailable": return "Claude exposed no complete utilization/reset window";
    case "codex_usage_command_unavailable": return "Codex CLI is unavailable";
    case "codex_usage_probe_failed": return "Codex subscription rate-limit probe failed";
    case "codex_usage_probe_timed_out": return "Codex subscription rate-limit probe timed out";
    case "codex_usage_response_schema_changed": return "Codex rate-limit response changed";
    case "codex_usage_subscription_auth_required": return "Codex is not authenticated through ChatGPT";
    case "codex_usage_transport_failed": return "Codex app-server transport failed";
    case "codex_usage_windows_unavailable": return "Codex exposed no complete subscription window";
    case "usage_observation_store_unavailable": return "North could not persist the usage observation";
    default: return "usage unavailable";
  }
}

function activityAge(activity: OpenAISessionActivity, now: Date): string {
  if (!activity.lastActivityAt) return "-";
  const ageSeconds = Math.max(0, Math.floor((now.getTime() - activity.lastActivityAt.getTime()) / 1_000));
  if (ageSeconds < 60) return `${ageSeconds}s ago`;
  const ageMinutes = Math.floor(ageSeconds / 60);
  if (ageMinutes < 60) return `${ageMinutes}m ago`;
  const ageHours = Math.floor(ageMinutes / 60);
  if (ageHours < 24) return `${ageHours}h ago`;
  return `${Math.floor(ageHours / 24)}d ago`;
}

function activityHours(hours: number): string {
  return Number.isInteger(hours) ? hours.toFixed(0) : String(hours);
}

type UsageActivities = Map<string, OpenAISessionActivity>;

function usageWindowTiming(window: ProviderUsageWindow, now: Date): string {
  if (window.resetState === "untouched") return "window untouched";
  const elapsedMs = now.getTime() - Date.parse(window.resetsAt);
  if (elapsedMs <= 0) return `resets ${window.resetsAt}`;
  const elapsedMinutes = Math.floor(elapsedMs / (60 * 1_000));
  const hours = Math.floor(elapsedMinutes / 60);
  const minutes = elapsedMinutes % 60;
  return `stale — window elapsed ${String(hours).padStart(2, "0")}:${String(minutes).padStart(2, "0")} ago, awaiting recollection`;
}

function usageReportStatus(report: AccountUsageReport, now: Date): AccountUsageReport["status"] | "stale" {
  const windows = report.observation.windows;
  return windows?.length && windows.every((window) =>
    window.resetState !== "untouched" && Date.parse(window.resetsAt) <= now.getTime())
    ? "stale"
    : report.status;
}

function printPlainUsageReports(
  accounts: ProviderAccount[],
  reports: AccountUsageReport[],
  activities: UsageActivities,
  now: Date,
): void {
  let firstGroup = true;
  for (const group of ACCOUNT_GROUPS) {
    const grouped = accounts.filter((account) => account.provider === group.provider);
    if (!grouped.length) continue;
    if (!firstGroup) console.log();
    firstGroup = false;
    console.log(group.label);
    if (codexCollectorsOffline(group, grouped, reports)) {
      console.log("  codex usage: collectors offline");
      continue;
    }
    for (const account of grouped) {
      const report = reports.find(({ accountId }) => accountId === account.id)!;
      console.log(`  ${account.id}`);
      const headroom = automatedPressure(report.observation, now) ?? "unknown";
      console.log(`    headroom: ${headroom} (${usageReportStatus(report, now)}${report.cached ? ", cached" : ""})`);
      console.log(`    source:   ${report.source}`);
      if (report.lastSuccessfulObservedAt)
        console.log(`    usage evidence:  ${report.lastSuccessfulObservedAt}${report.cached ? " (cached)" : ""}`);
      if (report.collectionAttemptedAt)
        console.log(`    collection tried: ${report.collectionAttemptedAt}`);
      if (report.observation.windows?.length) {
        console.log("    windows:");
        for (const window of report.observation.windows)
          console.log(`      ${window.limitId ?? "subscription"}: ${window.usedPercent}% used · ${usageWindowTiming(window, now)}`);
      }
      for (const component of report.unavailableComponents)
        console.log(`    component unavailable: ${component.limitId} (${component.reason})`);
      if (report.reason)
        console.log(`    reason: ${usageReasonLabel(report.reason)} (${report.reason})`);
      const activity = activities.get(account.id);
      if (activity) {
        console.log(`    activity: read live from provider session records (last ${activityHours(activity.hours)}h)`);
        console.log(`      sessions:      ${activity.sessions}`);
        console.log(`      live now:      ${activity.live}`);
        console.log(`      output tokens: ${activity.outputTokens.toLocaleString("en-US")}`);
        console.log(`      last activity: ${activityAge(activity, now)}`);
      }
    }
  }
}

function headroomLabel(style: CliStyle, headroom: string): string {
  switch (headroom) {
    case "plenty": return style.ok(headroom);
    case "normal": return style.accent(headroom);
    case "low": return style.warn(headroom);
    case "exhausted": return style.crit(headroom);
    default: return style.dim(headroom);
  }
}

function gaugeLabel(style: CliStyle, usedPercent: number): string {
  const gauge = percentageGauge(usedPercent);
  if (usedPercent >= 100) return style.crit(gauge);
  if (usedPercent >= 80) return style.warn(gauge);
  if (usedPercent >= 50) return style.accent(gauge);
  return style.ok(gauge);
}

function printStyledUsageReports(
  style: CliStyle,
  accounts: ProviderAccount[],
  reports: AccountUsageReport[],
  activities: UsageActivities,
  now: Date,
): void {
  let firstGroup = true;
  for (const group of ACCOUNT_GROUPS) {
    const grouped = accounts.filter((account) => account.provider === group.provider);
    if (!grouped.length) continue;
    if (!firstGroup) console.log();
    firstGroup = false;
    console.log(style.section(group.label));
    if (codexCollectorsOffline(group, grouped, reports)) {
      console.log(style.warn("  codex usage: collectors offline"));
      continue;
    }
    for (const [accountIndex, account] of grouped.entries()) {
      if (accountIndex) console.log();
      const report = reports.find(({ accountId }) => accountId === account.id)!;
      const headroom = automatedPressure(report.observation, now) ?? "unknown";
      console.log(`${style.accent(account.id)}  ${headroomLabel(style, headroom)}`);
      console.log(style.pairs([
        ["status", `${usageReportStatus(report, now)}${report.cached ? " · cached" : ""}`],
      ], "  "));
      if (report.observation.windows?.length) {
        console.log(style.dim("  windows"));
        console.log(style.pairs(report.observation.windows.map((window) => [
          window.limitId ?? "subscription",
          `${gaugeLabel(style, window.usedPercent)} · ${usageWindowTiming(window, now)}`,
        ]), "    "));
      }
      for (const component of report.unavailableComponents)
        console.log(style.warn(`  component unavailable: ${component.limitId} (${component.reason})`));
      if (report.reason)
        console.log(style.crit(`  reason: ${usageReasonLabel(report.reason)} (${report.reason})`));
      const activity = activities.get(account.id);
      if (activity) {
        console.log(style.dim(`  activity · live provider session records · last ${activityHours(activity.hours)}h`));
        console.log(style.pairs([
          ["sessions", String(activity.sessions)],
          ["live now", String(activity.live)],
          ["output tokens", formatTokens(activity.outputTokens)],
          ["last activity", activityAge(activity, now)],
        ], "    "));
      }
      const provenance: Array<readonly [string, string]> = [["source", report.source]];
      if (report.lastSuccessfulObservedAt)
        provenance.push(["evidence", `${report.lastSuccessfulObservedAt}${report.cached ? " · cached" : ""}`]);
      if (report.collectionAttemptedAt) provenance.push(["collection tried", report.collectionAttemptedAt]);
      console.log(style.dim(style.pairs(provenance, "  ")));
    }
  }
}

async function printUsageReports(
  accounts: ProviderAccount[],
  reports: AccountUsageReport[],
  hours: number,
): Promise<void> {
  const now = new Date();
  const activities = new Map((await Promise.all(accounts
    .filter(({ provider }) => provider === "openai")
    .map(async (account) => [
      account.id,
      await readOpenAISessionActivity({ accountRoot: account.root, hours, now }),
    ] as const))).map((entry) => entry));
  const style = createCliStyle();
  if (!style.enabled) {
    printPlainUsageReports(accounts, reports, activities, now);
    return;
  }
  printStyledUsageReports(style, accounts, reports, activities, now);
}

function availabilityPct(pct: number): string {
  const band = accountAvailabilityBand(pct);
  return `${pct}%${band === "available" ? "" : ` (${band})`}`;
}

function availabilityTiming(rung: { resetsAt?: string; resetState?: "untouched" }): string {
  return rung.resetState === "untouched" ? "window untouched" : `resets ${rung.resetsAt}`;
}

function printAvailabilityRows(rows: AccountAvailabilityRow[]): void {
  if (!rows.length) {
    console.log("no cached account availability");
    return;
  }
  for (const row of rows) {
    console.log(`${row.account} (${row.provider})  ${row.verdict}${row.stale ? " · stale" : ""}`);
    console.log(`  observed: ${row.observedAt}`);
    if (row.rungs.window)
      console.log(`  window ${row.rungs.window.name}: ${availabilityPct(row.rungs.window.pct)} · ${availabilityTiming(row.rungs.window)}`);
    if (row.rungs.week)
      console.log(`  week: ${availabilityPct(row.rungs.week.pct)} · ${availabilityTiming(row.rungs.week)}`);
    for (const [model, rung] of Object.entries(row.rungs.models))
      console.log(`  model ${model}: ${availabilityPct(rung.pct)} · ${availabilityTiming(rung)}`);
    console.log(`  usable models: ${row.usableModels.length ? row.usableModels.join(", ") : "none observed"}`);
  }
}

function availabilityOptions(args: string[]): { json: boolean; model?: string } {
  let json = false;
  let model: string | undefined;
  for (let index = 0; index < args.length; index += 1) {
    const entry = args[index];
    if (entry === "--json") {
      if (json) throw new Error(USAGE);
      json = true;
      continue;
    }
    if (entry === "--model") {
      if (model !== undefined || index + 1 >= args.length || args[index + 1]!.startsWith("--"))
        throw new Error(USAGE);
      model = args[index + 1]!;
      index += 1;
      continue;
    }
    throw new Error(USAGE);
  }
  return { json, ...(model === undefined ? {} : { model }) };
}

export async function runAccountCli(args: string[]): Promise<number> {
  const [command, ...rest] = args;
  try {
    switch (command) {
      case "add": {
        if (rest.length !== 2) throw new Error(USAGE);
        const account = await addProviderAccount(rest[0], rest[1]);
        console.log(`added isolated ${account.provider} account ${account.id}`);
        console.log(`root ${account.root}`);
        return 0;
      }
      case "login": {
        if (rest.length !== 1) throw new Error(USAGE);
        const account = requireProviderAccount(rest[0]);
        const status = loginProviderAccount(account);
        if (status === 0) console.log(`login complete for ${account.id}`);
        else if (status === 127) console.error(`${account.provider} CLI is not installed`);
        else console.error(`login failed for ${account.id}`);
        return status;
      }
      case "status": {
        if (rest.length > 1) throw new Error(USAGE);
        const accounts = rest.length ? [requireProviderAccount(rest[0])] : listProviderAccounts();
        if (!accounts.length) {
          console.log("no isolated accounts configured");
          return 0;
        }
        return runAccountStatus(accounts);
      }
      case "usage": {
        const refresh = rest.includes("--refresh");
        let hours = DEFAULT_SESSION_ACTIVITY_HOURS;
        const ids: string[] = [];
        for (let index = 0; index < rest.length; index += 1) {
          const entry = rest[index]!;
          if (entry === "--refresh") continue;
          if (entry === "--hours") {
            const value = rest[index + 1];
            hours = Number(value);
            if (value === undefined || !Number.isFinite(hours) || hours <= 0) throw new Error(USAGE);
            index += 1;
            continue;
          }
          if (entry.startsWith("--")) throw new Error(USAGE);
          ids.push(entry);
        }
        if (ids.length > 1) throw new Error(USAGE);
        const accounts = ids.length ? [requireProviderAccount(ids[0])] : listProviderAccounts();
        if (!accounts.length) {
          console.log("no isolated accounts configured");
          return 0;
        }
        const reports = await refreshAccountUsages({ accounts, force: refresh });
        await printUsageReports(accounts, reports, hours);
        return reports.every(({ status }) => status === "observed") ? 0 : 1;
      }
      case "availability": {
        const { json, model } = availabilityOptions(rest);
        const accounts = listProviderAccounts();
        const rows = readAccountAvailability({
          accounts,
          ...(model === undefined ? {} : { model }),
        });
        if (json) console.log(JSON.stringify(rows, null, 2));
        else printAvailabilityRows(rows);
        return rows.some((row) => accountAvailabilityRowIsUsable(row, model)) ? 0 : 1;
      }
      case "list": {
        const verbose = rest.length === 1 && rest[0] === "--verbose";
        if (rest.length && !verbose) throw new Error(USAGE);
        const accounts = listProviderAccounts();
        if (!accounts.length) {
          console.log("no isolated accounts configured");
          return 0;
        }
        printAccountList(accounts, verbose);
        return 0;
      }
      case "help":
      case "--help":
      case "-h":
        console.log(USAGE);
        return 0;
      default:
        throw new Error(USAGE);
    }
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    return 2;
  }
}

if (import.meta.main) process.exit(await runAccountCli(process.argv.slice(2)));
