import { issueRefs, type CoreInput, type DesiredIssue, type DesiredState, type IssueRef, type OrgNode } from "./paperclip-core";

type Box = { id: string; title: string; ref?: IssueRef; status?: DesiredIssue["status"] };
export type RecordAgent = { key: string; nodeId: string; parentId?: string; name: string; title: string; status: "active" | "paused" | "error" };
const states: Record<string, DesiredIssue["status"]> = {
  WORKING: "in_progress", ACTIVE: "in_progress", REVIEW: "in_review", "READY-TO-LAND": "in_review",
  LANDED: "done", DONE: "done", "NOT DONE": "blocked", BLOCKED: "blocked", QUESTION: "blocked",
};
const identity = (text: string) => text.trim().replace(/^\*\*|\*\*$/g, "").replace(/[^A-Za-z0-9_.-]+/g, "-").replace(/^-|-$/g, "").toLowerCase() || "current";
const itemToken = (text: string) => text.replace(/^\d{4}-\d\d-\d\dT\S+\s+|^\d\d:\d\d\s+/g, "").match(/^([A-Za-z0-9][A-Za-z0-9_.-]*)/)?.[1];

export function workstreamBrief(text: string): { title: string; goal: string; boxes: Box[] } {
  const title = /^\s*#\s+(.+)$/m.exec(text)?.[1]?.trim() ?? "";
  const goal = /^\s*Goal:\s*(.*)$/m.exec(text)?.[1]?.trim() ?? "";
  const boxes: Box[] = [];
  let queue = false;
  let numbered = true;
  for (const line of text.split(/\r?\n/)) {
    if (/^(?:#{1,6}\s+)?(?:Ordered queue|Queue|Boxes|Current work|Workstreams)\b/i.test(line)) { queue = true; numbered = true; }
    else if (/^#{2,6}\s/.test(line)) { queue = false; numbered = /^#{2,6}\s+Goal\b/i.test(line); }
    else if (/^(?:Done when|Supervision|Rules|Not required)\b/i.test(line)) { queue = false; numbered = false; }
    const content = (numbered ? /^\s*\d+[.)]\s+(.+)/.exec(line)?.[1] : undefined) ?? (queue ? /^\s*[-*]\s+(.+)/.exec(line)?.[1] : undefined);
    if (!content) continue;
    const ref = issueRefs(content)[0];
    const token = itemToken(content);
    const id = token && (/^[A-Z]+\d/.test(token) || /^\w+[,:(]/.test(content)) ? identity(token) : identity(ref ?? content);
    boxes.push({ id, title: content, ...(ref ? { ref } : {}) });
  }
  return { title, goal, boxes };
}

export function statusBoxes(text: string, briefBoxes: readonly Box[]): { boxes: Box[]; unknown: number } {
  const boxes = new Map(briefBoxes.map(box => [box.id, { ...box }]));
  let unknown = 0;
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (!line || line.startsWith("#") || /^FINISHED\b/.test(line)) continue;
    const direct = /^(WORKING|ACTIVE|REVIEW|READY-TO-LAND|LANDED|DONE|Not done|blocked|QUESTION)\b\s*:?[ \t]*(.*)$/i.exec(line);
    const embedded = /^(?:BOX|RECOVERY)\s+(.+?)\s+(WORKING|ACTIVE|REVIEW|READY-TO-LAND|LANDED|DONE|blocked|QUESTION)(?=\b|[0-9a-f]{7})\s*:?[ \t]*(.*)$/i.exec(line);
    if (!direct && !embedded) { unknown++; continue; }
    const state = states[(direct?.[1] ?? embedded![2]).toUpperCase()];
    const content = direct?.[2] ?? `${embedded![1]} ${embedded![3]}`;
    if (/^when\b/i.test(content)) { unknown++; continue; }
    const refs = issueRefs(content);
    const token = itemToken(content);
    const named = token ? boxes.get(identity(token)) : undefined;
    if (refs.length) {
      const ref = named?.ref ?? refs[0];
      const box = named ?? [...boxes.values()].find(b => b.ref === ref) ?? { id: identity(ref), title: content, ref };
      boxes.set(box.id, { ...box, status: state });
    } else if (named) boxes.set(named.id, { ...named, status: state });
    else if (token && !/^\d/.test(token)) {
      const id = identity(token);
      boxes.set(id, { id, title: content, status: state });
    } else if (boxes.size === 1) {
      const box = [...boxes.values()][0];
      boxes.set(box.id, { ...box, status: state });
    } else unknown++;
  }
  return { boxes: [...boxes.values()], unknown };
}

export function recordAgents(nodes: readonly OrgNode[], previous: readonly RecordAgent[] = [], finishedIds: readonly string[] = []): RecordAgent[] {
  const result = new Map(previous.map(agent => [agent.nodeId, { ...agent, status: "paused" as const }]));
  for (const node of nodes.filter(n => n.role !== "worker")) result.set(node.id, {
    key: `north:agent:${node.id}`, nodeId: node.id, parentId: node.parent, name: node.id,
    title: `${node.role} · ${node.provider ?? "unknown"}`,
    status: node.state === "finished" || finishedIds.includes(node.id) ? "paused" : node.died ? "error" : "active",
  });
  return [...result.values()].sort((a, b) => a.nodeId.localeCompare(b.nodeId));
}

export function topologyState(base: DesiredState, input: CoreInput & { statuses: Readonly<Record<string, string>> }): { desired: DesiredState; unknown: number } {
  const desired: DesiredState = { ...base, issues: base.issues.map(issue => ({ ...issue })) };
  const active = input.nodes.filter(n => n.role !== "worker" && n.state === "active" && !input.finishedLeadIds?.includes(n.id));
  const byId = new Map(input.nodes.map(n => [n.id, n]));
  const depth = (node: OrgNode) => {
    let result = 0;
    const seen = new Set<string>();
    let current: OrgNode | undefined = node;
    while (current && !seen.has(current.id)) { seen.add(current.id); result++; current = byId.get(current.parent ?? ""); }
    return result;
  };
  active.sort((a, b) => depth(b) - depth(a) || a.id.localeCompare(b.id));
  const owned = new Set<IssueRef>();
  let unknown = 0;
  for (const node of active) {
    const brief = workstreamBrief(input.briefs[node.brief] ?? "");
    const previousChildren = input.previous?.issues.filter(i => i.parentKey === `north:lead:${node.id}`) ?? [];
    const parsed = statusBoxes(input.statuses[node.status_file] ?? "", brief.boxes);
    unknown += parsed.unknown;
    const boxes = parsed.boxes;
    for (const previous of previousChildren) {
      if (!boxes.some(box => previous.ref ? box.ref === previous.ref : box.id === previous.boxId) && previous.boxId) {
        boxes.push({ id: previous.boxId, title: previous.title, ...(previous.ref ? { ref: previous.ref } : {}) });
      }
    }
    if (!boxes.length) boxes.push({ id: "current", title: brief.goal || brief.title || node.id });
    const children: DesiredIssue[] = [];
    for (const box of boxes) {
      if (box.ref && owned.has(box.ref)) continue;
      const key = box.ref ? `gh:${box.ref}` : `north:box:${node.id}:${box.id}`;
      const current = desired.issues.find(issue => issue.key === key);
      const previous = input.previous?.issues.find(issue => issue.key === key);
      if (box.ref && !current && !previous) { unknown++; continue; }
      if (box.ref) owned.add(box.ref);
      const issue: DesiredIssue = {
        ...(current ?? previous ?? { key, title: box.title, description: box.title, priority: "high" as const, projectKey: input.projects[node.domain] ? `north:project:${node.domain}` : null }),
        boxId: box.id, parentKey: `north:lead:${node.id}`, assigneeNodeId: node.id,
        status: box.status ?? previous?.status ?? current?.status ?? "in_progress",
      };
      if (current) Object.assign(current, issue); else desired.issues.push(issue);
      children.push(issue);
    }
    if (!children.length) {
      const key = `north:box:${node.id}:current`;
      const previous = input.previous?.issues.find(i => i.key === key);
      const issue: DesiredIssue = { key, boxId: "current", parentKey: `north:lead:${node.id}`, assigneeNodeId: node.id,
        title: brief.goal || brief.title || node.id, description: brief.goal, priority: "high",
        projectKey: input.projects[node.domain] ? `north:project:${node.domain}` : null, status: previous?.status ?? "in_progress" };
      desired.issues.push(issue); children.push(issue);
    }
    const parent = desired.issues.find(issue => issue.key === `north:lead:${node.id}`)!;
    Object.assign(parent, { title: `[${children.filter(i => i.status === "done").length}/${children.length}] ${brief.title || brief.goal || node.id}`, assigneeNodeId: node.id });
  }
  for (const previous of input.previous?.issues ?? []) {
    if (previous.boxId && !desired.issues.some(i => i.key === previous.key)) desired.issues.push({ ...previous });
  }
  return { desired, unknown };
}
