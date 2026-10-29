import { closeSync, existsSync, mkdirSync, openSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join } from "node:path";

export const ENROLLED = ["north", "smashcraft", "wisp", "padbridge", "smashcraft-client"];
const LAUNCH_GRACE_MS = 60 * 60 * 1000;
const PLACEMENT_PICK = /launch with --account ([\w-]+)/;

export type TickEnv = {
  codeRoot: string;
  stateRoot: string;
  handoffs: string;
  codeHealth: string[];
  north: string;
  template: string;
  dryRun: boolean;
  now: () => Date;
};

export type Outcome = { repo: string; result: "quiet" | "held" | "deferred" | "started" | "missing" | "error"; detail: string };

const shellQuote = (word: string) => (/^[\w@%+=:,./-]+$/.test(word) ? word : `'${word.replaceAll("'", `'\\''`)}'`);

export const reviewName = (repo: string, at: Date) => `code-health-review-${repo}-${at.toISOString().slice(0, 16).replace(/[-:]/g, "").replace("T", "-")}`;

function fleetNames(north: string, cwd: string): Set<string> | null {
  try {
    const result = run([north, "fleet", "list", "--json"], cwd);
    if (result.code !== 0) return null;
    return new Set(JSON.parse(result.stdout).map((session: { name?: string }) => session.name).filter(Boolean));
  } catch {
    return null;
  }
}

const lockedName = (lock: string): string => { try { return String(JSON.parse(readFileSync(lock, "utf8")).name); } catch { return ""; } };
function acquire(lock: string, name: string, env: TickEnv): boolean {
  mkdirSync(dirname(lock), { recursive: true });
  for (let attempt = 0; attempt < 2; attempt++) {
    try {
      const fd = openSync(lock, "wx");
      writeFileSync(fd, JSON.stringify({ name, at: env.now().toISOString(), pid: process.pid }) + "\n");
      closeSync(fd);
      return true;
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== "EEXIST") throw error;
    }
    const names = fleetNames(env.north, env.codeRoot);
    if (!names || names.has(lockedName(lock)) || env.now().getTime() - statSync(lock).mtimeMs < LAUNCH_GRACE_MS) return false;
    rmSync(lock, { force: true });
  }
  return false;
}

function run(argv: string[], cwd: string) {
  const proc = Bun.spawnSync(argv, { cwd, stdout: "pipe", stderr: "pipe" });
  return { code: proc.exitCode, stdout: proc.stdout.toString(), stderr: proc.stderr.toString() };
}

function fill(template: string, values: Record<string, string>): string {
  return template.replace(/\{\{(\w+)\}\}/g, (match, key) => values[key] ?? match);
}

export function tickRepo(repo: string, env: TickEnv, print: (line: string) => void): Outcome {
  const checkout = join(env.codeRoot, repo, "main");
  if (!existsSync(checkout)) return { repo, result: "missing", detail: `no checkout at ${checkout}` };
  const name = reviewName(repo, env.now());
  const state = join(env.stateRoot, repo), lock = join(state, "review.lock");
  if (!acquire(lock, name, env)) return { repo, result: "held", detail: `review lock ${lock}` };
  const release = (detail: string, result: Outcome["result"] = "error"): Outcome => {
    rmSync(lock, { force: true });
    return { repo, result, detail };
  };

  const fetched = run(["git", "-C", checkout, "fetch", "-q", "origin"], checkout);
  if (fetched.code !== 0) return release(`fetch origin failed: ${fetched.stderr.trim()}`);
  const signal = run([...env.codeHealth, "signal", "--repo", checkout, "--rev", "origin/main"], checkout);
  if (signal.code === 1) return release("quiet", "quiet");
  const reasons = signal.stdout.split("\n").filter((line) => line.startsWith("FIRE"));
  if (signal.code !== 0 || reasons.length === 0) return release(`signal exit ${signal.code}: ${signal.stderr.trim()}`);

  const report = run([...env.codeHealth, "report", "--repo", checkout, "--rev", "origin/main"], checkout);
  if (report.code !== 0) return release(`report exit ${report.code}: ${report.stderr.trim()}`);
  const reportPath = join(state, "review-report.txt"); writeFileSync(reportPath, report.stdout);

  mkdirSync(env.handoffs, { recursive: true });
  const brief = join(env.handoffs, `${name}-brief.md`);
  writeFileSync(brief, fill(readFileSync(env.template, "utf8"), {
    repo, checkout, lock, report: reportPath,
    reasons: reasons.map((line) => `- ${line}`).join("\n"),
    list: join(env.handoffs, `${name}-list.md`),
    date: env.now().toISOString().slice(0, 10),
  }));

  const started = launch((account) => [env.north, "fleet", "launch", "--provider", "claude", "--name", name,
    "--system-prompt", brief, "--cwd", checkout, "--account", account, `Review the code-health report for ${repo} and land the bounded maintenance changes it identifies.`], checkout, env, print);
  if (started.code !== 0 && /launch from another host/.test(started.stderr + started.stdout)) return release(`host saturated, the next tick retries: ${started.stderr.trim().split("\n").at(-1)}`, "deferred");
  if (started.code !== 0) return release(`fleet launch exit ${started.code}: ${(started.stderr || started.stdout).trim()}`);
  return { repo, result: "started", detail: `${env.dryRun ? "dry-run " : ""}${name} (${reasons.join("; ")})` };
}

function launch(launchOn: (account: string) => string[], checkout: string, env: TickEnv, print: (line: string) => void) {
  const account = run([env.north, "fleet", "accounts"], checkout).stdout.split("\n").find((line) => line.startsWith("claude\t"))?.split("\t")[1] ?? "a";
  if (env.dryRun) {
    print(`dry-run: ${launchOn(account).map(shellQuote).join(" ")}`);
    return { code: 0, stdout: "", stderr: "" };
  }
  const started = run(launchOn(account), checkout);
  const picked = PLACEMENT_PICK.exec(started.stderr + started.stdout)?.[1];
  return started.code !== 0 && picked && picked !== account ? run(launchOn(picked), checkout) : started;
}

export function defaultEnv(argv: string[]): { env: TickEnv; repos: string[] } {
  const home = homedir();
  const reposArg = argv.find((arg) => arg.startsWith("--repos="));
  return {
    repos: reposArg ? reposArg.slice("--repos=".length).split(",").filter(Boolean) : ENROLLED,
    env: {
      codeRoot: process.env.CODE_HEALTH_CODE_ROOT ?? join(home, "code"),
      stateRoot: process.env.CODE_HEALTH_STATE ?? join(home, ".local/state/code-health"),
      handoffs: process.env.AGENTS_HANDOFFS ?? join(home, ".local/state/agents/handoffs"),
      codeHealth: (process.env.CODE_HEALTH_BIN ?? "code-health").split(" "),
      north: process.env.CODE_HEALTH_NORTH_BIN ?? "north",
      template: join(import.meta.dir, "review-brief.md"),
      dryRun: argv.includes("--dry-run"),
      now: () => new Date(),
    },
  };
}

export function tick(argv: string[], print: (line: string) => void = console.log): number {
  if (argv.includes("-h") || argv.includes("--help")) {
    print("usage: code-health-tick [--dry-run] [--repos=a,b]");
    print(`For each enrolled repo (${ENROLLED.join(", ")}): take the review lock, run \`code-health signal\`, and on FIRE`);
    print("save the report, fill harness/code-health/review-brief.md and launch one fleet session.");
    print("--dry-run prints the launch command instead of running it.");
    return 0;
  }
  const { env, repos } = defaultEnv(argv);
  let failed = false;
  for (const repo of repos) {
    let outcome: Outcome;
    try {
      outcome = tickRepo(repo, env, print);
    } catch (error) {
      outcome = { repo, result: "error", detail: String(error) };
    }
    failed ||= outcome.result === "error";
    print(`${repo} ${outcome.result}: ${outcome.detail}`);
  }
  return failed ? 1 : 0;
}

if (import.meta.main) process.exit(tick(process.argv.slice(2)));
