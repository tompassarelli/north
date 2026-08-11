import { readdirSync, statSync } from "node:fs";
import { open } from "node:fs/promises";
import { join } from "node:path";

export const DEFAULT_SESSION_ACTIVITY_HOURS = 24;
export const LIVE_SESSION_MTIME_MS = 120_000;
// Rollouts can be hundreds of megabytes, while each usage record is cumulative.
const REVERSE_READ_CHUNK_BYTES = 64 * 1024;

export interface OpenAISessionActivity {
  hours: number;
  sessions: number;
  live: number;
  outputTokens: number;
  lastActivityAt?: Date;
}

export interface ReadOpenAISessionActivityOptions {
  accountRoot: string;
  hours?: number;
  now?: Date;
}

function rolloutFiles(directory: string): string[] {
  let entries;
  try {
    entries = readdirSync(directory, { withFileTypes: true });
  } catch {
    return [];
  }
  const files: string[] = [];
  for (const entry of entries) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) files.push(...rolloutFiles(path));
    else if (entry.isFile() && entry.name.startsWith("rollout-") && entry.name.endsWith(".jsonl"))
      files.push(path);
  }
  return files;
}

function totalTokenUsage(line: string): Record<string, unknown> | undefined {
  if (!line.includes("total_token_usage")) return undefined;
  try {
    const parsed = JSON.parse(line) as {
      payload?: { info?: { total_token_usage?: unknown } };
    };
    const usage = parsed.payload?.info?.total_token_usage;
    return usage !== null && typeof usage === "object"
      ? usage as Record<string, unknown>
      : undefined;
  } catch {
    return undefined;
  }
}

async function lastOutputTokens(path: string): Promise<number> {
  let file: Awaited<ReturnType<typeof open>> | undefined;
  try {
    file = await open(path, "r");
    let position = (await file.stat()).size;
    let suffix = Buffer.alloc(0);
    while (position > 0) {
      const length = Math.min(position, REVERSE_READ_CHUNK_BYTES);
      position -= length;
      const chunk = Buffer.allocUnsafe(length);
      const { bytesRead } = await file.read(chunk, 0, length, position);
      if (bytesRead <= 0) break;
      const contents = suffix.length
        ? Buffer.concat([chunk.subarray(0, bytesRead), suffix])
        : chunk.subarray(0, bytesRead);
      let lineEnd = contents.length;
      while (lineEnd > 0) {
        const newline = contents.lastIndexOf(0x0a, lineEnd - 1);
        if (newline < 0) break;
        const usage = totalTokenUsage(contents.subarray(newline + 1, lineEnd).toString("utf8"));
        const outputTokens = usage?.output_tokens;
        if (typeof outputTokens === "number" && Number.isFinite(outputTokens)) return outputTokens;
        lineEnd = newline;
      }
      suffix = Buffer.from(contents.subarray(0, lineEnd));
    }
    const outputTokens = totalTokenUsage(suffix.toString("utf8"))?.output_tokens;
    return typeof outputTokens === "number" && Number.isFinite(outputTokens)
      ? outputTokens
      : 0;
  } catch {
    return 0;
  } finally {
    await file?.close().catch(() => {});
  }
}

export async function readOpenAISessionActivity(
  options: ReadOpenAISessionActivityOptions,
): Promise<OpenAISessionActivity> {
  const hours = options.hours ?? DEFAULT_SESSION_ACTIVITY_HOURS;
  const now = options.now ?? new Date();
  const cutoffMs = hours * 60 * 60 * 1_000;
  const files = rolloutFiles(join(options.accountRoot, "sessions"));
  const recent: Array<{ path: string; mtimeMs: number }> = [];
  for (const path of files) {
    try {
      const mtimeMs = statSync(path).mtimeMs;
      if (now.getTime() - mtimeMs < cutoffMs) recent.push({ path, mtimeMs });
    } catch {
      // A provider may rotate a rollout between directory enumeration and stat.
    }
  }

  let outputTokens = 0;
  for (const file of recent) outputTokens += await lastOutputTokens(file.path);
  const lastActivityMs = recent.reduce(
    (latest, file) => Math.max(latest, file.mtimeMs),
    Number.NEGATIVE_INFINITY,
  );
  return {
    hours,
    sessions: recent.length,
    live: recent.filter(({ mtimeMs }) => now.getTime() - mtimeMs < LIVE_SESSION_MTIME_MS).length,
    outputTokens,
    ...(Number.isFinite(lastActivityMs) ? { lastActivityAt: new Date(lastActivityMs) } : {}),
  };
}
