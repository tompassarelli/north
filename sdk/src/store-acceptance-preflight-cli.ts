import {
  NORTH_STORE_ACCEPTANCE_JOURNEYS,
  NorthStoreAcceptanceError,
  type NorthStoreAcceptanceJourney,
  type NorthStoreAcceptanceJourneyResult,
  runNorthStoreAcceptancePreflight,
} from "./store-acceptance-preflight";

const USAGE = "usage: cat EVIDENCE.json | bun run src/store-acceptance-preflight-cli.ts --release-id SHA256 --socket ENDPOINT --cutover-at ISO-8601";

type JsonObject = Record<string, unknown>;

function object(value: unknown, label: string): JsonObject {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error(`${label} must be an object`);
  return value as JsonObject;
}

function onlyKeys(value: JsonObject, keys: readonly string[], label: string): void {
  const actual = Object.keys(value).sort();
  const expected = [...keys].sort();
  if (actual.length !== expected.length || actual.some((key, index) => key !== expected[index]))
    throw new Error(`${label} must contain exactly: ${expected.join(", ")}`);
}

function string(value: unknown, label: string): string {
  if (typeof value !== "string") throw new Error(`${label} must be a string`);
  return value;
}

function stringArray(value: unknown, label: string): string[] {
  if (!Array.isArray(value) || value.some((entry) => typeof entry !== "string"))
    throw new Error(`${label} must be a string array`);
  return [...value];
}

function journey(value: unknown, label: string): NorthStoreAcceptanceJourneyResult {
  const input = object(value, label);
  const name = string(input.journey, `${label}.journey`);
  if (!NORTH_STORE_ACCEPTANCE_JOURNEYS.includes(name as NorthStoreAcceptanceJourney))
    throw new Error(`${label}.journey is not a required North acceptance journey`);
  const census = name === "account-census";
  onlyKeys(input, census
    ? ["journey", "exitCode", "stdout", "stderr", "evidence", "censusAccountIds"]
    : ["journey", "exitCode", "stdout", "stderr", "evidence"], label);
  if (typeof input.exitCode !== "number" || !Number.isSafeInteger(input.exitCode))
    throw new Error(`${label}.exitCode must be an integer`);
  const evidence = object(input.evidence, `${label}.evidence`);
  onlyKeys(evidence, ["releaseId", "socket", "persistenceConfirmed", "routingEligible", "observedAt", "evidenceMode"], `${label}.evidence`);
  const evidenceMode = string(evidence.evidenceMode, `${label}.evidence.evidenceMode`);
  if (evidenceMode !== "authoritative" && evidenceMode !== "live-only")
    throw new Error(`${label}.evidence.evidenceMode must be authoritative or live-only`);
  if (typeof evidence.persistenceConfirmed !== "boolean" || typeof evidence.routingEligible !== "boolean")
    throw new Error(`${label}.evidence persistenceConfirmed and routingEligible must be booleans`);
  return {
    journey: name as NorthStoreAcceptanceJourney,
    exitCode: input.exitCode,
    stdout: string(input.stdout, `${label}.stdout`),
    stderr: string(input.stderr, `${label}.stderr`),
    evidence: {
      releaseId: string(evidence.releaseId, `${label}.evidence.releaseId`),
      socket: string(evidence.socket, `${label}.evidence.socket`),
      persistenceConfirmed: evidence.persistenceConfirmed,
      routingEligible: evidence.routingEligible,
      observedAt: string(evidence.observedAt, `${label}.evidence.observedAt`),
      evidenceMode,
    },
    ...(census ? { censusAccountIds: stringArray(input.censusAccountIds, `${label}.censusAccountIds`) } : {}),
  };
}

function argument(argv: readonly string[], name: string): string | undefined {
  const index = argv.indexOf(name);
  return index < 0 ? undefined : argv[index + 1];
}

function argumentsFor(argv: readonly string[]): { releaseId: string; socket: string; cutoverAt: string } {
  const releaseId = argument(argv, "--release-id");
  const socket = argument(argv, "--socket");
  const cutoverAt = argument(argv, "--cutover-at");
  if (!releaseId || !socket || !cutoverAt || argv.length !== 6
      || new Set(argv.filter((value) => value.startsWith("--"))).size !== 3) throw new Error(USAGE);
  return { releaseId, socket, cutoverAt };
}

async function main(argv: readonly string[]): Promise<void> {
  const options = argumentsFor(argv);
  const input = object(JSON.parse(await Bun.stdin.text()), "acceptance evidence");
  onlyKeys(input, ["expectedAccountIds", "journeys"], "acceptance evidence");
  const expectedAccountIds = stringArray(input.expectedAccountIds, "acceptance evidence.expectedAccountIds");
  if (!Array.isArray(input.journeys)) throw new Error("acceptance evidence.journeys must be an array");
  const parsed = input.journeys.map((entry, index) => journey(entry, `acceptance evidence.journeys[${index}]`));
  if (parsed.length !== NORTH_STORE_ACCEPTANCE_JOURNEYS.length
      || new Set(parsed.map(({ journey }) => journey)).size !== parsed.length)
    throw new Error("acceptance evidence must contain each required journey exactly once");
  const byJourney = new Map(parsed.map((entry) => [entry.journey, entry]));
  const receipt = await runNorthStoreAcceptancePreflight({ ...options, expectedAccountIds }, {
    async runJourney(required) {
      const result = byJourney.get(required);
      if (!result) throw new Error(`missing ${required} result`);
      return result;
    },
  });
  process.stdout.write(`${JSON.stringify(receipt)}\n`);
}

if (import.meta.main) {
  main(process.argv.slice(2)).catch((error) => {
    if (error instanceof NorthStoreAcceptanceError) {
      process.stdout.write(`${JSON.stringify({
        version: "north:store-acceptance-preflight:v1",
        accepted: false,
        failures: error.failures,
        journeys: error.journeys,
      })}\n`);
    }
    const detail = error instanceof NorthStoreAcceptanceError ? error.failures.join("; ")
      : error instanceof Error ? error.message : String(error);
    console.error(`north Store acceptance preflight: ${detail}`);
    process.exitCode = 1;
  });
}
