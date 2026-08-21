import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { basename, isAbsolute, resolve } from "node:path";
import { parseStrictJson } from "./strict-json";
import { trustedGitExecutable } from "./trusted-runtime";

export const DEFAULT_DELIVERY_LIVENESS_PATH = resolve(
  homedir(), ".local/state/firn/delivery-liveness.json",
);
export const MAX_DELIVERY_LIVENESS_FRESHNESS_SECONDS = 3_600;
export const DEFAULT_DELIVERY_LIVENESS_ACTIVATION_PATH =
  "/etc/north/delivery-liveness-required";

export type DeliveryDispatchClass = "feature" | "repair";

export interface DeliveryLivenessFact {
  version: 1;
  observedAt: string;
  freshnessSeconds: number;
  buildable: boolean;
  failingCheck: string | null;
  inputs: { nixosConfig: string };
  firn: { current: string | null; candidate: string | null };
}

export class DeliveryLivenessAuthorityError extends Error {
  constructor(readonly reason: string) {
    super(reason);
    this.name = "DeliveryLivenessAuthorityError";
  }
}

export function deliveryLivenessPath(
  environment: NodeJS.ProcessEnv = process.env,
): string {
  const home = environment.HOME?.trim();
  return resolve(home || homedir(), ".local/state/firn/delivery-liveness.json");
}

export function deliveryLivenessInputRevision(
  environment: NodeJS.ProcessEnv = process.env,
): string {
  const home = environment.HOME?.trim() || homedir();
  let git: string;
  try {
    git = trustedGitExecutable([
      environment.NORTH_GIT_BIN?.trim(),
      "/run/current-system/sw/bin/git",
      `${home}/.nix-profile/bin/git`,
    ]);
  } catch {
    authorityError("delivery_liveness_authority_input_unavailable");
  }
  const result = spawnSync(
    git,
    ["-C", resolve(home, "code/nixos-config/main"), "rev-parse", "--verify", "HEAD^{commit}"],
    {
      env: environment,
      encoding: "utf8",
      timeout: 2_000,
      maxBuffer: 4_096,
      stdio: ["ignore", "pipe", "ignore"],
    },
  );
  const revision = result.stdout?.trim();
  if (result.error || result.status !== 0 || !/^[0-9a-f]{40}$/.test(revision))
    authorityError("delivery_liveness_authority_input_unavailable");
  return revision;
}

function authorityError(reason: string): never {
  throw new DeliveryLivenessAuthorityError(reason);
}

function record(value: unknown, label: string): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value))
    authorityError(`delivery_liveness_authority_malformed:${label}`);
  return value as Record<string, unknown>;
}

function exactKeys(value: Record<string, unknown>, expected: readonly string[], label: string): void {
  const keys = Object.keys(value);
  if (keys.length !== expected.length
      || keys.some((key) => !expected.includes(key)))
    authorityError(`delivery_liveness_authority_malformed:${label}`);
}

function nonEmptyString(value: unknown, label: string): string {
  if (typeof value !== "string" || !value.trim())
    authorityError(`delivery_liveness_authority_malformed:${label}`);
  return value;
}

function storePathOrNull(value: unknown, label: string): string | null {
  if (value === null) return null;
  const path = nonEmptyString(value, label);
  if (!path.startsWith("/nix/store/") || path.includes("\0"))
    authorityError(`delivery_liveness_authority_malformed:${label}`);
  return path;
}

function observedAt(value: unknown): { raw: string; milliseconds: number } {
  const raw = nonEmptyString(value, "observed_at");
  const match = raw.match(
    /^(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2})(?:\.(\d{1,3}))?Z$/,
  );
  if (!match)
    authorityError("delivery_liveness_authority_malformed:observed_at");
  const milliseconds = Date.parse(raw);
  const canonical = `${match[1]}.${(match[2] ?? "").padEnd(3, "0")}Z`;
  if (!Number.isFinite(milliseconds)
      || new Date(milliseconds).toISOString() !== canonical)
    authorityError("delivery_liveness_authority_malformed:observed_at");
  return { raw, milliseconds };
}

function expectedContentDigest(path: string): string {
  let source: string;
  try { source = readFileSync(`${path}.sha256`, "utf8"); }
  catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT")
      authorityError("delivery_liveness_authority_missing:sha256");
    authorityError("delivery_liveness_authority_unreadable:sha256");
  }
  const match = source.match(/^([0-9a-f]{64})  ([^/\n]+)\n$/);
  if (!match || match[2] !== basename(path))
    authorityError("delivery_liveness_authority_malformed:sha256");
  return match[1];
}

/** Parse the sole deterministic Firn floor fact consumed by managed dispatch. */
export function parseDeliveryLivenessFact(value: unknown): DeliveryLivenessFact {
  const fact = record(value, "top_level");
  exactKeys(fact, [
    "version", "observed_at", "freshness_seconds", "buildable", "failing_check", "inputs", "firn",
  ], "top_level");
  if (fact.version !== 1) authorityError("delivery_liveness_authority_malformed:version");
  const observation = observedAt(fact.observed_at);
  if (!Number.isSafeInteger(fact.freshness_seconds)
      || (fact.freshness_seconds as number) <= 0
      || (fact.freshness_seconds as number) > MAX_DELIVERY_LIVENESS_FRESHNESS_SECONDS)
    authorityError("delivery_liveness_authority_malformed:freshness_seconds");
  if (typeof fact.buildable !== "boolean")
    authorityError("delivery_liveness_authority_malformed:buildable");
  if (fact.buildable ? fact.failing_check !== null
    : typeof fact.failing_check !== "string" || !fact.failing_check.trim())
    authorityError("delivery_liveness_authority_malformed:failing_check");

  const inputs = record(fact.inputs, "inputs");
  exactKeys(inputs, ["nixos_config"], "inputs");
  const nixosConfig = nonEmptyString(inputs.nixos_config, "inputs.nixos_config");
  if (!/^[0-9a-f]{40}$/.test(nixosConfig))
    authorityError("delivery_liveness_authority_malformed:inputs.nixos_config");

  const firn = record(fact.firn, "firn");
  exactKeys(firn, ["current", "candidate"], "firn");
  return {
    version: 1,
    observedAt: observation.raw,
    freshnessSeconds: fact.freshness_seconds as number,
    buildable: fact.buildable,
    failingCheck: fact.failing_check as string | null,
    inputs: { nixosConfig },
    firn: {
      current: storePathOrNull(firn.current, "firn.current"),
      candidate: storePathOrNull(firn.candidate, "firn.candidate"),
    },
  };
}

export function admitDeliveryLivenessFact(
  options: { path?: string; now?: Date; expectedNixosConfigRevision?: string } = {},
): DeliveryLivenessFact {
  const path = options.path ?? DEFAULT_DELIVERY_LIVENESS_PATH;
  const expectedDigest = expectedContentDigest(path);
  let source: string;
  try { source = readFileSync(path, "utf8"); }
  catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT")
      authorityError("delivery_liveness_authority_missing");
    authorityError("delivery_liveness_authority_unreadable");
  }
  const actualDigest = createHash("sha256").update(source).digest("hex");
  if (actualDigest !== expectedDigest)
    authorityError("delivery_liveness_authority_content_mismatch");
  let fact: DeliveryLivenessFact;
  try { fact = parseDeliveryLivenessFact(parseStrictJson(source, "Firn delivery liveness fact")); }
  catch (error) {
    if (error instanceof DeliveryLivenessAuthorityError) throw error;
    authorityError("delivery_liveness_authority_malformed:json");
  }
  if (options.expectedNixosConfigRevision !== undefined
      && fact.inputs.nixosConfig !== options.expectedNixosConfigRevision)
    authorityError("delivery_liveness_authority_input_changed");
  const now = (options.now ?? new Date()).getTime();
  const at = Date.parse(fact.observedAt);
  if (!Number.isFinite(now) || at > now || now - at > fact.freshnessSeconds * 1_000)
    authorityError("delivery_liveness_authority_stale");
  if (!fact.buildable)
    authorityError(`delivery_liveness_build_not_buildable:${fact.failingCheck}`);
  return fact;
}

export function deliveryDispatchClassFromEnvironment(
  environment: NodeJS.ProcessEnv = process.env,
): DeliveryDispatchClass {
  const value = environment.NORTH_DELIVERY_DISPATCH_CLASS ?? "feature";
  if (value === "feature" || value === "repair") return value;
  authorityError("delivery_liveness_dispatch_class_invalid");
}

/** Source may land before Firn's guarded system switch enables this floor. */
export function deliveryLivenessRequiredFromEnvironment(
  environment: NodeJS.ProcessEnv = process.env,
  activationPath = DEFAULT_DELIVERY_LIVENESS_ACTIVATION_PATH,
): boolean {
  const value = environment.NORTH_DELIVERY_LIVENESS_REQUIRED;
  if (value !== undefined && value !== "0" && value !== "1")
    authorityError("delivery_liveness_activation_invalid");
  const override = environment.NORTH_DELIVERY_LIVENESS_ACTIVATION_PATH;
  if (override !== undefined
      && (!isAbsolute(override) || resolve(override) !== override || override.includes("\0")))
    authorityError("delivery_liveness_activation_invalid");
  const paths = [activationPath, ...(override && override !== activationPath ? [override] : [])];
  for (const path of paths) {
    try {
      const source = readFileSync(path, "utf8");
      if (source !== "1\n") authorityError("delivery_liveness_activation_invalid");
      return true;
    } catch (error) {
      if (error instanceof DeliveryLivenessAuthorityError) throw error;
      if ((error as NodeJS.ErrnoException).code !== "ENOENT")
        authorityError("delivery_liveness_activation_unreadable");
    }
  }
  return false;
}
