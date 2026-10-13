import { titleOnlyRef, type GitHubIssue, type IssueRef, type ProjectConfig } from "./paperclip-core";

export function labelledSnapshot(value: unknown, projects: Record<string, ProjectConfig>, sources: Record<string, string[]>, bodyRefs: readonly IssueRef[]): GitHubIssue[] {
  if (!Array.isArray(value)) throw new Error("Invalid labelled GitHub snapshot");
  const repos = new Set(Object.entries(projects).flatMap(([name, project]) => project.repos ?? [name]));
  const ref = (value: unknown): value is IssueRef => typeof value === "string" && /^[A-Za-z0-9_.-]+#[1-9][0-9]*$/.test(value);
  const seen = new Set<string>();
  return value.map(issue => {
    if (!issue || !ref(issue.ref) || seen.has(issue.ref) || !repos.has(issue.ref.split("#")[0]) || !Array.isArray(issue.labels) || issue.labels.some((label: unknown) => typeof label !== "string") || !(sources[issue.ref.split("#")[0]] ?? []).some(label => issue.labels.includes(label))) throw new Error("GitHub snapshot outside labelled source scope");
    if ([issue.title, issue.body, issue.url].some(field => typeof field !== "string") || !["OPEN", "CLOSED"].includes(issue.state) || issue.stateReason != null && !["COMPLETED", "NOT_PLANNED"].includes(issue.stateReason) || issue.parent != null && !ref(issue.parent) || !Array.isArray(issue.subissues) || issue.subissues.some((child: unknown) => !ref(child))) throw new Error("Invalid labelled GitHub issue");
    if (titleOnlyRef(issue.ref) && !bodyRefs.includes(issue.ref) && issue.body !== "") throw new Error("Restricted body in labelled GitHub snapshot");
    seen.add(issue.ref);
    return { ref: issue.ref, title: issue.title, body: issue.body, url: issue.url, state: issue.state, stateReason: issue.stateReason ?? null, labels: issue.labels, parent: issue.parent ?? null, subissues: issue.subissues };
  });
}
