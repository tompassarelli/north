import { createHash, randomUUID } from "node:crypto";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import { withFileLease } from "./file-lease";
import { getThreadFacts, normalizeNorthEntityId } from "./north-client";
import type { ProviderId } from "./providers/types";
import type { RoutingRequest } from "./routing-metadata";
import {
  gitOracleEnvironment, trustedGitExecutable, trustedGitProjectRoot,
} from "./trusted-runtime";
import tier1Prompt from "./tier1-distiller-prompt.md" with { type: "text" };
import tier1Routing from "./tier1-distiller-routing.md" with { type: "text" };

const SHA256 = /^[a-f0-9]{64}$/;
const CURSOR_BASENAME = /^\.cursors\.v4\.([a-z0-9-]+-[0-9a-f]{16})$/;
const RAW_BASENAME = /^[A-Za-z0-9][A-Za-z0-9._-]*\.jsonl$/;
const SAFE_SLUG = /^[a-z0-9][a-z0-9-]*$/;
const ARTIFACT_BODY_MARKER = "<!-- north-tier1-body -->\n";
const CLAIM_VERSION = 1 as const;
const DEFAULT_CLAIM_LEASE_MS = 60 * 60_000;
const MAX_TIER1_BODY_BYTES = 128 * 1_024;

export const TIER1_DISTILLER_PROMPT = tier1Prompt.trim();
export const TIER1_DISTILLER_PROMPT_SHA256 = sha256(TIER1_DISTILLER_PROMPT);
export const TIER1_DISTILLER_ROUTING_SHA256 = sha256(tier1Routing.trim());
export const TIER1_DISTILLER_ROUTING = parseRoutingRequest(tier1Routing);

export interface Tier1ProjectIdentity {
  readonly repository: string;
  readonly digest: string;
  readonly slug: string;
}

export interface Tier1CursorProvenance {
  readonly cursor: string;
  readonly cursorRecordSha256: string;
  readonly cursorBytes: number;
  readonly cursorSnapshotToken: string;
  readonly provider: ProviderId;
  readonly namespace: string;
  readonly lineage: string;
  readonly lineageDigest: string;
  readonly rawSource: string;
  readonly rawSnapshotSha256: string;
  readonly sourceReceipt: string;
  readonly sourceReceiptSha256: string;
  readonly settledAt: string;
  readonly sourceAgent: string;
  readonly sourceRun: string;
  readonly sourceThread: string;
  readonly sessionId: string;
}

export interface Tier1ModelRequest {
  readonly input: string;
  readonly inputSha256: string;
  readonly project: Tier1ProjectIdentity;
  readonly provenance: Tier1CursorProvenance;
  readonly streamThread: string;
  readonly attemptAgentId: string;
  readonly signal?: AbortSignal;
}

export interface Tier1ModelResult {
  readonly body: string;
  readonly execution: {
    readonly provider: ProviderId;
    readonly wireRunId: string;
    readonly wirePromptSha256: string;
    readonly promptManifestSha256: string;
    readonly environmentReceiptSha256: string;
    readonly availableSkillCatalogSha256: string;
    readonly activatedResourceClosureSha256: string;
  };
}

export type Tier1ModelRunner = (request: Tier1ModelRequest) => Promise<Tier1ModelResult>;

export interface Tier1DistillationOptions {
  readonly rawDirectory: string;
  readonly distillationsDirectory: string;
  readonly stateDirectory: string;
  readonly project: Tier1ProjectIdentity;
  readonly streamThread: string;
  readonly lineageDigest?: string;
  readonly runner: Tier1ModelRunner;
  readonly signal?: AbortSignal;
  readonly runtime?: Tier1DistillerRuntime;
}

export interface Tier1DistillerRuntime {
  readonly now?: () => Date;
  readonly uuid?: () => string;
  readonly pid?: number;
  readonly processAlive?: (pid: number) => boolean;
  readonly claimLeaseMs?: number;
  readonly verifyStreamThread?: (thread: string) => Promise<void> | void;
  readonly afterArtifactWrite?: (artifactPath: string) => Promise<void> | void;
}

export interface Tier1DistillationResult {
  readonly status: "created" | "recovered" | "already_complete";
  readonly artifactPath: string;
  readonly artifactSha256: string;
  readonly lineageDigest: string;
  readonly projectDigest: string;
  readonly streamThread: string;
}

interface CursorRecord {
  readonly bytes: number;
  readonly source: string;
  readonly provider: ProviderId;
  readonly namespace: string;
  readonly lineage: string;
  readonly lineageDigest: string;
  readonly destination: string;
  readonly prefixDigest: string;
  readonly snapshotToken: string;
}

interface LaunchReceipt {
  readonly agentId: string;
  readonly runId: string;
  readonly threadId: string;
  readonly settledAt: string;
}

interface SettledSnapshot {
  readonly provenance: Tier1CursorProvenance;
  readonly transcript: string;
  readonly date: string;
}

interface ClaimAttempt {
  readonly token: string;
  readonly pid: number;
  readonly agentId: string;
  readonly claimedAt: string;
  readonly leaseExpiresAt: string;
}

interface ClaimCompletion {
  readonly artifactSha256: string;
  readonly completedAt: string;
}

interface ClaimPublication {
  readonly artifactSha256: string;
}

interface ClaimState {
  readonly version: typeof CLAIM_VERSION;
  status: "available" | "claimed" | "complete";
  readonly projectDigest: string;
  readonly streamThread: string;
  readonly inputSha256: string;
  readonly provenance: Tier1CursorProvenance;
  readonly artifactBasename: string;
  attempt?: ClaimAttempt;
  publication?: ClaimPublication;
  completion?: ClaimCompletion;
  lastFailure?: { readonly at: string; readonly detail: string };
}

interface ClaimedSnapshot {
  readonly statePath: string;
  readonly lockPath: string;
  readonly artifactPath: string;
  readonly snapshot: SettledSnapshot;
  readonly state: ClaimState;
  readonly attempt: ClaimAttempt;
}

interface PreparedClaim {
  readonly complete?: Tier1DistillationResult;
  readonly claim?: ClaimedSnapshot;
}

function sha256(value: string | Uint8Array): string {
  return createHash("sha256").update(value).digest("hex");
}

function parseRoutingRequest(source: string): RoutingRequest {
  let value: unknown;
  try { value = JSON.parse(source); }
  catch (cause) { throw new Error("tier-1 routing contract is invalid JSON", { cause }); }
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("tier-1 routing contract must be an object");
  }
  return value as RoutingRequest;
}

function requireDigest(value: unknown, label: string): string {
  if (typeof value !== "string" || !SHA256.test(value)) {
    throw new Error(`${label} must be a lowercase SHA-256 digest`);
  }
  return value;
}

function requireString(value: unknown, label: string): string {
  if (typeof value !== "string" || !value || value !== value.trim() || value.includes("\0")) {
    throw new Error(`${label} must be a non-empty exact string`);
  }
  return value;
}

function parseInstant(value: unknown, label: string): string {
  const instant = requireString(value, label);
  if (!/^\d{4}-\d{2}-\d{2}T/.test(instant) || !Number.isFinite(Date.parse(instant))) {
    throw new Error(`${label} must be an ISO-8601 instant`);
  }
  return instant;
}

function projectSlug(repository: string): string {
  const withoutSuffix = repository.replace(/[\/#]+$/, "").replace(/\.git$/i, "");
  const leaf = withoutSuffix.split(/[\/:]/).at(-1) ?? "project";
  const slug = leaf.toLowerCase().replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "").slice(0, 48).replace(/-+$/g, "");
  return slug || "project";
}

export function tier1ProjectIdentity(repositoryInput: string): Tier1ProjectIdentity {
  const repository = requireString(repositoryInput, "project repository identity");
  if (/[\u0000-\u001f\u007f]/u.test(repository)) {
    throw new Error("project repository identity contains control characters");
  }
  if (Buffer.byteLength(repository, "utf8") > 2_048) {
    throw new Error("project repository identity exceeds 2048 UTF-8 bytes");
  }
  return Object.freeze({
    repository,
    digest: sha256(`north-tier1-project-v1\0${repository}`),
    slug: projectSlug(repository),
  });
}

export async function tier1ProjectIdentityFromRoot(rootInput: string): Promise<Tier1ProjectIdentity> {
  const git = trustedGitExecutable();
  const requested = await fs.promises.realpath(path.resolve(rootInput));
  const root = trustedGitProjectRoot(requested, git);
  const marker = path.join(root, ".git");
  try { await fs.promises.lstat(marker); }
  catch (cause) {
    throw new Error(`project root is not a Git checkout: ${root}`, { cause });
  }
  if (requested !== root && !requested.startsWith(`${root}${path.sep}`)) {
    throw new Error(`project path escapes its trusted Git root: ${requested}`);
  }
  const child = Bun.spawn([
    git, "-C", root, "config", "--local", "--no-includes", "--get", "remote.origin.url",
  ], {
    env: gitOracleEnvironment(),
    stdin: "ignore",
    stdout: "pipe",
    stderr: "pipe",
  });
  const [exitCode, stdout] = await Promise.all([
    child.exited,
    new Response(child.stdout).text(),
    new Response(child.stderr).text(),
  ]);
  if (exitCode !== 0) {
    throw new Error(`project root has no readable remote.origin.url: ${root}`);
  }
  const repository = stdout.endsWith("\n") ? stdout.slice(0, -1) : stdout;
  if (!repository || repository.includes("\n") || repository.includes("\r")) {
    throw new Error(`project root has a non-canonical remote.origin.url: ${root}`);
  }
  return tier1ProjectIdentity(repository);
}

function safeRelativeControlPath(value: string, label: string): string {
  if (value.includes("/") || value.includes("\\") || value === "." || value === "..") {
    throw new Error(`${label} is not a safe raw control basename`);
  }
  return `streams/raw/${value}`;
}

function parseCursorRecord(line: string, cursorPath: string): CursorRecord {
  const fields = line.split("\t");
  if (fields.length !== 10 || fields[0] !== "v4") {
    throw new Error(`tier-1 cursor has an invalid v4 record: ${cursorPath}`);
  }
  const [_, bytesText, source, provider, namespace, lineage, lineageDigest,
    destination, prefixDigest, snapshotToken] = fields;
  if (!/^(0|[1-9][0-9]*)$/.test(bytesText!)) {
    throw new Error(`tier-1 cursor has an invalid byte offset: ${cursorPath}`);
  }
  const bytes = Number(bytesText);
  if (!Number.isSafeInteger(bytes) || bytes < 1) {
    throw new Error(`tier-1 cursor byte offset is unsupported: ${cursorPath}`);
  }
  if (provider !== "openai" && provider !== "anthropic") {
    throw new Error(`tier-1 cursor provider is unsupported: ${cursorPath}`);
  }
  if (!namespace || !/^managed-[A-Za-z0-9._:-]+$/.test(namespace)) {
    throw new Error(`tier-1 cursor is not a managed settled-session authority: ${cursorPath}`);
  }
  if (!lineage || lineage.startsWith("/") || lineage.includes("\\")
      || lineage.split("/").some((part) => !part || part === "." || part === "..")) {
    throw new Error(`tier-1 cursor lineage is unsafe: ${cursorPath}`);
  }
  if (!source?.startsWith("/") || source.includes("\t") || source.includes("\n")) {
    throw new Error(`tier-1 cursor source path is unsafe: ${cursorPath}`);
  }
  if (!destination || !RAW_BASENAME.test(destination)) {
    throw new Error(`tier-1 cursor destination is unsafe: ${cursorPath}`);
  }
  const canonicalLineageDigest = sha256(
    `north-stream-source-v4\0${provider}\0${namespace}\0${lineage}`,
  );
  if (requireDigest(lineageDigest, "cursor lineage digest") !== canonicalLineageDigest
      || !destination.endsWith(`.${lineageDigest}.jsonl`)) {
    throw new Error(`tier-1 cursor lineage identity is contradictory: ${cursorPath}`);
  }
  return Object.freeze({
    bytes,
    source,
    provider,
    namespace,
    lineage,
    lineageDigest,
    destination,
    prefixDigest: requireDigest(prefixDigest, "cursor prefix digest"),
    snapshotToken: requireDigest(snapshotToken, "cursor snapshot token"),
  });
}

async function readRegularFile(filePath: string, label: string): Promise<Uint8Array> {
  const info = await fs.promises.lstat(filePath);
  if (!info.isFile() || info.isSymbolicLink()) {
    throw new Error(`${label} is not a regular file: ${filePath}`);
  }
  return fs.promises.readFile(filePath);
}

function parseLaunchReceipt(source: string, receiptPath: string): LaunchReceipt {
  let value: unknown;
  try { value = JSON.parse(source); }
  catch (cause) { throw new Error(`tier-1 source receipt is invalid JSON: ${receiptPath}`, { cause }); }
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`tier-1 source receipt is not an object: ${receiptPath}`);
  }
  const record = value as Record<string, unknown>;
  return Object.freeze({
    agentId: normalizeNorthEntityId(requireString(record.agentId, "source receipt agentId")),
    runId: normalizeNorthEntityId(requireString(record.runId, "source receipt runId")),
    threadId: normalizeNorthEntityId(requireString(record.threadId, "source receipt threadId")),
    settledAt: parseInstant(record.settledAt, "source receipt settledAt"),
  });
}

function parseSessionMetadata(transcript: string, rawPath: string): {
  repository: string;
  sessionId: string;
} {
  const firstLine = transcript.slice(0, transcript.indexOf("\n") < 0
    ? transcript.length : transcript.indexOf("\n"));
  let value: unknown;
  try { value = JSON.parse(firstLine); }
  catch (cause) { throw new Error(`tier-1 raw source lacks valid session metadata: ${rawPath}`, { cause }); }
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`tier-1 raw source session metadata is not an object: ${rawPath}`);
  }
  const row = value as Record<string, unknown>;
  if (row.type !== "session_meta" || !row.payload || typeof row.payload !== "object"
      || Array.isArray(row.payload)) {
    throw new Error(`tier-1 raw source does not begin with Codex session_meta: ${rawPath}`);
  }
  const payload = row.payload as Record<string, unknown>;
  const git = payload.git;
  if (!git || typeof git !== "object" || Array.isArray(git)) {
    throw new Error(`tier-1 raw source session metadata lacks Git identity: ${rawPath}`);
  }
  const repository = requireString(
    (git as Record<string, unknown>).repository_url,
    "session metadata git.repository_url",
  );
  const sessionId = requireString(
    payload.session_id ?? payload.id,
    "session metadata session id",
  );
  return { repository, sessionId };
}

async function settledSnapshots(
  rawDirectoryInput: string,
  project: Tier1ProjectIdentity,
): Promise<SettledSnapshot[]> {
  const rawDirectory = await fs.promises.realpath(path.resolve(rawDirectoryInput));
  const entries = await fs.promises.readdir(rawDirectory, { withFileTypes: true });
  const snapshots: SettledSnapshot[] = [];
  const seenMatching = new Set<string>();
  for (const entry of entries.sort((left, right) => left.name.localeCompare(right.name))) {
    const match = CURSOR_BASENAME.exec(entry.name);
    if (!match || !entry.isFile() || entry.isSymbolicLink()) continue;
    const scope = match[1]!;
    const cursorPath = path.join(rawDirectory, entry.name);
    const cursorBytes = await readRegularFile(cursorPath, "tier-1 cursor");
    const cursorSource = new TextDecoder("utf-8", { fatal: true }).decode(cursorBytes);
    const receiptBasename = `source-receipt.${scope}.json`;
    const receiptPath = path.join(rawDirectory, receiptBasename);
    let receiptBytes: Uint8Array;
    try { receiptBytes = await readRegularFile(receiptPath, "tier-1 source receipt"); }
    catch (error) {
      if (error instanceof Error && "code" in error && error.code === "ENOENT") continue;
      throw error;
    }
    for (const line of cursorSource.split("\n")) {
      if (!line) continue;
      const cursor = parseCursorRecord(line, cursorPath);
      const rawPath = path.join(rawDirectory, cursor.destination);
      const rawBytes = await readRegularFile(rawPath, "tier-1 raw snapshot");
      if (rawBytes.byteLength !== cursor.bytes || sha256(rawBytes) !== cursor.prefixDigest) {
        throw new Error(`tier-1 raw snapshot disagrees with its exact cursor: ${rawPath}`);
      }
      const transcript = new TextDecoder("utf-8", { fatal: true }).decode(rawBytes);
      const metadata = parseSessionMetadata(transcript, rawPath);
      if (metadata.repository !== project.repository) continue;
      if (seenMatching.has(cursor.lineageDigest)) {
        throw new Error(`tier-1 raw store has duplicate project lineage ${cursor.lineageDigest}`);
      }
      seenMatching.add(cursor.lineageDigest);
      const receiptSource = new TextDecoder("utf-8", { fatal: true }).decode(receiptBytes);
      const receipt = parseLaunchReceipt(receiptSource, receiptPath);
      if (!cursor.lineage.endsWith(metadata.sessionId)) {
        throw new Error(`tier-1 cursor lineage disagrees with session metadata: ${rawPath}`);
      }
      const date = cursor.destination.slice(0, 10);
      if (!/^\d{4}-\d{2}-\d{2}$/.test(date)) {
        throw new Error(`tier-1 raw destination lacks a canonical date: ${rawPath}`);
      }
      snapshots.push(Object.freeze({
        transcript,
        date,
        provenance: Object.freeze({
          cursor: safeRelativeControlPath(entry.name, "cursor"),
          cursorRecordSha256: sha256(line),
          cursorBytes: cursor.bytes,
          cursorSnapshotToken: cursor.snapshotToken,
          provider: cursor.provider,
          namespace: cursor.namespace,
          lineage: cursor.lineage,
          lineageDigest: cursor.lineageDigest,
          rawSource: safeRelativeControlPath(cursor.destination, "raw source"),
          rawSnapshotSha256: cursor.prefixDigest,
          sourceReceipt: safeRelativeControlPath(receiptBasename, "source receipt"),
          sourceReceiptSha256: sha256(receiptBytes),
          settledAt: receipt.settledAt,
          sourceAgent: receipt.agentId,
          sourceRun: receipt.runId,
          sourceThread: receipt.threadId,
          sessionId: metadata.sessionId,
        }),
      }));
    }
  }
  return snapshots.sort((left, right) =>
    left.provenance.lineageDigest.localeCompare(right.provenance.lineageDigest));
}

function defaultProcessAlive(pid: number): boolean {
  if (!Number.isSafeInteger(pid) || pid < 1) return false;
  try { process.kill(pid, 0); return true; }
  catch (error) {
    return error instanceof Error && "code" in error && error.code === "EPERM";
  }
}

function runtimeValues(runtime: Tier1DistillerRuntime | undefined): Required<Pick<
  Tier1DistillerRuntime,
  "now" | "uuid" | "pid" | "processAlive" | "claimLeaseMs"
>> {
  const claimLeaseMs = runtime?.claimLeaseMs ?? DEFAULT_CLAIM_LEASE_MS;
  if (!Number.isSafeInteger(claimLeaseMs) || claimLeaseMs < 1) {
    throw new Error("tier-1 claim lease must be a positive safe integer");
  }
  return {
    now: runtime?.now ?? (() => new Date()),
    uuid: runtime?.uuid ?? randomUUID,
    pid: runtime?.pid ?? process.pid,
    processAlive: runtime?.processAlive ?? defaultProcessAlive,
    claimLeaseMs,
  };
}

async function ensurePrivateDirectory(directory: string): Promise<void> {
  await fs.promises.mkdir(directory, { recursive: true, mode: 0o700 });
  const info = await fs.promises.lstat(directory);
  if (!info.isDirectory() || info.isSymbolicLink()) {
    throw new Error(`tier-1 state path is not a private directory: ${directory}`);
  }
  await fs.promises.chmod(directory, 0o700);
}

async function syncDirectory(directory: string): Promise<void> {
  const handle = await fs.promises.open(directory, "r");
  try { await handle.sync(); }
  finally { await handle.close(); }
}

async function atomicReplace(filePath: string, content: string): Promise<void> {
  const directory = path.dirname(filePath);
  await ensurePrivateDirectory(directory);
  const temporary = `${filePath}.${process.pid}.${randomUUID()}.tmp`;
  let handle: fs.promises.FileHandle | undefined;
  try {
    handle = await fs.promises.open(temporary, "wx", 0o600);
    await handle.writeFile(content, "utf8");
    await handle.sync();
    await handle.close();
    handle = undefined;
    await fs.promises.rename(temporary, filePath);
    await fs.promises.chmod(filePath, 0o600);
    await syncDirectory(directory);
  } finally {
    await handle?.close().catch(() => {});
    await fs.promises.rm(temporary, { force: true });
  }
}

async function atomicWriteOnce(filePath: string, content: string): Promise<boolean> {
  const directory = path.dirname(filePath);
  await fs.promises.mkdir(directory, { recursive: true });
  const directoryInfo = await fs.promises.lstat(directory);
  if (!directoryInfo.isDirectory() || directoryInfo.isSymbolicLink()) {
    throw new Error(`tier-1 distillations path is not a directory: ${directory}`);
  }
  const temporary = `${filePath}.${process.pid}.${randomUUID()}.tmp`;
  let handle: fs.promises.FileHandle | undefined;
  try {
    handle = await fs.promises.open(temporary, "wx", 0o600);
    await handle.writeFile(content, "utf8");
    await handle.sync();
    await handle.close();
    handle = undefined;
    try {
      await fs.promises.link(temporary, filePath);
      await syncDirectory(directory);
      return true;
    } catch (error) {
      if (!(error instanceof Error && "code" in error && error.code === "EEXIST")) throw error;
      return false;
    }
  } finally {
    await handle?.close().catch(() => {});
    await fs.promises.rm(temporary, { force: true });
  }
}

async function readOptional(filePath: string): Promise<string | undefined> {
  try {
    const info = await fs.promises.lstat(filePath);
    if (!info.isFile() || info.isSymbolicLink()) {
      throw new Error(`tier-1 state path is not a regular file: ${filePath}`);
    }
    return await Bun.file(filePath).text();
  } catch (error) {
    if (error instanceof Error && "code" in error && error.code === "ENOENT") return undefined;
    throw error;
  }
}

function parseClaimState(source: string, statePath: string): ClaimState {
  let value: unknown;
  try { value = JSON.parse(source); }
  catch (cause) { throw new Error(`tier-1 claim state is invalid JSON: ${statePath}`, { cause }); }
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`tier-1 claim state is not an object: ${statePath}`);
  }
  const raw = value as Record<string, unknown>;
  const provenance = raw.provenance;
  const attempt = raw.attempt;
  const publication = raw.publication;
  const completion = raw.completion;
  if (raw.version !== CLAIM_VERSION
      || !["available", "claimed", "complete"].includes(String(raw.status))
      || typeof raw.projectDigest !== "string" || !SHA256.test(raw.projectDigest)
      || typeof raw.streamThread !== "string"
      || normalizeNorthEntityId(raw.streamThread) !== raw.streamThread
      || typeof raw.inputSha256 !== "string" || !SHA256.test(raw.inputSha256)
      || typeof raw.artifactBasename !== "string" || !raw.artifactBasename
      || raw.artifactBasename.includes("/") || raw.artifactBasename.includes("\\")
      || !provenance || typeof provenance !== "object" || Array.isArray(provenance)) {
    throw new Error(`tier-1 claim state is malformed: ${statePath}`);
  }
  const sourceProvenance = provenance as Record<string, unknown>;
  for (const digestField of [
    "cursorRecordSha256", "cursorSnapshotToken", "lineageDigest",
    "rawSnapshotSha256", "sourceReceiptSha256",
  ]) {
    if (typeof sourceProvenance[digestField] !== "string"
        || !SHA256.test(sourceProvenance[digestField])) {
      throw new Error(`tier-1 claim state is malformed: ${statePath}`);
    }
  }
  if (raw.status === "claimed" && (!attempt || typeof attempt !== "object" || Array.isArray(attempt))) {
    throw new Error(`tier-1 claimed state lacks its owner: ${statePath}`);
  }
  if (attempt !== undefined) {
    if (!attempt || typeof attempt !== "object" || Array.isArray(attempt)) {
      throw new Error(`tier-1 claim owner is malformed: ${statePath}`);
    }
    const owner = attempt as Record<string, unknown>;
    if (typeof owner.token !== "string" || !owner.token
        || !Number.isSafeInteger(owner.pid) || (owner.pid as number) < 1
        || typeof owner.agentId !== "string" || normalizeNorthEntityId(owner.agentId) !== owner.agentId) {
      throw new Error(`tier-1 claim owner is malformed: ${statePath}`);
    }
    parseInstant(owner.claimedAt, "tier-1 claim claimedAt");
    parseInstant(owner.leaseExpiresAt, "tier-1 claim leaseExpiresAt");
  }
  if (publication !== undefined) {
    if (!publication || typeof publication !== "object" || Array.isArray(publication)) {
      throw new Error(`tier-1 planned publication is malformed: ${statePath}`);
    }
    requireDigest(
      (publication as Record<string, unknown>).artifactSha256,
      "tier-1 planned artifact digest",
    );
  }
  if (raw.status === "complete") {
    if (!publication || typeof publication !== "object" || Array.isArray(publication)
        || !completion || typeof completion !== "object" || Array.isArray(completion)) {
      throw new Error(`tier-1 completed state lacks its receipt: ${statePath}`);
    }
    const receipt = completion as Record<string, unknown>;
    const completedDigest = requireDigest(
      receipt.artifactSha256, "tier-1 completed artifact digest",
    );
    if (completedDigest !== (publication as Record<string, unknown>).artifactSha256) {
      throw new Error(`tier-1 completed state contradicts its planned publication: ${statePath}`);
    }
    parseInstant(receipt.completedAt, "tier-1 completion instant");
  }
  return raw as unknown as ClaimState;
}

async function loadClaimState(statePath: string): Promise<ClaimState | undefined> {
  const source = await readOptional(statePath);
  return source === undefined ? undefined : parseClaimState(source, statePath);
}

async function writeClaimState(statePath: string, state: ClaimState): Promise<void> {
  await atomicReplace(statePath, `${JSON.stringify(state, null, 2)}\n`);
}

function sameProvenance(left: Tier1CursorProvenance, right: Tier1CursorProvenance): boolean {
  return JSON.stringify(left) === JSON.stringify(right);
}

function artifactBasename(snapshot: SettledSnapshot, project: Tier1ProjectIdentity): string {
  if (!SAFE_SLUG.test(project.slug)) throw new Error("tier-1 project slug is unsafe");
  return `${snapshot.date}-${project.slug}.${snapshot.provenance.lineageDigest}.tier1.md`;
}

function claimPaths(
  options: Tier1DistillationOptions,
  snapshot: SettledSnapshot,
): { statePath: string; lockPath: string; artifactPath: string; artifactBasename: string } {
  const projectDirectory = path.resolve(options.stateDirectory, "projects", options.project.digest);
  const statePath = path.resolve(projectDirectory, `${snapshot.provenance.lineageDigest}.json`);
  const lockPath = `${statePath}.lock`;
  const basename = artifactBasename(snapshot, options.project);
  const artifactPath = path.resolve(options.distillationsDirectory, basename);
  if (path.dirname(statePath) !== projectDirectory
      || path.dirname(artifactPath) !== path.resolve(options.distillationsDirectory)) {
    throw new Error("tier-1 claim path escaped its authority root");
  }
  return { statePath, lockPath, artifactPath, artifactBasename: basename };
}

function headerValue(source: string, key: string): unknown {
  if (!source.startsWith("---\n")) throw new Error("tier-1 artifact lacks frontmatter");
  const end = source.indexOf("\n---\n", 4);
  if (end < 0) throw new Error("tier-1 artifact frontmatter is unterminated");
  for (const line of source.slice(4, end).split("\n")) {
    const separator = line.indexOf(": ");
    if (separator < 1 || line.slice(0, separator) !== key) continue;
    try { return JSON.parse(line.slice(separator + 2)); }
    catch { throw new Error(`tier-1 artifact has invalid ${key} metadata`); }
  }
  throw new Error(`tier-1 artifact lacks ${key} metadata`);
}

function validateArtifact(
  source: string,
  state: ClaimState,
  claimToken: string,
): { artifactSha256: string; bodySha256: string } {
  const expected: ReadonlyArray<readonly [string, string | number]> = [
    ["north-tier1", "north:tier1:v1"],
    ["project-sha256", state.projectDigest],
    ["raw-lineage-sha256", state.provenance.lineageDigest],
    ["raw-snapshot-sha256", state.provenance.rawSnapshotSha256],
    ["raw-snapshot-bytes", state.provenance.cursorBytes],
    ["cursor-record-sha256", state.provenance.cursorRecordSha256],
    ["cursor-snapshot-token", state.provenance.cursorSnapshotToken],
    ["raw-source", state.provenance.rawSource],
    ["raw-lineage", state.provenance.lineage],
    ["cursor", state.provenance.cursor],
    ["source-receipt", state.provenance.sourceReceipt],
    ["source-receipt-sha256", state.provenance.sourceReceiptSha256],
    ["source-provider", state.provenance.provider],
    ["source-namespace", state.provenance.namespace],
    ["source-session", state.provenance.sessionId],
    ["source-agent", `@${state.provenance.sourceAgent}`],
    ["source-run", `@${state.provenance.sourceRun}`],
    ["source-thread", `@${state.provenance.sourceThread}`],
    ["stream-thread", `@${state.streamThread}`],
    ["settled-at", state.provenance.settledAt],
    ["claim-token", claimToken],
    ["distiller-tier", "economy"],
    ["distiller-prompt-template-sha256", TIER1_DISTILLER_PROMPT_SHA256],
    ["distiller-routing-sha256", TIER1_DISTILLER_ROUTING_SHA256],
    ["distiller-input-sha256", state.inputSha256],
  ];
  for (const [key, value] of expected) {
    if (headerValue(source, key) !== value) {
      throw new Error(`tier-1 artifact ${key} disagrees with its durable claim`);
    }
  }
  const provider = headerValue(source, "distiller-provider");
  if (provider !== "anthropic") {
    throw new Error("tier-1 artifact was not produced by the sealed data-only provider");
  }
  const run = headerValue(source, "distiller-run");
  if (typeof run !== "string" || !run.startsWith("@")
      || `@${normalizeNorthEntityId(run)}` !== run) {
    throw new Error("tier-1 artifact has an invalid distiller run");
  }
  for (const key of [
    "distiller-wire-prompt-sha256",
    "distiller-prompt-manifest-sha256",
    "distiller-environment-receipt-sha256",
    "distiller-available-skills-sha256",
    "distiller-activated-resources-sha256",
  ]) {
    requireDigest(headerValue(source, key), `tier-1 artifact ${key}`);
  }
  const marker = source.indexOf(ARTIFACT_BODY_MARKER);
  if (marker < 0) throw new Error("tier-1 artifact lacks its body boundary");
  const body = source.slice(marker + ARTIFACT_BODY_MARKER.length).replace(/\n$/, "");
  const bodySha256 = sha256(body);
  if (headerValue(source, "body-sha256") !== bodySha256) {
    throw new Error("tier-1 artifact body digest is contradictory");
  }
  return { artifactSha256: sha256(source), bodySha256 };
}

function completeResult(
  status: Tier1DistillationResult["status"],
  artifactPath: string,
  artifactSha256: string,
  state: ClaimState,
): Tier1DistillationResult {
  return Object.freeze({
    status,
    artifactPath,
    artifactSha256,
    lineageDigest: state.provenance.lineageDigest,
    projectDigest: state.projectDigest,
    streamThread: state.streamThread,
  });
}

async function recoverArtifact(
  state: ClaimState,
  statePath: string,
  artifactPath: string,
  now: Date,
): Promise<Tier1DistillationResult | undefined> {
  if (!state.attempt) return undefined;
  const source = await readOptional(artifactPath);
  if (source === undefined) return undefined;
  if (!state.publication) {
    throw new Error("tier-1 artifact exists without a planned publication digest");
  }
  const validated = validateArtifact(source, state, state.attempt.token);
  if (validated.artifactSha256 !== state.publication.artifactSha256) {
    throw new Error("tier-1 artifact changed after publication planning");
  }
  state.status = "complete";
  state.completion = {
    artifactSha256: validated.artifactSha256,
    completedAt: now.toISOString(),
  };
  delete state.lastFailure;
  await writeClaimState(statePath, state);
  return completeResult("recovered", artifactPath, validated.artifactSha256, state);
}

async function prepareClaim(
  options: Tier1DistillationOptions,
  snapshot: SettledSnapshot,
  inputSha256: string,
): Promise<PreparedClaim> {
  const paths = claimPaths(options, snapshot);
  const runtime = runtimeValues(options.runtime);
  return withFileLease(paths.lockPath, async () => {
    const now = runtime.now();
    if (!Number.isFinite(now.getTime())) throw new Error("tier-1 clock returned an invalid date");
    let state = await loadClaimState(paths.statePath);
    if (state) {
      if (state.projectDigest !== options.project.digest
          || state.streamThread !== options.streamThread
          || state.inputSha256 !== inputSha256
          || state.artifactBasename !== paths.artifactBasename
          || !sameProvenance(state.provenance, snapshot.provenance)) {
        throw new Error("tier-1 claim identity disagrees with the settled snapshot request");
      }
      if (state.status === "complete") {
        const source = await readOptional(paths.artifactPath);
        if (source === undefined || !state.completion) {
          throw new Error("tier-1 completed claim has no durable artifact");
        }
        const digest = sha256(source);
        if (digest !== state.completion.artifactSha256) {
          throw new Error("tier-1 completed artifact changed after publication");
        }
        return { complete: completeResult("already_complete", paths.artifactPath, digest, state) };
      }
      const recovered = await recoverArtifact(state, paths.statePath, paths.artifactPath, now);
      if (recovered) return { complete: recovered };
      if (state.status === "claimed" && state.attempt) {
        const expiresAt = Date.parse(state.attempt.leaseExpiresAt);
        if (runtime.processAlive(state.attempt.pid) && now.getTime() < expiresAt) {
          throw new Error(`tier-1 lineage ${snapshot.provenance.lineageDigest} is already claimed`);
        }
      }
    } else {
      if (await readOptional(paths.artifactPath) !== undefined) {
        throw new Error("tier-1 artifact exists without a durable project claim");
      }
      state = {
        version: CLAIM_VERSION,
        status: "available",
        projectDigest: options.project.digest,
        streamThread: options.streamThread,
        inputSha256,
        provenance: snapshot.provenance,
        artifactBasename: paths.artifactBasename,
      };
    }
    const token = runtime.uuid();
    const attempt: ClaimAttempt = Object.freeze({
      token,
      pid: runtime.pid,
      agentId: `tier1-${snapshot.provenance.lineageDigest.slice(0, 12)}-${token}`,
      claimedAt: now.toISOString(),
      leaseExpiresAt: new Date(now.getTime() + runtime.claimLeaseMs).toISOString(),
    });
    state.status = "claimed";
    state.attempt = attempt;
    delete state.publication;
    delete state.completion;
    delete state.lastFailure;
    await writeClaimState(paths.statePath, state);
    return {
      claim: {
        statePath: paths.statePath,
        lockPath: paths.lockPath,
        artifactPath: paths.artifactPath,
        snapshot,
        state,
        attempt,
      },
    };
  });
}

function normalizedBody(bodyInput: string): string {
  if (typeof bodyInput !== "string") throw new Error("tier-1 model body must be text");
  const body = bodyInput.trim();
  if (!body || body.includes("\0")) throw new Error("tier-1 model returned an empty or unsafe body");
  if (Buffer.byteLength(body, "utf8") > MAX_TIER1_BODY_BYTES) {
    throw new Error("tier-1 model body exceeds the 128 KiB publication bound");
  }
  if (body.startsWith("---")) {
    throw new Error("tier-1 model body must not supply provenance frontmatter");
  }
  return body;
}

function requireModelResult(result: Tier1ModelResult): Tier1ModelResult {
  if (!result || typeof result !== "object" || !result.execution) {
    throw new Error("tier-1 model returned no execution provenance");
  }
  if (result.execution.provider !== "anthropic") {
    throw new Error("tier-1 model did not use the sealed data-only provider");
  }
  normalizeNorthEntityId(result.execution.wireRunId);
  requireDigest(result.execution.wirePromptSha256, "tier-1 wire prompt digest");
  requireDigest(result.execution.promptManifestSha256, "tier-1 prompt manifest digest");
  requireDigest(result.execution.environmentReceiptSha256, "tier-1 environment receipt digest");
  requireDigest(result.execution.availableSkillCatalogSha256, "tier-1 available-skill closure digest");
  requireDigest(result.execution.activatedResourceClosureSha256, "tier-1 activated-resource closure digest");
  normalizedBody(result.body);
  return result;
}

function tier1Input(
  snapshot: SettledSnapshot,
  project: Tier1ProjectIdentity,
  streamThread: string,
): { input: string; inputSha256: string } {
  const input = JSON.stringify({
    schema: "north:tier1-distillation-input:v1",
    projectSha256: project.digest,
    streamThread: `@${streamThread}`,
    provenance: snapshot.provenance,
    transcript: snapshot.transcript,
  });
  return { input, inputSha256: sha256(input) };
}

function artifactContent(
  claim: ClaimedSnapshot,
  inputSha256: string,
  resultInput: Tier1ModelResult,
  project: Tier1ProjectIdentity,
): string {
  const result = requireModelResult(resultInput);
  const body = normalizedBody(result.body);
  const provenance = claim.snapshot.provenance;
  const fields: ReadonlyArray<readonly [string, string | number]> = [
    ["north-tier1", "north:tier1:v1"],
    ["project-sha256", project.digest],
    ["raw-source", provenance.rawSource],
    ["raw-lineage", provenance.lineage],
    ["raw-lineage-sha256", provenance.lineageDigest],
    ["raw-snapshot-sha256", provenance.rawSnapshotSha256],
    ["raw-snapshot-bytes", provenance.cursorBytes],
    ["cursor", provenance.cursor],
    ["cursor-record-sha256", provenance.cursorRecordSha256],
    ["cursor-snapshot-token", provenance.cursorSnapshotToken],
    ["source-receipt", provenance.sourceReceipt],
    ["source-receipt-sha256", provenance.sourceReceiptSha256],
    ["source-provider", provenance.provider],
    ["source-namespace", provenance.namespace],
    ["source-session", provenance.sessionId],
    ["source-agent", `@${provenance.sourceAgent}`],
    ["source-run", `@${provenance.sourceRun}`],
    ["source-thread", `@${provenance.sourceThread}`],
    ["stream-thread", `@${claim.state.streamThread}`],
    ["settled-at", provenance.settledAt],
    ["claim-token", claim.attempt.token],
    ["distiller-tier", "economy"],
    ["distiller-provider", result.execution.provider],
    ["distiller-run", `@${result.execution.wireRunId}`],
    ["distiller-prompt-template-sha256", TIER1_DISTILLER_PROMPT_SHA256],
    ["distiller-routing-sha256", TIER1_DISTILLER_ROUTING_SHA256],
    ["distiller-wire-prompt-sha256", result.execution.wirePromptSha256],
    ["distiller-prompt-manifest-sha256", result.execution.promptManifestSha256],
    ["distiller-environment-receipt-sha256", result.execution.environmentReceiptSha256],
    ["distiller-available-skills-sha256", result.execution.availableSkillCatalogSha256],
    ["distiller-activated-resources-sha256", result.execution.activatedResourceClosureSha256],
    ["distiller-input-sha256", inputSha256],
    ["body-sha256", sha256(body)],
  ];
  const frontmatter = fields.map(([key, value]) => `${key}: ${JSON.stringify(value)}`).join("\n");
  return `---\n${frontmatter}\n---\n# Tier 1 — ${project.slug}\n\n${ARTIFACT_BODY_MARKER}${body}\n`;
}

async function publishClaim(
  options: Tier1DistillationOptions,
  claim: ClaimedSnapshot,
  inputSha256: string,
  result: Tier1ModelResult,
): Promise<Tier1DistillationResult> {
  return withFileLease(claim.lockPath, async () => {
    const state = await loadClaimState(claim.statePath);
    if (!state || !state.attempt || state.attempt.token !== claim.attempt.token
        || state.status !== "claimed") {
      throw new Error("tier-1 model output lost claim ownership and is stale");
    }
    const content = artifactContent(claim, inputSha256, result, options.project);
    const artifactSha256 = sha256(content);
    state.publication = { artifactSha256 };
    await writeClaimState(claim.statePath, state);
    const created = await atomicWriteOnce(claim.artifactPath, content);
    if (!created) {
      const existing = await readOptional(claim.artifactPath);
      if (existing === undefined) throw new Error("tier-1 artifact publication raced with removal");
      validateArtifact(existing, state, claim.attempt.token);
      if (existing !== content) {
        throw new Error("tier-1 artifact already contains divergent output for this claim");
      }
    }
    await options.runtime?.afterArtifactWrite?.(claim.artifactPath);
    state.status = "complete";
    state.completion = {
      artifactSha256,
      completedAt: runtimeValues(options.runtime).now().toISOString(),
    };
    delete state.lastFailure;
    await writeClaimState(claim.statePath, state);
    return completeResult(created ? "created" : "recovered", claim.artifactPath, artifactSha256, state);
  });
}

async function releaseClaim(
  options: Tier1DistillationOptions,
  claim: ClaimedSnapshot,
  error: unknown,
): Promise<void> {
  await withFileLease(claim.lockPath, async () => {
    const state = await loadClaimState(claim.statePath);
    if (!state?.attempt || state.attempt.token !== claim.attempt.token || state.status !== "claimed") return;
    state.status = "available";
    const detail = error instanceof Error ? `${error.name}: ${error.message}` : String(error);
    state.lastFailure = {
      at: runtimeValues(options.runtime).now().toISOString(),
      detail: detail.replace(/\s+/g, " ").slice(0, 512),
    };
    await writeClaimState(claim.statePath, state);
  });
}

async function verifyStreamThread(
  threadInput: string,
  runtime: Tier1DistillerRuntime | undefined,
): Promise<string> {
  const thread = normalizeNorthEntityId(threadInput);
  if (runtime?.verifyStreamThread) {
    await runtime.verifyStreamThread(thread);
    return thread;
  }
  const facts = getThreadFacts(thread);
  if (!facts.some(({ predicate, value }) => predicate === "kind" && value === "thread")) {
    throw new Error(`tier-1 stream thread @${thread} is not a graph thread`);
  }
  return thread;
}

export async function distillOneTier1(
  optionsInput: Tier1DistillationOptions,
): Promise<Tier1DistillationResult> {
  optionsInput.signal?.throwIfAborted();
  if (optionsInput.project.digest !== tier1ProjectIdentity(optionsInput.project.repository).digest
      || optionsInput.project.slug !== projectSlug(optionsInput.project.repository)) {
    throw new Error("tier-1 project identity is contradictory");
  }
  const streamThread = await verifyStreamThread(optionsInput.streamThread, optionsInput.runtime);
  const options = { ...optionsInput, streamThread };
  const snapshots = await settledSnapshots(options.rawDirectory, options.project);
  const candidates = options.lineageDigest
    ? snapshots.filter(({ provenance }) => provenance.lineageDigest === options.lineageDigest)
    : snapshots;
  if (candidates.length === 0) {
    throw new Error(options.lineageDigest
      ? `no settled mirrored session matches project and lineage ${options.lineageDigest}`
      : "no settled mirrored session matches the project repository identity");
  }
  if (candidates.length !== 1) {
    throw new Error(
      `project has ${candidates.length} settled mirrored sessions; select one exact --lineage digest`,
    );
  }
  const snapshot = candidates[0]!;
  if (snapshot.provenance.sourceThread === streamThread) {
    throw new Error("tier-1 stream thread must be distinct from the source work thread");
  }
  const input = tier1Input(snapshot, options.project, streamThread);
  const prepared = await prepareClaim(options, snapshot, input.inputSha256);
  if (prepared.complete) return prepared.complete;
  const claim = prepared.claim!;
  try {
    options.signal?.throwIfAborted();
    const result = await options.runner({
      ...input,
      project: options.project,
      provenance: snapshot.provenance,
      streamThread,
      attemptAgentId: claim.attempt.agentId,
      ...(options.signal === undefined ? {} : { signal: options.signal }),
    });
    options.signal?.throwIfAborted();
    return await publishClaim(options, claim, input.inputSha256, result);
  } catch (error) {
    try { await releaseClaim(options, claim, error); }
    catch (releaseError) {
      throw new AggregateError([error, releaseError], "tier-1 distillation and claim release failed");
    }
    throw error;
  }
}

export function defaultTier1StateDirectory(): string {
  const stateHome = process.env.XDG_STATE_HOME
    ? path.resolve(process.env.XDG_STATE_HOME)
    : path.resolve(os.homedir(), ".local/state");
  return path.join(stateHome, "north", "tier1-distiller");
}
