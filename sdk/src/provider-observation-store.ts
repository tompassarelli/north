import {
  chmod, mkdir, open, readFile, rename, rm, stat, unlink, writeFile,
} from "node:fs/promises";
import { dirname } from "node:path";
import { randomUUID } from "node:crypto";
import {
  DEFAULT_PROVIDER_OBSERVATIONS_PATH,
  parseProviderUsageObservations,
} from "./resource-policy";
import type { ProviderUsageObservation, ProviderUsageObservationStore } from "./providers/types";

const LOCK_STALE_MS = 30_000;
const LOCK_WAIT_MS = 20;
const LOCK_ATTEMPTS = 250;

function observationKey({ targetId, provider }: ProviderUsageObservation): string {
  return `${targetId}\u0000${provider}`;
}

/** Keep one newest observation for each target/provider pair. */
export function mergeProviderUsageObservations(
  existing: ProviderUsageObservationStore | undefined,
  incoming: ProviderUsageObservation | ProviderUsageObservation[],
): ProviderUsageObservationStore {
  const newest = new Map<string, ProviderUsageObservation>();
  for (const observation of [...(existing?.observations ?? []), ...([incoming].flat())]) {
    const key = observationKey(observation);
    const previous = newest.get(key);
    if (!previous || Date.parse(observation.observedAt) >= Date.parse(previous.observedAt))
      newest.set(key, observation);
  }
  return {
    version: 1,
    observations: [...newest.values()].sort((left, right) =>
      left.targetId.localeCompare(right.targetId) || left.provider.localeCompare(right.provider)),
  };
}

async function delay(ms: number): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, ms));
}

async function acquireLock(path: string): Promise<Awaited<ReturnType<typeof open>>> {
  for (let attempt = 0; attempt < LOCK_ATTEMPTS; attempt++) {
    try {
      return await open(path, "wx", 0o600);
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== "EEXIST") throw error;
      try {
        const info = await stat(path);
        if (Date.now() - info.mtimeMs > LOCK_STALE_MS) {
          await unlink(path);
          continue;
        }
      } catch (inspectError) {
        if ((inspectError as NodeJS.ErrnoException).code === "ENOENT") continue;
        throw inspectError;
      }
      await delay(LOCK_WAIT_MS);
    }
  }
  throw new Error(`timed out waiting for North provider observation store lock ${path}`);
}

async function readExisting(path: string): Promise<ProviderUsageObservationStore | undefined> {
  try {
    const raw = await readFile(path, "utf8");
    try { return parseProviderUsageObservations(JSON.parse(raw), path); }
    catch (error) {
      if (error instanceof SyntaxError)
        throw new Error(`invalid North provider usage observations at ${path}: could not parse JSON: ${error.message}`);
      throw error;
    }
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return undefined;
    throw error;
  }
}

/**
 * Serialize read/merge/replace through a filesystem lock so independent
 * provider collectors cannot erase one another's observations.
 */
export async function writeProviderUsageObservations(
  incoming: ProviderUsageObservation | ProviderUsageObservation[],
  path = process.env.NORTH_PROVIDER_OBSERVATIONS ?? DEFAULT_PROVIDER_OBSERVATIONS_PATH,
): Promise<ProviderUsageObservationStore> {
  const validatedIncoming = parseProviderUsageObservations({ version: 1, observations: [incoming].flat() }, "<incoming observations>");
  const directory = dirname(path);
  await mkdir(directory, { recursive: true, mode: 0o700 });
  const lockPath = `${path}.lock`;
  const lock = await acquireLock(lockPath);
  const temporary = `${path}.${process.pid}.${randomUUID()}.tmp`;
  try {
    const merged = mergeProviderUsageObservations(await readExisting(path), validatedIncoming.observations);
    await writeFile(temporary, `${JSON.stringify(merged, null, 2)}\n`, { mode: 0o600, flag: "wx" });
    await chmod(temporary, 0o600);
    await rename(temporary, path);
    await chmod(path, 0o600);
    return merged;
  } finally {
    await rm(temporary, { force: true });
    await lock.close();
    await rm(lockPath, { force: true });
  }
}
