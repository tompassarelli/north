import { createHash } from "node:crypto";

export type IssueRef = `${string}#${number}`;
export interface ProjectConfig {
  name?: string;
  priority: number;
  target?: string;
  repos?: readonly string[];
}
export interface OrgNode {
  id: string;
  role: "lead" | "sub-lead" | "worker";
  domain: string;
  state: "active" | "finished";
  brief: string;
  status_file: string;
  parent?: string;
  provider?: string;
  depth?: number;
  died?: string;
}
export interface GitHubIssue {
  ref: IssueRef;
  title: string;
  body: string;
  url: string;
  state: "OPEN" | "CLOSED";
  stateReason?: "COMPLETED" | "NOT_PLANNED" | null;
  labels: readonly string[];
  parent?: IssueRef | null;
  subissues?: readonly IssueRef[];
}
export interface DesiredProject {
  key: string;
  name: string;
  description: string;
  targetDate: string | null;
  status: "in_progress" | "planned";
}
export interface DesiredIssue {
  key: string;
  ref?: IssueRef;
  leadId?: string;
  assigneeNodeId?: string;
  boxId?: string;
  title: string;
  description: string;
  projectKey: string | null;
  parentKey?: string;
  priority: "high" | "medium";
  status: "todo" | "in_progress" | "in_review" | "blocked" | "done" | "cancelled";
}
export interface DesiredState {
  projects: DesiredProject[];
  issues: DesiredIssue[];
  trackedRefs: IssueRef[];
}
export interface CoreInput {
  projects: Readonly<Record<string, ProjectConfig>>;
  nodes: readonly OrgNode[];
  briefs: Readonly<Record<string, string>>;
  github: readonly GitHubIssue[];
  previous?: DesiredState;
  finishedLeadIds?: readonly string[];
  additionalRefs?: readonly IssueRef[];
}
export interface ParsedBrief {
  goal: string;
  refs: IssueRef[];
  boxes: Record<string, IssueRef>;
}

export function issueRefs(text: string): IssueRef[] {
  return [...new Set([...text.matchAll(/\b([A-Za-z0-9_.-]+)#[1-9][0-9]*\b/g)].map(m => m[0] as IssueRef))];
}

export function parseBrief(text: string): ParsedBrief {
  const result: ParsedBrief = { goal: "", refs: [], boxes: {} };
  for (const line of text.split(/\r?\n/)) {
    const goal = /^\s*Goal:\s*(.*)$/.exec(line);
    const queue = /^\s*\d+[.)]\s+(.*)$/.exec(line);
    if (!goal && !queue) continue;
    const content = (goal ?? queue)![1];
    if (goal) result.goal = content;
    const refs = issueRefs(content);
    result.refs.push(...refs);
    const box = queue && /^([A-Za-z][A-Za-z0-9_.-]*)\s*[,:(]/.exec(content);
    if (box && refs[0]) result.boxes[box[1]] = refs[0];
  }
  result.refs = [...new Set(result.refs)];
  return result;
}

const projectKey = (name: string) => `north:project:${name}`;
const leadKey = (id: string) => `north:lead:${id}`;

export function desiredState(input: CoreInput): DesiredState {
  const snapshots = new Map(input.github.map(issue => [issue.ref, issue]));
  const active = input.nodes.filter(n => n.state === "active" && n.role !== "worker");
  const briefs = new Map(input.nodes.map(n => [n.id, parseBrief(input.briefs[n.brief] ?? "")]));
  const activeRefs = new Set(active.flatMap(n => briefs.get(n.id)!.refs));
  const repoProjects = new Map<string, string>();
  for (const [name, config] of Object.entries(input.projects)) {
    for (const repo of config.repos ?? [name]) repoProjects.set(repo, name);
  }
  const selected = new Set<IssueRef>([...activeRefs, ...(input.additionalRefs ?? []), ...(input.previous?.trackedRefs ?? [])]);
  const related = new Map<IssueRef, Set<IssueRef>>();
  const parents = new Map<IssueRef, IssueRef>();
  const connect = (parent: IssueRef, child: IssueRef) => {
    for (const [from, to] of [[parent, child], [child, parent]] as const) {
      if (!related.has(from)) related.set(from, new Set());
      related.get(from)!.add(to);
    }
  };
  for (const issue of input.github) {
    if (repoProjects.has(issue.ref.split("#")[0]) && issue.state === "OPEN" && issue.labels.includes("priority:now")) selected.add(issue.ref);
    if (issue.parent) {
      parents.set(issue.ref, issue.parent);
      connect(issue.parent, issue.ref);
    }
    for (const child of issue.subissues ?? []) {
      if (!parents.has(child)) parents.set(child, issue.ref);
      connect(issue.ref, child);
    }
  }
  const pending = [...selected];
  for (let i = 0; i < pending.length; i++) {
    for (const ref of related.get(pending[i]) ?? []) {
      if (selected.has(ref)) continue;
      selected.add(ref);
      pending.push(ref);
    }
  }
  const projects: DesiredProject[] = Object.entries(input.projects)
    .sort(([a, x], [b, y]) => x.priority - y.priority || a.localeCompare(b))
    .map(([name, config]) => ({
      key: projectKey(name), name: config.name ?? name, description: `Priority ${config.priority}`,
      targetDate: config.target ?? null, status: active.some(n => n.domain === name) ? "in_progress" : "planned",
    }));
  const issues: DesiredIssue[] = [];
  const trackedRefs: IssueRef[] = [];
  for (const ref of [...selected].sort()) {
    const issue = snapshots.get(ref);
    if (!issue) {
      const previous = input.previous?.issues.find(i => i.ref === ref);
      if (previous) issues.push(previous);
      trackedRefs.push(ref);
      continue;
    }
    const domain = repoProjects.get(ref.split("#")[0]);
    if (!domain) continue;
    const parent = parents.get(ref);
    issues.push({ key: `gh:${ref}`, ref, title: `${ref} ${issue.title}`, description: `${issue.url}\n\n${issue.body.slice(0, 1000)}`,
      projectKey: projectKey(domain), ...(parent ? { parentKey: `gh:${parent}` } : {}),
      priority: activeRefs.has(ref) ? "high" : "medium",
      status: issue.state === "CLOSED" ? issue.stateReason === "NOT_PLANNED" ? "cancelled" : "done"
        : activeRefs.has(ref) ? "in_progress" : "todo" });
    if (issue.state === "OPEN") trackedRefs.push(ref);
  }
  const leads = new Map<string, DesiredIssue>(input.previous?.issues.filter(i => i.leadId).map(i => [i.leadId!, { ...i, status: "done" as const }]) ?? []);
  for (const node of input.nodes.filter(n => n.role !== "worker")) {
    const brief = briefs.get(node.id)!;
    leads.set(node.id, { key: leadKey(node.id), leadId: node.id, title: `${node.id}: ${brief.goal || node.id}`,
      description: brief.goal, projectKey: input.projects[node.domain] ? projectKey(node.domain) : null,
      priority: "high", status: node.state === "finished" || input.finishedLeadIds?.includes(node.id) ? "done" : "in_progress" });
  }
  issues.push(...[...leads.values()].sort((a, b) => a.key.localeCompare(b.key)));
  return { projects, issues, trackedRefs };
}

export function sourceFieldDiff<T extends object>(desired: T, lastPushed: Partial<T>): Partial<T> {
  const patch: Partial<T> = {};
  for (const key of Object.keys(desired) as (keyof T)[]) {
    if (JSON.stringify(desired[key]) !== JSON.stringify(lastPushed[key])) patch[key] = desired[key];
  }
  return patch;
}

export type StatusKind = "LANDED" | "DONE" | "FINISHED" | "QUESTION";
export interface StatusComment {
  kind: StatusKind;
  targetKey: string;
  body: string;
  clientRequestId: string;
  authorType: "system";
  presentation: { kind: "system_notice"; tone: "success" | "info" | "warning" };
  metadata: { key: string; value: string }[];
  finishedLead: boolean;
}

export function commentId(nodeId: string, line: string): string {
  const namespace = Buffer.from("6ba7b8119dad11d180b400c04fd430c8", "hex");
  const hash = createHash("sha1").update(namespace).update(JSON.stringify([nodeId, line])).digest().subarray(0, 16);
  hash[6] = (hash[6] & 0x0f) | 0x50;
  hash[8] = (hash[8] & 0x3f) | 0x80;
  const hex = hash.toString("hex");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

export function parseStatusLine(nodeId: string, line: string, trackedRefs: readonly IssueRef[], briefText: string): StatusComment | null {
  const match = /^(LANDED|DONE|FINISHED|QUESTION)\b/.exec(line);
  if (!match) return null;
  const kind = match[1] as StatusKind;
  const tracked = new Set(trackedRefs);
  let ref = issueRefs(line).find(r => tracked.has(r));
  if (!ref) {
    const boxes = parseBrief(briefText).boxes;
    for (const token of line.match(/[A-Za-z][A-Za-z0-9_.-]*/g) ?? []) {
      if (boxes[token] && tracked.has(boxes[token])) { ref = boxes[token]; break; }
    }
  }
  const sha = /\b[0-9a-f]{7,40}\b/i.exec(line)?.[0];
  return { kind, targetKey: ref ? `gh:${ref}` : leadKey(nodeId), body: line,
    clientRequestId: commentId(nodeId, line), authorType: "system",
    presentation: { kind: "system_notice", tone: kind === "LANDED" ? "success" : kind === "QUESTION" ? "warning" : "info" },
    metadata: [{ key: "lead", value: nodeId }, { key: "kind", value: kind }, ...(sha ? [{ key: "sha", value: sha }] : [])],
    finishedLead: kind === "FINISHED" };
}
