import { appendFileSync } from "node:fs";

export interface Step { name: string; conclusion: string | null; number: number }
export interface Job { id: number; name: string; conclusion: string | null; steps: Step[] }
export interface Commit { sha: string; parents: string[]; message: string; files: string[] }
export interface Situation {
  attempt: number; conclusion: string | null; original: Job[]; confirmed: Job[]; previous?: { sha: string; conclusion: string | null };
  commits: Commit[]; onMain: boolean; head: string; tip: string; successor?: { status: string; conclusion: string | null };
}
export type Decision = { kind: "none" | "report" | "confirm" | "request"; reason: string; step?: string; job?: number; shas?: string[]; parent?: string };

export function decide(s: Situation): Decision {
  if (s.conclusion === "cancelled" || s.conclusion === null) return { kind: "none", reason: "The run was cancelled or never concluded; nothing to revert" };
  const job = [...s.original].sort((a, b) => a.id - b.id).find(j => j.conclusion === "failure" && j.steps.some(t => t.conclusion === "failure"));
  const step = job?.steps.find(t => t.conclusion === "failure");
  if (!job || !step) return { kind: s.conclusion === "failure" ? "report" : "none", reason: "No failing test step was recorded" };
  const name = `${job.name}: ${step.name}`;
  const report = (reason: string): Decision => ({ kind: "report", reason, step: name });
  if (!s.onMain) return report("The failing commit is no longer on main; report only");
  if (s.tip !== s.head) return report(s.successor && s.successor.status !== "completed" ? `Main moved to ${s.tip}, whose run is still ${s.successor.status}: a fix-forward may land, so revert nothing` : s.successor?.conclusion === "success" ? `Main moved to ${s.tip}, whose run passed: a fix-forward landed, so revert nothing` : `Main moved past the failing commit to ${s.tip}; a later landing may already fix it, so never revert on this stale s.successor`);
  if (s.attempt === 1) return { kind: "confirm", reason: "Confirm this failed job once; retain the original failure", step: name, job: job.id };
  if (s.attempt !== 2) return report("Confirmation budget exhausted; never rerun again");
  const confirmed = s.confirmed.find(j => j.name === job.name);
  if (s.conclusion !== "failure" || confirmed?.conclusion !== "failure" || !confirmed.steps.some(t => t.name === step.name && t.conclusion === "failure")) return report("The original step did not fail again; no confirmed culprit");
  if (/^(Set up job|Complete job|Post |Install |Checkout |Cache |Activate |Read setup |Toolchain |Run actions\/|Run oven-sh\/)/.test(step.name) || /planted fault$/.test(step.name)) return report(/planted fault$/.test(step.name) ? "A canary failed: a check stopped checking or its planted fault no longer applies; report, never revert" : "A setup step failed; report infrastructure failure");
  if (s.previous?.conclusion !== "success") return report("The previous main run of this workflow was not green");
  if (!s.commits.length) return report("No landed commits after the previous run");
  let parent = s.previous.sha;
  for (const commit of [...s.commits].reverse()) {
    if (commit.parents.length !== 1 || commit.parents[0] !== parent) return report("The green run is not the landing's parent");
    if (/^Revert\b|^Sheriff-Reverts:/m.test(commit.message)) return report("Never revert a revert");
    if (commit.files.some(p => p.startsWith(".github/workflows/"))) return report("Never revert workflow changes");
    parent = commit.sha;
  }
  if (parent !== s.head) return report("The landed range does not end at the failing commit");
  return { kind: "request", reason: `Same step failed twice after a green parent; the lander reverts the one of ${s.commits.length} landings whose removal turns the failing cases green`, step: name, job: job.id, shas: s.commits.map(c => c.sha), parent: s.previous.sha };
}

const FAILURE = /---- .* stdout ----|test .* \.\.\. FAILED|land check: \S+\.(sh|py) (exited|was killed)|\bFAIL:? /;
export const revertRequest = (head: string, verdict: Decision, failed: string[], url: string): string => `Main red at ${head}\n\n${verdict.step}\n${failed.join("\n")}\n\nGreen-parent: ${verdict.parent}\nMain-red-run: ${url}`;

function run(command: string, args: string[]): string {
  const result = Bun.spawnSync([command, ...args], { stdout: "pipe", stderr: "pipe", env: process.env });
  if (result.exitCode !== 0) throw new Error(`${command} ${args[0]} exited ${result.exitCode}: ${result.stderr.toString().trim()}`);
  return result.stdout.toString();
}
const git = (...args: string[]) => run("git", args).trim();
const gh = (...args: string[]) => run("gh", args);
function api<T>(path: string): T { return JSON.parse(gh("api", path)) as T; }
interface Run { id: number; workflow_id: number; head_sha: string; head_branch: string; event: string; run_attempt: number; status: string; conclusion: string | null; html_url: string }
interface Issue { number: number; title: string; body: string }
const repo = () => {
  const value = process.env.GH_REPO;
  if (!value || !/^[\w.-]+\/[\w.-]+$/.test(value)) throw new Error("GH_REPO must name owner/repository");
  return `repos/${value}`;
};
function jobs(id: number, attempt: number): Job[] {
  const pages = JSON.parse(gh("api", "--paginate", "--slurp", `${repo()}/actions/runs/${id}/attempts/${attempt}/jobs?per_page=100`)) as { jobs: Job[] }[];
  return pages.flatMap(p => p.jobs);
}
function issue(): Issue | undefined {
  const issues = JSON.parse(gh("issue", "list", "--state", "open", "--search", '"main is red" in:title', "--json", "number,title,body", "--limit", "100")) as Issue[];
  return issues.find(i => i.title === "main is red");
}
export const issueBody = (existing = "Main CI failures and confirmed culprit decisions.\n\n## Done when\n- [ ] The newest Harness and Cargo runs on main are green on a commit after the newest culprit range below: `gh run list --branch main --workflow Harness --limit 1` and the same for Cargo both print success; close this issue with that run's link.", marker: string, body: string): string => `${existing}\n\n${marker}\n${body}`;
function report(existing: Issue | undefined, marker: string, body: string): void {
  const args = existing ? ["issue", "edit", String(existing.number)] : ["issue", "create", "--title", "main is red"];
  gh(...args, "--body", issueBody(existing?.body, marker, body));
  if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, `${body}\n`);
  console.log(body);
}

export function sheriff(id: number): void {
  const ci = api<Run>(`${repo()}/actions/runs/${id}`);
  if (ci.head_branch !== "main" || ci.event !== "push") return;
  const original = jobs(id, 1);
  if (ci.conclusion === "success" && ci.run_attempt === 1) return;
  const existing = issue();
  const marker = `<!-- main-red:${id}:${ci.run_attempt} -->`;
  if (existing?.body.includes(marker)) return;
  git("fetch", "-q", "origin", "main");
  const tip = git("rev-parse", "origin/main"), onMain = Bun.spawnSync(["git", "merge-base", "--is-ancestor", ci.head_sha, tip]).exitCode === 0, successor = api<{ workflow_runs: Run[] }>(`${repo()}/actions/workflows/${ci.workflow_id}/runs?branch=main&event=push&head_sha=${tip}&per_page=1`).workflow_runs[0];
  const history = api<{ workflow_runs: Run[] }>(`${repo()}/actions/workflows/${ci.workflow_id}/runs?branch=main&event=push&status=completed&per_page=100`).workflow_runs;
  const prior = history.filter(r => r.id < id).sort((a, b) => b.id - a.id)[0];
  let commits: Commit[] = [];
  if (prior) {
    const ancestor = Bun.spawnSync(["git", "merge-base", "--is-ancestor", prior.head_sha, ci.head_sha]);
    if (ancestor.exitCode === 0) {
      commits = git("log", "--first-parent", "-z", "--format=%H%n%P%n%B", `${prior.head_sha}..${ci.head_sha}`).split("\0").filter(x => x.trim()).map(entry => {
        const [sha, parents, ...message] = entry.trimStart().split("\n");
        return { sha: sha!, parents: parents!.split(" ").filter(Boolean), message: message.join("\n"), files: git("diff-tree", "--root", "--no-commit-id", "--name-only", "-r", sha!).split("\n") };
      });
    }
  }
  const verdict = decide({ attempt: ci.run_attempt, conclusion: ci.conclusion, original, confirmed: ci.run_attempt === 2 ? jobs(id, 2) : [], previous: prior && { sha: prior.head_sha, conclusion: prior.conclusion }, commits, onMain, head: ci.head_sha, tip, successor });
  const detail = `Culprit range: ${commits.map(c => c.sha).join(", ") || ci.head_sha}\nFailing step: ${verdict.step ?? "unknown"}\n[CI run](${ci.html_url})\n${verdict.reason}`;
  if (verdict.kind === "confirm") {
    report(existing, marker, `${detail}\nRequested one confirmation of job ${verdict.job}; a green confirmation is reported, never treated as a repaired run.`);
    gh("api", "--method", "POST", `${repo()}/actions/jobs/${verdict.job}/rerun`);
  } else if (verdict.kind === "request") {
    const failed = gh("api", `${repo()}/actions/jobs/${verdict.job}/logs`).split("\n").filter(line => FAILURE.test(line)).slice(0, 200);
    const request = git("commit-tree", `${ci.head_sha}^{tree}`, "-p", ci.head_sha, "-m", revertRequest(ci.head_sha, verdict, failed, ci.html_url));
    git("push", "origin", `${request}:refs/heads/claude/land-revert-${ci.head_sha}`);
    report(existing, marker, `${detail}\nAsked the lander to revert the culprit through the landing queue (claude/land-revert-${ci.head_sha}).`);
  } else if (verdict.kind === "report") report(existing, marker, detail);
}

if (import.meta.main) {
  const id = Number(process.argv[2]);
  if (!Number.isSafeInteger(id) || id <= 0) throw new Error("usage: bun main-red.ts RUN_ID");
  sheriff(id);
}
