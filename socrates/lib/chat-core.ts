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
