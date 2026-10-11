export type Node = {
  id: string;
  role: "proxy" | "lead" | "sub-lead" | "worker";
  domain: string;
  parent: string;
  provider: "claude" | "codex";
  session: string;
  state: "active" | "finished";
  pid?: number;
  died?: string;
  status_file?: string;
};

export type Room = {
  node: string;
  host: string;
  room_id: string;
  state: "active" | "finished" | "died";
  tom: "join" | "leave";
};

export type Action =
  | { kind: "open"; node: Node }
  | { kind: "close"; node: string }
  | { kind: "died"; node: string; text: string }
  | { kind: "revived"; node: string };

export function decide(nodes: Node[], rooms: Room[], live: Record<string, boolean>): Action[] {
  const actions: Action[] = [];
  const byNode = new Map(rooms.map((room) => [room.node, room]));
  const active = new Set<string>();
  for (const node of nodes) {
    if (node.role === "worker" || node.state === "finished") continue;
    active.add(node.id);
    const room = byNode.get(node.id);
    const opening = !room || room.state === "finished" || room.tom !== "join";
    if (opening) actions.push({ kind: "open", node });
    const alive = !node.died && live[node.id] !== false;
    if (!alive && room?.state !== "died") {
      actions.push({ kind: "died", node: node.id, text: `session died; restart: agents lead restart ${node.id}` });
    } else if (alive && room?.state === "died") {
      actions.push({ kind: "revived", node: node.id });
    }
  }
  for (const room of rooms) {
    if (!active.has(room.node) && (room.state !== "finished" || room.tom !== "leave")) {
      actions.push({ kind: "close", node: room.node });
    }
  }
  return actions;
}

export const localpart = (name: string) => name.toLowerCase().replace(/[^a-z0-9._=\/-]/g, "_");
export const displayname = (node: string) => node === "proxy" ? "Plato" : node;
export const roomTopic = (node: Node, host: string) => [node.role, node.domain, node.provider, host, node.parent].join(" · ");

export const domains = ["smashcraft", "galileo", "wisp", "nexus", "north", "tooling", "mail"] as const;
export const bootstrapNodes: Node[] = [
  { id: "proxy", role: "proxy", domain: "all", parent: "", provider: "claude", session: "", state: "active" },
  { id: "alerts", role: "lead", domain: "all", parent: "", provider: "claude", session: "", state: "active" },
];
export const parentSpace = (node: Node) => node.role !== "proxy" && domains.some((domain) => domain === node.domain) ? node.domain : "socrates";
export type SpaceChild = { parent: string; room_id: string; content: { via?: string[]; suggested?: boolean; order?: string } };
export type SpaceParent = { parent: string; room_id: string; content: { canonical?: boolean; via?: string[] } };

function spaceLinks(nodes: Node[], rooms: Room[], spaces: Record<string, string>, server: string): SpaceChild[] {
  const desired: SpaceChild[] = domains.map((domain, index) => ({
    parent: "socrates", room_id: spaces[domain]!, content: { via: [server], suggested: true, order: `0${index + 2}-${domain}` },
  }));
  const byNode = new Map(nodes.filter((node) => node.role !== "worker" && node.state === "active").map((node) => [node.id, node]));
  for (const room of rooms) {
    const node = byNode.get(room.node);
    if (!node || room.state === "finished") continue;
    const parent = parentSpace(node);
    desired.push({ parent, room_id: room.room_id, content: { via: [server], suggested: true,
      order: node.id === "proxy" ? "00-plato" : node.id === "alerts" ? "01-alerts" : parent === "socrates" ? `99-${localpart(node.id)}` : localpart(node.id) } });
  }
  return desired;
}

export function decideSpaces(nodes: Node[], rooms: Room[], spaces: Record<string, string>, children: SpaceChild[], server: string): SpaceChild[] {
  const desired = spaceLinks(nodes, rooms, spaces, server);
  const managed = new Set([...rooms.map((room) => room.room_id), ...domains.map((domain) => spaces[domain])]);
  const actions: SpaceChild[] = children.filter((child) => managed.has(child.room_id) && child.content.via?.length &&
    !desired.some((link) => link.parent === child.parent && link.room_id === child.room_id)).map((child) => ({ ...child, content: {} }));
  for (const link of desired) {
    const old = children.find((child) => child.parent === link.parent && child.room_id === link.room_id)?.content;
    if (old?.order !== link.content.order || old?.suggested !== true || old?.via?.length !== 1 || old.via[0] !== server) actions.push(link);
  }
  return actions;
}

export function decideParents(nodes: Node[], rooms: Room[], spaces: Record<string, string>, parents: SpaceParent[], server: string): SpaceParent[] {
  const desired: SpaceParent[] = spaceLinks(nodes, rooms, spaces, server).map((link) => ({
    parent: spaces[link.parent]!, room_id: link.room_id, content: { canonical: true, via: [server] },
  }));
  const managed = new Set([...rooms.map((room) => room.room_id), ...domains.map((domain) => spaces[domain])]);
  const actions: SpaceParent[] = parents.filter((parent) => managed.has(parent.room_id) && Object.keys(parent.content).length &&
    !desired.some((link) => link.parent === parent.parent && link.room_id === parent.room_id)).map((parent) => ({ ...parent, content: {} }));
  for (const link of desired) {
    const old = parents.find((parent) => parent.parent === link.parent && parent.room_id === link.room_id)?.content;
    if (old?.canonical !== true || old?.via?.length !== 1 || old.via[0] !== server) actions.push(link);
  }
  return actions;
}

export type Entry = {
  v: 1;
  node: string;
  kind: "reply" | "result" | "notice";
  tag: string | null;
  text: string;
  file: string | null;
  source: "claude-stop" | "claude-subagent" | "codex-stop" | "remote-trigger" | "vast-job" | "vast-reaper" | "status" | "cli";
  session: string | null;
  ts: string;
};

export function outboundRoom(entry: Entry, nodes: Node[]): { node: string; tag: string | null } {
  const byNode = new Map(nodes.map((node) => [node.id, node]));
  let node = byNode.get(entry.node);
  const tag = entry.tag ?? (node?.role === "worker" ? node.id : null);
  const seen = new Set<string>();
  while (node?.role === "worker") {
    if (seen.has(node.id)) throw new Error("worker parent cycle");
    seen.add(node.id);
    if (!byNode.has(node.parent)) return { node: "proxy", tag };
    node = byNode.get(node.parent);
  }
  return { node: node?.id ?? entry.node, tag };
}

export function formatOutbound(entry: Entry) {
  const text = (entry.tag ? `[${entry.tag}] ` : "") + entry.text;
  const attachment = entry.file !== null || text.length > 4000 || text.split("\n").length > 40;
  return {
    msgtype: attachment ? "m.file" : entry.kind === "notice" ? "m.notice" : "m.text",
    body: attachment ? text.split("\n").filter((line) => line.trim()).slice(0, 3).map((line) => line.slice(0, 200)).join("\n") : text,
    attachment,
    filename: `${localpart(entry.node)}-${entry.ts}.md`,
    text,
  };
}

export type ChatEvent = {
  event_id: string; type: string; sender: string; origin_server_ts: number;
  content: { msgtype?: string; body?: string; [key: string]: unknown };
};

export function chatText(text: string): string {
  return text.replace(/\x1b\[(?:200|201)~/g, "").replace(/\r?\n/g, "\u2028").replace(/[\x00-\x1f\x7f-\x9f]/g, "");
}

export const dialogOpen = (pane: string) => /(?:Enter to select|Enter to confirm|Esc to cancel|Do you trust|permission dialog|Allow once|Yes, allow|Would you like to proceed|❯\s*\d+\.)/i.test(pane);
export const inputReady = (pane: string) => pane.trimEnd().split("\n").slice(-8).some((line) => /^(?:❯|›)[ \u00a0]/.test(line));

export type InboundDecision =
  | { kind: "drop"; reason: string }
  | { kind: "notice"; node: string; text: string }
  | { kind: "deliver" | "queue"; node: string; text: string; copy: string | null; notice: string | null; wake: { node: string; text: string } | null };

export function routeInbound(event: ChatEvent, roomId: string, nodes: Node[], rooms: Room[], tom: string,
  live: Record<string, boolean>, dialogs: Record<string, boolean>, proxyPane: boolean, reply = "", queued = 0): InboundDecision {
  const room = rooms.find((room) => room.room_id === roomId && room.state !== "finished");
  if (!room || event.type !== "m.room.message") return { kind: "drop", reason: "unowned room or non-message" };
  if (!tom || event.sender !== tom) return { kind: "drop", reason: "sender not Tom" };
  if (event.content.msgtype !== "m.text" || typeof event.content.body !== "string") return { kind: "notice", node: room.node, text: "only text is delivered" };
  const body = chatText(reply ? event.content.body.replace(/^>[^\n]*(?:\n|$)(?:>[^\n]*(?:\n|$))*\n/, "") : event.content.body);
  const text = `Tom (chat): ${reply ? `Re: ${chatText(reply.split("\n")[0]!)} — ` : ""}${body}`;
  const node = nodes.find((node) => node.id === room.node);
  const available = live[room.node] === true && (room.node !== "proxy" || proxyPane);
  const blocked = dialogs[room.node] === true;
  const copy = room.node === "proxy" ? null : `Tom → ${room.node}: ${body.slice(0, 300)}`;
  const notice = available && !blocked ? null : blocked ? `a dialog is open; agents attach ${room.node}` : `queued (${queued + 1})`;
  const parent = node?.parent;
  const wake = available || room.node === "proxy" ? null : {
    node: parent && parent !== "proxy" && live[parent] === true && !dialogs[parent] ? parent : "proxy",
    text: `[socrates-chat] Tom wrote to ${room.node}, which is not live: "${text}". Restart: agents lead restart ${room.node}; queued messages follow when it starts.`,
  };
  return { kind: available && !blocked ? "deliver" : "queue", node: room.node, text, copy, notice, wake };
}
