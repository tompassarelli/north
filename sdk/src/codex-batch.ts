import { createWriteStream, readFileSync } from "node:fs";
import { spawn } from "node:child_process";
import { mkdir } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { accountEnvironment, requireProviderAccount } from "./accounts";

const USAGE = `usage: bun run sdk/src/codex-batch.ts --brief <file|-> [options]

Runs one Codex turn and writes its stdout to a result file.

Options:
  --brief <file|->  Full self-contained prompt; - reads stdin.
  --cwd <dir>       Codex working directory (default: current directory).
  --out <file>      Result file (default: codex-batch-<timestamp>.out in --cwd).
  --account <name>  North OpenAI account id; selects its isolated CODEX_HOME.
  --help            Show this help.`;

type Options = { brief: string; cwd: string; out?: string; account?: string };

function parse(argv: string[]): Options | undefined {
  const values: Partial<Options> = {};
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--help") return undefined;
    if (arg !== "--brief" && arg !== "--cwd" && arg !== "--out" && arg !== "--account") throw new Error(USAGE);
    const value = argv[++index];
    if (!value) throw new Error(`${arg} requires a value\n${USAGE}`);
    values[arg.slice(2) as keyof Options] = value;
  }
  if (!values.brief) throw new Error(USAGE);
  return { brief: values.brief, cwd: resolve(values.cwd ?? process.cwd()), out: values.out, account: values.account };
}

function briefFrom(path: string): string {
  return path === "-" ? readFileSync(0, "utf8") : readFileSync(path, "utf8");
}

async function run(options: Options): Promise<{ exitCode: number; outFile: string }> {
  const outFile = resolve(options.cwd, options.out ?? `codex-batch-${Date.now()}.out`);
  await mkdir(dirname(outFile), { recursive: true });
  const env = options.account
    ? accountEnvironment(requireProviderAccount(options.account))
    : process.env;
  const output = createWriteStream(outFile, { flags: "w" });
  const child = spawn("codex", ["exec", "--skip-git-repo-check", briefFrom(options.brief)], {
    cwd: options.cwd, env, stdio: ["ignore", "pipe", "inherit"],
  });
  child.stdout.pipe(output);
  const exitCode = await new Promise<number>((done) => {
    child.once("error", (error) => { console.error(error.message); done(1); });
    child.once("close", (code) => done(code ?? 1));
  });
  await new Promise<void>((done, fail) => output.end((error?: Error | null) => error ? fail(error) : done()));
  return { exitCode, outFile };
}

export async function main(argv = process.argv.slice(2)): Promise<number> {
  const started = Date.now();
  let options: Options | undefined;
  let exitCode = 1;
  let outFile: string | null = null;
  try {
    options = parse(argv);
    if (!options) { console.log(USAGE); return 0; }
    ({ exitCode, outFile } = await run(options));
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
  }
  console.log(JSON.stringify({ account: options?.account ?? "ambient", exitCode, outFile, durationMs: Date.now() - started }));
  return exitCode;
}

if (import.meta.main) process.exit(await main());
