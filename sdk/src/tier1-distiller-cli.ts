import * as path from "node:path";

import {
  defaultTier1StateDirectory,
  distillOneTier1,
  tier1ProjectIdentityFromRoot,
} from "./tier1-distiller";
import { runTier1Model } from "./tier1-distiller-model";

const REPO = path.resolve(import.meta.dir, "../..");
const USAGE = `usage: north stream-distill --project-root DIR --stream-thread ID [--lineage SHA256]

Claims exactly one settled managed-session mirror and publishes its write-once
Tier 1 Markdown artifact. When a project has multiple unselected mirrors,
--lineage is required.`;

interface CliOptions {
  projectRoot: string;
  streamThread: string;
  lineageDigest?: string;
  rawDirectory: string;
  distillationsDirectory: string;
  stateDirectory: string;
}

function defaultRawDirectory(): string {
  if (process.env.NORTH_STREAM_RAW_DIR) return path.resolve(process.env.NORTH_STREAM_RAW_DIR);
  if (process.env.NORTH_PACKAGE_MODE === "nix-store") {
    const stateHome = process.env.XDG_STATE_HOME
      ? path.resolve(process.env.XDG_STATE_HOME)
      : path.resolve(process.env.HOME ?? "", ".local/state");
    return path.join(stateHome, "north", "streams", "raw");
  }
  return path.join(REPO, "streams", "raw");
}

function parseArgs(argv: string[]): CliOptions | undefined {
  if (argv.length === 1 && (argv[0] === "--help" || argv[0] === "-h")) return undefined;
  const values: Partial<CliOptions> = {};
  for (let index = 0; index < argv.length; index += 1) {
    const flag = argv[index];
    if (!["--project-root", "--stream-thread", "--lineage", "--raw-dir",
      "--distillations-dir", "--state-dir"].includes(flag!)) {
      throw new Error(`unknown tier-1 distiller option: ${flag}\n${USAGE}`);
    }
    const value = argv[++index];
    if (!value) throw new Error(`${flag} requires a value\n${USAGE}`);
    if (flag === "--project-root") values.projectRoot = value;
    else if (flag === "--stream-thread") values.streamThread = value;
    else if (flag === "--lineage") values.lineageDigest = value;
    else if (flag === "--raw-dir") values.rawDirectory = path.resolve(value);
    else if (flag === "--distillations-dir") values.distillationsDirectory = path.resolve(value);
    else values.stateDirectory = path.resolve(value);
  }
  if (!values.projectRoot || !values.streamThread) throw new Error(USAGE);
  return {
    projectRoot: path.resolve(values.projectRoot),
    streamThread: values.streamThread,
    ...(values.lineageDigest === undefined ? {} : { lineageDigest: values.lineageDigest }),
    rawDirectory: values.rawDirectory ?? defaultRawDirectory(),
    distillationsDirectory: values.distillationsDirectory
      ?? path.resolve(process.env.NORTH_STREAM_DISTILLATIONS_DIR
        ?? path.join(REPO, "streams", "distillations")),
    stateDirectory: values.stateDirectory ?? defaultTier1StateDirectory(),
  };
}

export async function runTier1DistillerCli(argv: string[]): Promise<number> {
  let options: CliOptions | undefined;
  try { options = parseArgs(argv); }
  catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    return 2;
  }
  if (!options) {
    console.log(USAGE);
    return 0;
  }
  const abort = new AbortController();
  const interrupt = (): void => abort.abort(new Error("tier-1 distillation interrupted"));
  process.once("SIGINT", interrupt);
  process.once("SIGTERM", interrupt);
  try {
    const project = await tier1ProjectIdentityFromRoot(options.projectRoot);
    const result = await distillOneTier1({
      rawDirectory: options.rawDirectory,
      distillationsDirectory: options.distillationsDirectory,
      stateDirectory: options.stateDirectory,
      project,
      streamThread: options.streamThread,
      ...(options.lineageDigest === undefined ? {} : { lineageDigest: options.lineageDigest }),
      runner: runTier1Model,
      signal: abort.signal,
    });
    console.log(`${result.status}: ${result.artifactPath}`);
    return 0;
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    return 1;
  } finally {
    process.removeListener("SIGINT", interrupt);
    process.removeListener("SIGTERM", interrupt);
  }
}

if (import.meta.main) process.exitCode = await runTier1DistillerCli(process.argv.slice(2));
