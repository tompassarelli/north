import { createHash } from "node:crypto";
import { execFileSync, spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { getChildren, getThreadFacts, type Fact } from "./north-client";
import {
  modelFamily,
  resolveModelAlias,
  resolveTier,
  SEMANTIC_TIER_ORDER,
  type SemanticTier,
} from "./providers/catalog";
import {
  ROUTING_PIN_POLICY_VERSION,
  type RoutingPinEvidence,
} from "./routing-economics";
import type { AccountAvailabilityRow } from "./account-availability";
import type { ProviderId } from "./providers/types";
import {
  framBabashkaArguments,
  framCoordinatorChildTimeout,
  framEngineEnvironment,
} from "./fram-engine";

const REPO = resolve(import.meta.dir, "..", "..");
const DEFAULT_THRESHOLD = 80;
const PIN_LIFETIME_MS = 60 * 60 * 1_000;

export interface AvailabilityRung {
  pct: number;
  resetsAt: string;
}

export type AvailabilityRow = AccountAvailabilityRow;

export type FailoverClassification =
  | "available"
  | "unknown"
  | "account-dead"
  | "window-dead"
  | "model-dead";

export interface ActiveSessionRoute {
  provider: ProviderId;
  account: string;
  model?: string;
  tier?: SemanticTier;
}

export interface RungTrigger {
  rung: "week" | "window" | "model";
  name: string;
  pct: number;
  resetsAt: string;
  model?: string;
}

export interface HeirRoute {
  provider: ProviderId;
  account: string;
  model: string;
  tier: SemanticTier;
  observedAt: string;
}

export interface FailoverCheck {
  threshold: number;
  classification: FailoverClassification;
  active: ActiveSessionRoute;
  unknownReason?: string;
  trigger?: RungTrigger;
  heir?: HeirRoute;
  receipts: {
    active: AvailabilityRow;
    heir?: AvailabilityRow;
  };
}

export interface ThreadMapEntry {
  id: string;
  title?: string;
  facts: Fact[];
}

export interface FailoverContextPackage {
  brief: {
    path: string;
    sha256: string;
    content: string;
  };
  threadMap: ThreadMapEntry[];
}

export interface FailoverSpawn {
  version: 1;
  check: FailoverCheck;
  context: FailoverContextPackage;
  pinEvidence: RoutingPinEvidence;
  prompt: string;
  command: {
    executable: string;
    args: string[];
  };
  notification: {
    executable: string;
    args: string[];
    target: string;
    subject: "PROVIDER FAILOVER FIRED";
    body: string;
  };
}

type SpawnResult = {
  error?: Error;
  status: number | null;
  stderr?: string | Buffer;
};

export interface FailoverRuntime {
  env?: NodeJS.ProcessEnv;
  now?: Date;
  northBin?: string;
  peerBb?: string;
  msgCli?: string;
  readBrief?: (path: string) => string;
  getFacts?: typeof getThreadFacts;
  getChildren?: typeof getChildren;
  loadRows?: () => AvailabilityRow[];
  run?: (executable: string, args: string[]) => SpawnResult;
}

export interface FailoverWarning {
  version: 1;
  thread?: string;
  threshold: number;
  active: ActiveSessionRoute;
  observedAt: string;
  crossing: RungTrigger;
  automaticFire: boolean;
}

function record(value: unknown, label: string): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw new Error(`${label} must be an object`);
  return value as Record<string, unknown>;
}

function exactFields(value: Record<string, unknown>, expected: readonly string[], label: string): void {
  const fields = Object.keys(value);
  const unknown = fields.filter((field) => !expected.includes(field));
  const missing = expected.filter((field) => !fields.includes(field));
  if (unknown.length || missing.length)
    throw new Error(`${label} fields mismatch (missing=${missing.join(",") || "none"}; unknown=${unknown.join(",") || "none"})`);
}

function text(value: unknown, label: string): string {
  if (typeof value !== "string" || !value.trim()) throw new Error(`${label} must be a non-empty string`);
  return value;
}

function percent(value: unknown, label: string): number {
  if (typeof value !== "number" || !Number.isFinite(value) || value < 0 || value > 100)
    throw new Error(`${label} must be a number from 0 through 100`);
  return value;
}

function timestamp(value: unknown, label: string): string {
  const rendered = text(value, label);
  if (!Number.isFinite(Date.parse(rendered))) throw new Error(`${label} must be an ISO timestamp`);
  return rendered;
}

function availabilityVerdict(
  value: unknown,
  label: string,
): AvailabilityRow["verdict"] {
  const verdict = text(value, label);
  if (!/^(available|unknown|cooked-week|cooked-window|model-cooked\[[^\]]+\])$/.test(verdict))
    throw new Error(`${label} is outside the pinned contract`);
  return verdict as AvailabilityRow["verdict"];
}

function parseRung(value: unknown, label: string, named = false): AvailabilityRung & { name?: string } {
  const raw = record(value, label);
  exactFields(raw, named ? ["name", "pct", "resetsAt"] : ["pct", "resetsAt"], label);
  return {
    ...(named ? { name: text(raw.name, `${label}.name`) } : {}),
    pct: percent(raw.pct, `${label}.pct`),
    resetsAt: timestamp(raw.resetsAt, `${label}.resetsAt`),
  };
}

function parseNullableRung(
  value: unknown,
  label: string,
  named = false,
): (AvailabilityRung & { name?: string }) | null {
  return value === null ? null : parseRung(value, label, named);
}

export function parseAvailabilityRows(value: unknown): AvailabilityRow[] {
  if (!Array.isArray(value)) throw new Error("account availability JSON must be an array");
  const rows = value.map((entry, index) => {
    const label = `account availability row[${index}]`;
    const raw = record(entry, label);
    exactFields(
      raw,
      ["account", "provider", "observedAt", "stale", "rungs", "verdict", "usableModels"],
      label,
    );
    const provider = text(raw.provider, `${label}.provider`);
    if (provider !== "anthropic" && provider !== "openai")
      throw new Error(`${label}.provider must be anthropic or openai`);
    if (typeof raw.stale !== "boolean") throw new Error(`${label}.stale must be boolean`);
    const rungs = record(raw.rungs, `${label}.rungs`);
    exactFields(rungs, ["window", "week", "models"], `${label}.rungs`);
    const models = record(rungs.models, `${label}.rungs.models`);
    const parsedModels = Object.fromEntries(Object.entries(models).map(([model, rung]) => [
      text(model, `${label}.rungs.models key`),
      parseRung(rung, `${label}.rungs.models.${model}`),
    ]));
    if (!Array.isArray(raw.usableModels) || !raw.usableModels.every((model) => typeof model === "string"))
      throw new Error(`${label}.usableModels must be an array of strings`);
    return {
      account: text(raw.account, `${label}.account`),
      provider,
      observedAt: timestamp(raw.observedAt, `${label}.observedAt`),
      stale: raw.stale,
      rungs: {
        window: parseNullableRung(
          rungs.window,
          `${label}.rungs.window`,
          true,
        ) as AvailabilityRow["rungs"]["window"],
        week: parseNullableRung(rungs.week, `${label}.rungs.week`),
        models: parsedModels,
      },
      verdict: availabilityVerdict(raw.verdict, `${label}.verdict`),
      usableModels: [...raw.usableModels],
    } satisfies AvailabilityRow;
  });
  const identities = rows.map(({ provider, account }) => `${provider}\u0000${account}`);
  if (new Set(identities).size !== identities.length)
    throw new Error("account availability JSON contains duplicate provider/account rows");
  return rows;
}

export function loadAvailabilityRows(
  northBin = process.env.NORTH_BIN ?? `${REPO}/bin/north`,
  invoke: typeof execFileSync = execFileSync,
): AvailabilityRow[] {
  const output = invoke(northBin, ["account", "availability", "--json"], {
    encoding: "utf8",
    timeout: 10_000,
    stdio: ["ignore", "pipe", "pipe"],
  });
  try {
    return parseAvailabilityRows(JSON.parse(String(output)));
  } catch (error) {
    throw new Error(`north account availability --json returned invalid data: ${(error as Error).message}`);
  }
}

export function failoverThreshold(value: unknown = DEFAULT_THRESHOLD): number {
  const parsed = typeof value === "number" ? value : Number(value);
  if (!Number.isFinite(parsed) || parsed <= 0 || parsed > 100)
    throw new Error("failover threshold must be greater than 0 and at most 100");
  return parsed;
}

function normalizedModel(provider: ProviderId, model: string): string {
  return resolveModelAlias(provider, model) ?? model;
}

function modelMatches(provider: ProviderId, candidate: string, expected: string): boolean {
  const exactCandidate = normalizedModel(provider, candidate);
  const exactExpected = normalizedModel(provider, expected);
  return exactCandidate === exactExpected
    || modelFamily(provider, exactCandidate) === expected
    || candidate === modelFamily(provider, exactExpected);
}

export function semanticTierForModel(
  provider: ProviderId,
  model: string,
): SemanticTier | undefined {
  const exact = normalizedModel(provider, model);
  const matches = SEMANTIC_TIER_ORDER.filter((tier) =>
    resolveTier(provider, tier).model === exact);
  return matches.length === 1 ? matches[0] : undefined;
}

function activeRow(rows: readonly AvailabilityRow[], route: ActiveSessionRoute): AvailabilityRow {
  const providerRows = rows.filter(({ provider }) => provider === route.provider);
  const matches = providerRows.filter(({ account }) => account === route.account);
  if (matches.length === 1) return matches[0];
  if (!route.account && providerRows.length === 1) return providerRows[0];
  throw new Error(`active account ${route.account || "(unspecified)"} is not a unique ${route.provider} availability row`);
}

export function availabilityForRoute(
  rows: readonly AvailabilityRow[],
  route: ActiveSessionRoute,
): AvailabilityRow {
  return activeRow(rows, route);
}

function triggerFor(row: AvailabilityRow, model: string | undefined, threshold: number): RungTrigger | undefined {
  if (row.rungs.week && row.rungs.week.pct >= threshold) {
    return {
      rung: "week", name: "week", pct: row.rungs.week.pct,
      resetsAt: row.rungs.week.resetsAt,
    };
  }
  if (row.rungs.window && row.rungs.window.pct >= threshold) {
    return {
      rung: "window", name: row.rungs.window.name, pct: row.rungs.window.pct,
      resetsAt: row.rungs.window.resetsAt,
    };
  }
  if (!model) return undefined;
  const entry = Object.entries(row.rungs.models)
    .find(([candidate]) => modelMatches(row.provider, candidate, model));
  if (!entry || entry[1].pct < threshold) return undefined;
  return {
    rung: "model", name: entry[0], model: entry[0], pct: entry[1].pct,
    resetsAt: entry[1].resetsAt,
  };
}

function unknownAvailabilityReason(row: AvailabilityRow): string | undefined {
  if (!row.rungs.window)
    return `${row.provider}/${row.account} window rung is unavailable`;
  if (row.provider === "anthropic" && !row.rungs.week)
    return `${row.provider}/${row.account} week rung is unavailable`;
  if (row.verdict === "unknown")
    return `${row.provider}/${row.account} availability verdict is unknown`;
  return undefined;
}

function classification(trigger: RungTrigger | undefined): FailoverClassification {
  if (!trigger) return "available";
  if (trigger.rung === "week") return "account-dead";
  if (trigger.rung === "window") return "window-dead";
  return "model-dead";
}

function candidateUsable(
  row: AvailabilityRow,
  model: string,
  threshold: number,
): boolean {
  if (row.stale || unknownAvailabilityReason(row)
      || (row.rungs.week !== null && row.rungs.week.pct >= threshold)
      || (row.rungs.window !== null && row.rungs.window.pct >= threshold))
    return false;
  const scoped = Object.entries(row.rungs.models)
    .find(([candidate]) => modelMatches(row.provider, candidate, model));
  if (scoped && scoped[1].pct >= threshold) return false;
  return row.usableModels.some((candidate) => modelMatches(row.provider, candidate, model));
}

function heirFor(
  rows: readonly AvailabilityRow[],
  active: ActiveSessionRoute,
  tier: SemanticTier | undefined,
  threshold: number,
): { route: HeirRoute; receipt: AvailabilityRow } | undefined {
  if (!tier) return undefined;
  const candidates = rows.flatMap((row) => {
    if (row.provider === active.provider && row.account === active.account) return [];
    const model = resolveTier(row.provider, tier).model;
    if (!model || !candidateUsable(row, model, threshold)) return [];
    return [{
      route: {
        provider: row.provider,
        account: row.account,
        model,
        tier,
        observedAt: row.observedAt,
      },
      receipt: row,
    }];
  });
  return candidates.sort((left, right) => {
    const leftSameProvider = left.route.provider === active.provider ? 1 : 0;
    const rightSameProvider = right.route.provider === active.provider ? 1 : 0;
    return leftSameProvider - rightSameProvider
      || left.route.account.localeCompare(right.route.account);
  })[0];
}

export function checkFailover(
  rows: readonly AvailabilityRow[],
  route: ActiveSessionRoute,
  thresholdValue: unknown = DEFAULT_THRESHOLD,
): FailoverCheck {
  const threshold = failoverThreshold(thresholdValue);
  const receipt = activeRow(rows, route);
  if (receipt.stale)
    throw new Error(`active availability evidence for ${receipt.provider}/${receipt.account} is stale`);
  const canonicalActive = {
    ...route,
    account: receipt.account,
    ...(route.model ? { model: normalizedModel(route.provider, route.model) } : {}),
  };
  const tier = canonicalActive.tier
    ?? (canonicalActive.model ? semanticTierForModel(route.provider, canonicalActive.model) : undefined);
  const active = { ...canonicalActive, ...(tier ? { tier } : {}) };
  const unknownReason = unknownAvailabilityReason(receipt);
  if (unknownReason) {
    return {
      threshold,
      classification: "unknown",
      active,
      unknownReason,
      receipts: { active: receipt },
    };
  }
  const trigger = triggerFor(receipt, active.model, threshold);
  const heir = trigger ? heirFor(rows, active, tier, threshold) : undefined;
  return {
    threshold,
    classification: classification(trigger),
    active,
    ...(trigger ? { trigger } : {}),
    ...(heir ? { heir: heir.route } : {}),
    receipts: {
      active: receipt,
      ...(heir ? { heir: heir.receipt } : {}),
    },
  };
}

export function thresholdCrossings(
  row: AvailabilityRow,
  thresholdValue: unknown = DEFAULT_THRESHOLD,
): RungTrigger[] {
  const threshold = failoverThreshold(thresholdValue);
  return [
    ...(row.rungs.week !== null && row.rungs.week.pct >= threshold ? [{
      rung: "week" as const, name: "week", pct: row.rungs.week.pct,
      resetsAt: row.rungs.week.resetsAt,
    }] : []),
    ...(row.rungs.window !== null && row.rungs.window.pct >= threshold ? [{
      rung: "window" as const, name: row.rungs.window.name, pct: row.rungs.window.pct,
      resetsAt: row.rungs.window.resetsAt,
    }] : []),
    ...Object.entries(row.rungs.models)
      .filter(([, rung]) => rung.pct >= threshold)
      .map(([model, rung]) => ({
        rung: "model" as const, name: model, model, pct: rung.pct, resetsAt: rung.resetsAt,
      })),
  ];
}

export function contextPackage(
  rootThread: string,
  briefPath: string,
  runtime: FailoverRuntime = {},
): FailoverContextPackage {
  const readBrief = runtime.readBrief ?? ((path: string) => readFileSync(path, "utf8"));
  const getFacts = runtime.getFacts ?? getThreadFacts;
  const children = (runtime.getChildren ?? getChildren)(rootThread);
  const ids = [rootThread, ...children];
  const threadMap = ids.map((id) => {
    const facts = getFacts(id);
    return {
      id,
      ...(facts.find(({ predicate }) => predicate === "title")?.value
        ? { title: facts.find(({ predicate }) => predicate === "title")!.value }
        : {}),
      facts,
    };
  });
  const content = readBrief(briefPath);
  return {
    brief: {
      path: briefPath,
      sha256: createHash("sha256").update(content).digest("hex"),
      content,
    },
    threadMap,
  };
}

function recoveryDetail(check: FailoverCheck): string {
  return `provider-recovery ${JSON.stringify({
    threshold: check.threshold,
    trigger: check.trigger,
    activeReceipt: check.receipts.active,
    heirReceipt: check.receipts.heir,
  })}`;
}

export function recoveryPinEvidence(check: FailoverCheck, now = new Date()): RoutingPinEvidence {
  if (!check.heir) throw new Error("cannot compose provider-recovery pin evidence without an heir");
  return {
    policyVersion: ROUTING_PIN_POLICY_VERSION,
    issuedAt: now.toISOString(),
    expiresAt: new Date(now.getTime() + PIN_LIFETIME_MS).toISOString(),
    reasonCode: "provider-recovery",
    detail: recoveryDetail(check),
    pins: [
      { kind: "provider", value: check.heir.provider },
      { kind: "account", value: check.heir.account },
      { kind: "model", value: check.heir.model },
    ],
  };
}

function contextPrompt(rootThread: string, context: FailoverContextPackage): string {
  return [
    `You are the heir team-lead orchestrator for root thread @${rootThread}.`,
    "Continue the workstream from this sealed succession context package.",
    "Treat the embedded North thread facts as asserted coordination state; reconcile fresh evidence before acting.",
    "",
    `BRIEF ${context.brief.path} sha256=${context.brief.sha256}`,
    context.brief.content,
    "",
    "THREAD MAP",
    JSON.stringify(context.threadMap),
  ].join("\n");
}

export function composeFailoverSpawn(
  check: FailoverCheck,
  rootThread: string,
  briefPath: string,
  notifyTarget: string,
  runtime: FailoverRuntime = {},
): FailoverSpawn {
  if (check.classification === "unknown")
    throw new Error(`failover fire refused: active availability is unknown (${check.unknownReason ?? "required rung unavailable"})`);
  if (check.classification === "available")
    throw new Error("failover fire refused: active route has not crossed the threshold");
  if (!check.heir)
    throw new Error("failover fire refused: no same-tier provider/account/model heir has fresh capacity");
  if (!notifyTarget.trim())
    throw new Error("failover fire requires NORTH_FAILOVER_NOTIFY or AGENT_COORDINATOR");
  const now = runtime.now ?? new Date();
  const context = contextPackage(rootThread, briefPath, runtime);
  const pinEvidence = recoveryPinEvidence(check, now);
  const prompt = contextPrompt(rootThread, context);
  const northBin = runtime.northBin ?? process.env.NORTH_BIN ?? `${REPO}/bin/north`;
  const subject = "PROVIDER FAILOVER FIRED" as const;
  const body = `@${rootThread} -> ${check.heir.provider}/${check.heir.account}/${check.heir.model} (${check.heir.tier}); reason=provider-recovery`;
  const env = runtime.env ?? process.env;
  const sender = env.AGENT_ID ?? "north-failover";
  const port = env.NORTH_PORT ?? "7977";
  return {
    version: 1,
    check,
    context,
    pinEvidence,
    prompt,
    command: {
      executable: northBin,
      args: [
        "spawn", "team-lead", prompt,
        "--thread", rootThread,
        "--provider", check.heir.provider,
        "--target", check.heir.account,
        "--model", check.heir.model,
        "--pin-evidence", JSON.stringify(pinEvidence),
        "--notify", notifyTarget,
      ],
    },
    notification: {
      executable: runtime.peerBb ?? env.NORTH_PEER_BB ?? "bb",
      args: framBabashkaArguments([
        runtime.msgCli ?? `${REPO}/cli/msg-cli.clj`,
        port,
        "send",
        sender,
        notifyTarget,
        subject,
        body,
      ], env),
      target: notifyTarget,
      subject,
      body,
    },
  };
}

function runChecked(
  run: NonNullable<FailoverRuntime["run"]>,
  executable: string,
  args: string[],
  label: string,
): void {
  const result = run(executable, args);
  if (result.error || result.status !== 0) {
    const detail = String(result.stderr ?? result.error?.message ?? "").trim();
    throw new Error(`${label} failed${detail ? `: ${detail}` : ""}`);
  }
}

export function fireFailover(spawn: FailoverSpawn, runtime: FailoverRuntime = {}): void {
  const runSpawn = runtime.run ?? ((executable: string, args: string[]) =>
    spawnSync(executable, args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }));
  runChecked(runSpawn, spawn.command.executable, spawn.command.args, "heir spawn");
  try {
    const runNotification = runtime.run ?? ((executable: string, args: string[]) =>
      spawnSync(executable, args, {
        encoding: "utf8",
        env: framEngineEnvironment(runtime.env ?? process.env),
        timeout: framCoordinatorChildTimeout(),
        stdio: ["ignore", "pipe", "pipe"],
      }));
    runNotification(spawn.notification.executable, spawn.notification.args);
  } catch {
    // The spawn is authoritative once it succeeds. Notification is advisory.
  }
}

export function activeSessionRoute(
  rows: readonly AvailabilityRow[],
  providerOverride: string | undefined,
  env: NodeJS.ProcessEnv = process.env,
  identityFacts: readonly Fact[] = [],
): ActiveSessionRoute {
  const identityValue = (predicate: string) =>
    identityFacts.find((fact) => fact.predicate === predicate)?.value;
  const rawProvider = providerOverride ?? (
    env.AGENT_PROVIDER === "anthropic" || env.AGENT_PROVIDER === "openai"
      ? env.AGENT_PROVIDER
      : identityValue("provider")
  );
  if (rawProvider !== "anthropic" && rawProvider !== "openai")
    throw new Error("active provider is unavailable; pass --provider anthropic|openai");
  const providerRows = rows.filter(({ provider }) => provider === rawProvider);
  const account = env.AGENT_TARGET
    ?? identityValue("provider_target")
    ?? (providerRows.length === 1 ? providerRows[0].account : "");
  const rawTier = env.AGENT_TIER;
  const tier = SEMANTIC_TIER_ORDER.includes(rawTier as SemanticTier)
    ? rawTier as SemanticTier
    : undefined;
  const model = env.AGENT_MODEL ?? identityValue("model");
  return {
    provider: rawProvider,
    account,
    ...(model ? { model } : {}),
    ...(tier ? { tier } : {}),
  };
}

export function activeSessionIdentityFacts(
  providerOverride: string | undefined,
  runtime: FailoverRuntime = {},
): Fact[] {
  const env = runtime.env ?? process.env;
  if (!env.AGENT_ID) return [];
  const providerKnown = providerOverride === "anthropic" || providerOverride === "openai"
    || env.AGENT_PROVIDER === "anthropic" || env.AGENT_PROVIDER === "openai";
  if (providerKnown && env.AGENT_TARGET && env.AGENT_MODEL) return [];
  return (runtime.getFacts ?? getThreadFacts)(`agent:${env.AGENT_ID}`);
}

export function automaticFailoverFireEnabled(
  env: NodeJS.ProcessEnv = process.env,
): boolean {
  return ["1", "true", "on"].includes((env.NORTH_FAILOVER_AUTO_FIRE ?? "").toLowerCase());
}

export function failoverWarningCommands(
  warning: FailoverWarning,
  runtime: FailoverRuntime = {},
): Array<{ executable: string; args: string[] }> {
  const env = runtime.env ?? process.env;
  const northBin = runtime.northBin ?? env.NORTH_BIN ?? `${REPO}/bin/north`;
  const commands: Array<{ executable: string; args: string[] }> = [];
  if (warning.thread) {
    commands.push({
      executable: northBin,
      args: ["tell", warning.thread, "failover_warning", JSON.stringify(warning)],
    });
  }
  const notify = env.NORTH_FAILOVER_NOTIFY ?? env.AGENT_COORDINATOR;
  if (notify) {
    const route = [
      warning.active.provider,
      warning.active.account,
      warning.active.model,
    ].filter(Boolean).join("/");
    commands.push({
      executable: runtime.peerBb ?? env.NORTH_PEER_BB ?? "bb",
      args: framBabashkaArguments([
        runtime.msgCli ?? `${REPO}/cli/msg-cli.clj`,
        env.NORTH_PORT ?? "7977",
        "send",
        env.AGENT_ID ?? "north-failover",
        notify,
        "PROVIDER CAPACITY WARNING",
        `${route} ${warning.crossing.rung}:${warning.crossing.name}=${warning.crossing.pct}% `
          + `threshold=${warning.threshold} resets=${warning.crossing.resetsAt}; `
          + `automatic-fire=${warning.automaticFire ? "enabled" : "off"}`,
      ], env),
    });
  }
  return commands;
}

function runBestEffort(
  executable: string,
  args: string[],
  runtime: FailoverRuntime,
): void {
  try {
    const run = runtime.run ?? ((command: string, commandArgs: string[]) =>
      spawnSync(command, commandArgs, {
        encoding: "utf8",
        env: framEngineEnvironment(runtime.env ?? process.env),
        timeout: framCoordinatorChildTimeout(10_000),
        stdio: ["ignore", "ignore", "ignore"],
      }));
    run(executable, args);
  } catch {
    // Usage observations remain advisory. Warning transport cannot break routing.
  }
}

/**
 * Detection hook for the account-usage sampling boundary. It reads only lane A's
 * cached availability JSON, emits warnings first, and invokes automatic fire
 * only under the explicit default-off gate.
 */
export function observeFailoverUsageSample(
  runtime: FailoverRuntime = {},
): FailoverWarning[] {
  const env = runtime.env ?? process.env;
  const provider = env.AGENT_PROVIDER;
  if (provider !== "anthropic" && provider !== "openai" && !env.AGENT_ID) return [];
  try {
    const rows = (runtime.loadRows ?? (() => loadAvailabilityRows(runtime.northBin)))();
    const active = activeSessionRoute(
      rows,
      undefined,
      env,
      activeSessionIdentityFacts(undefined, runtime),
    );
    const receipt = availabilityForRoute(rows, active);
    if (receipt.stale) return [];
    const threshold = failoverThreshold(env.NORTH_FAILOVER_WARN_THRESHOLD ?? DEFAULT_THRESHOLD);
    const automaticFire = automaticFailoverFireEnabled(env);
    const warnings = thresholdCrossings(receipt, threshold).map((crossing) => ({
      version: 1 as const,
      ...(env.AGENT_THREAD ? { thread: env.AGENT_THREAD } : {}),
      threshold,
      active,
      observedAt: receipt.observedAt,
      crossing,
      automaticFire,
    }));
    for (const warning of warnings) {
      for (const command of failoverWarningCommands(warning, runtime))
        runBestEffort(command.executable, command.args, runtime);
    }

    // Warn-first is literal ordering: every warning fact/mail command above is
    // attempted before the gated fire command is even composed.
    if (automaticFire && warnings.length) {
      const root = env.NORTH_FAILOVER_ROOT_THREAD ?? env.AGENT_THREAD;
      const brief = env.NORTH_FAILOVER_BRIEF;
      const check = checkFailover(rows, active, threshold);
      if (root && brief && check.classification !== "available" && check.heir) {
        runBestEffort(
          runtime.northBin ?? env.NORTH_BIN ?? `${REPO}/bin/north`,
          ["failover", "fire", "--thread", root, "--brief", brief],
          runtime,
        );
      }
    }
    return warnings;
  } catch {
    // Missing/stale lane-A data is unknown capacity, never a routing failure.
    return [];
  }
}
