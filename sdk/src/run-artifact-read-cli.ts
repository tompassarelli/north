import { readRunArtifactPage } from "./run-artifacts";
import { parseStrictJson } from "./strict-json";

const MAX_REQUEST_BYTES = 4_096;

interface ArtifactReadRequest {
  artifactId: string;
  offset?: number;
  limit?: number;
  snapshot?: string;
}

interface ArtifactReadFailure {
  error: string;
  expected?: string;
  actual?: string;
}

function request(value: unknown): ArtifactReadRequest {
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw new Error("artifact read request must be an object");
  const source = value as Record<string, unknown>;
  const allowed = new Set(["artifactId", "offset", "limit", "snapshot"]);
  const unknown = Object.keys(source).filter((key) => !allowed.has(key)).sort();
  if (unknown.length) throw new Error(`unknown artifact read arguments: ${unknown.join(", ")}`);
  if (typeof source.artifactId !== "string" || !source.artifactId.trim())
    throw new Error("artifactId must be a non-empty string");
  return {
    artifactId: source.artifactId,
    ...(source.offset === undefined ? {} : { offset: source.offset as number }),
    ...(source.limit === undefined ? {} : { limit: source.limit as number }),
    ...(source.snapshot === undefined ? {} : { snapshot: source.snapshot as string }),
  };
}

function safeDigest(value: unknown): string | undefined {
  return typeof value === "string" && /^[a-f0-9]{64}$/.test(value) ? value : undefined;
}

export function artifactReadFailure(error: unknown): ArtifactReadFailure {
  if (error && typeof error === "object") {
    const source = error as Record<string, unknown>;
    const code = typeof source.code === "string" && /^[a-z][a-z0-9_]*$/.test(source.code)
      ? source.code
      : undefined;
    if (code) {
      const expected = safeDigest(source.expected);
      const actual = safeDigest(source.actual);
      return {
        error: code,
        ...(expected === undefined ? {} : { expected }),
        ...(actual === undefined ? {} : { actual }),
      };
    }
  }
  return { error: "artifact_read_failed" };
}

export async function runArtifactReadCli(
  input: string,
  environment: NodeJS.ProcessEnv = process.env,
): Promise<unknown> {
  const directory = environment.NORTH_RUN_ARTIFACT_DIR;
  if (!directory) {
    const unavailable = new Error("run artifact directory unavailable") as Error & { code: string };
    unavailable.code = "artifact_directory_unavailable";
    throw unavailable;
  }
  const parsed = request(parseStrictJson(input, "artifact read request", {
    maxBytes: MAX_REQUEST_BYTES,
    maxDepth: 2,
    maxNodes: 16,
  }));
  return readRunArtifactPage(directory, parsed);
}

if (import.meta.main) {
  runArtifactReadCli(await Bun.stdin.text()).then(
    (page) => process.stdout.write(`${JSON.stringify(page)}\n`),
    (error) => {
      process.stdout.write(`${JSON.stringify(artifactReadFailure(error))}\n`);
      process.exitCode = 1;
    },
  );
}
