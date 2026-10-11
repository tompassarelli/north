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

export const mention = (tom: string, body: string) => ({ msgtype: "m.text", body: `${tom} ${body}`, "m.mentions": { user_ids: [tom] } });

export function formatOutbound(entry: Entry, tom = "", roomNode = entry.node) {
  const text = (entry.tag ? `[${entry.tag}] ` : "") + entry.text;
  const attachment = entry.file !== null || text.length > 4000 || text.split("\n").length > 40;
  const needsTom = !!tom && /^Needs Tom:/im.test(entry.text);
  const summary = text.split("\n").filter((line) => line.trim()).slice(0, 3).map((line) => line.slice(0, 200)).join("\n");
  return {
    msgtype: attachment ? "m.file" : needsTom || roomNode === "proxy" ? "m.text" : "m.notice",
    body: attachment ? summary : needsTom ? `${tom} ${text}` : text,
    ...(needsTom && !attachment ? { "m.mentions": { user_ids: [tom] } } : {}),
    ping: needsTom && attachment ? mention(tom, entry.text.split("\n").find((line) => /^Needs Tom:/i.test(line))!) : null,
    attachment,
    filename: `${localpart(entry.node)}-${entry.ts}.md`,
    text,
  };
}

export type ChatEvent = {
  event_id: string; type: string; sender: string; origin_server_ts: number;
  content: { msgtype?: string; body?: string; [key: string]: unknown };
};

export const watchGrammar = "watch <repo>#<n>|this|<node>|<box> [done] [if over <n>m|<n>min|<n>h|<n>d]; unwatch <target>";
export type WatchCommand = { kind: "watch" | "unwatch"; target: string; duration: number | null } | { kind: "invalid" };
export function parseWatch(body: string): WatchCommand | null {
  if (!/^(?:watch|unwatch)\s/i.test(body)) return null;
  const match = /^(watch|unwatch)\s+([a-z0-9._-]+(?:#[1-9]\d*)?)(?:\s+(done))?(?:\s+if\s+over\s+(\d+)(min|m|h|d))?\s*$/i.exec(body);
  if (!match || match[1]!.toLowerCase() === "unwatch" && (match[3] || match[4])) return { kind: "invalid" };
  const duration = match[4] ? Number(match[4]) * ({ m: 60_000, min: 60_000, h: 3_600_000, d: 86_400_000 }[match[5]!.toLowerCase()]!) : null;
  if (duration !== null && (!Number.isSafeInteger(duration) || duration <= 0)) return { kind: "invalid" };
  return { kind: match[1]!.toLowerCase() as "watch" | "unwatch", target: match[2]!.toLowerCase(), duration };
}

export type WatchSignal = "done" | "blocked" | "overdue";
export type ChatWatch = { id: string; room_id: string; target: string; created: number; deadline: number | null; stated?: boolean; fired: WatchSignal[]; ended: boolean };
export type ChatEta = { target: string; deadline: number; posted: number };
export type WatchTimeline = { room_id: string; node: string; event: ChatEvent };
export type WatchPost = { txn: string; room_id: string; content: { msgtype: string; body: string; "m.mentions"?: { user_ids: string[] } } };

function postTargets(event: ChatEvent): string[] {
  const sender = /^@agent-([^:]+):/.exec(event.sender)?.[1];
  if (!sender || typeof event.content.body !== "string") return [];
  const tag = /^\[([^\]]+)\]/.exec(event.content.body)?.[1];
  return [...new Set([sender === "plato" ? "proxy" : sender, ...(tag ? [tag.toLowerCase()] : []),
    ...Array.from(event.content.body.matchAll(/\b[A-Z]\d+\b/gi), (match) => match[0].toLowerCase())])];
}

export function decideWatches(watches: ChatWatch[], etas: ChatEta[], timeline: WatchTimeline[], issues: Record<string, string>, now: number, tom: string) {
  let next = watches.map((watch) => ({ ...watch, fired: [...watch.fired] }));
  const byEta = new Map(etas.map((eta) => [eta.target, { ...eta }]));
  const posts: WatchPost[] = [];
  const fire = (watch: ChatWatch, signal: WatchSignal) => {
    if (watch.ended || watch.fired.includes(signal)) return;
    watch.fired.push(signal);
    if (signal === "done") watch.ended = true;
    posts.push({ txn: `watch-${watch.id}-${signal}`, room_id: watch.room_id, content: mention(tom, `${signal}: ${watch.target}`) });
  };
  for (const { room_id, node, event } of [...timeline].sort((a, b) => a.event.origin_server_ts - b.event.origin_server_ts)) {
    if (event.type !== "m.room.message" || typeof event.content.body !== "string") continue;
    const body = event.content.body;
    if (event.sender === tom && event.content.msgtype === "m.text") {
      const command = parseWatch(body);
      if (!command) continue;
      let text = `Watch grammar: ${watchGrammar}`;
      if (command.kind !== "invalid") {
        const target = command.target === "this" ? node.toLowerCase() : command.target === "plato" ? "proxy" : command.target;
        next = next.filter((watch) => watch.room_id !== room_id || watch.target !== target);
        if (command.kind === "watch") {
          const deadline = command.duration === null ? byEta.get(target)?.deadline ?? null : event.origin_server_ts + command.duration;
          next.push({ id: event.event_id, room_id, target, created: event.origin_server_ts, deadline, stated: command.duration !== null, fired: [], ended: false });
          text = `Watching ${target}; deadline: ${deadline === null ? "none" : new Date(deadline).toISOString()}`;
        } else text = `Unwatched ${target}`;
      }
      posts.push({ txn: `watch-ack-${event.event_id}`, room_id, content: { msgtype: "m.notice", body: text } });
      continue;
    }
    const targets = postTargets(event);
    if (!targets.length) continue;
    for (const line of body.split("\n")) {
      const eta = /\bETA(?::\s*|\s+)(\d+)\s*(minutes|min|m|hours|h)\b/i.exec(line);
      if (!eta) continue;
      const duration = Number(eta[1]) * (/^(?:hours|h)$/i.test(eta[2]!) ? 3_600_000 : 60_000);
      if (!Number.isSafeInteger(duration) || duration <= 0) continue;
      const sender = /^@agent-([^:]+):/.exec(event.sender)![1]!;
      const lineTargets = targets.filter((target) => target === (sender === "plato" ? "proxy" : sender) || target === /^\[([^\]]+)\]/.exec(body)?.[1]?.toLowerCase() ||
        Array.from(line.matchAll(/\b[A-Z]\d+\b/gi), (match) => match[0].toLowerCase()).includes(target));
      for (const target of lineTargets) {
        if ((byEta.get(target)?.posted ?? -Infinity) <= event.origin_server_ts) byEta.set(target, { target, posted: event.origin_server_ts, deadline: event.origin_server_ts + duration });
      }
    }
    const sender = /^@agent-([^:]+):/.exec(event.sender)![1]!;
    const tag = /^\[([^\]]+)\]/.exec(body)?.[1]?.toLowerCase();
    const named = (pattern: RegExp) => {
      const lines = body.replace(/^\[[^\]]+\] /, "").split("\n").filter((line) => pattern.test(line));
      if (!lines.length) return null;
      return new Set([sender === "plato" ? "proxy" : sender, ...(tag ? [tag] : []),
        ...lines.flatMap((line) => Array.from(line.matchAll(/\b[A-Z]\d+\b/gi), (match) => match[0].toLowerCase()))]);
    };
    const done = named(/^Done:/i);
    const blocked = named(/^(?:Blocked:|Not done:)/i);
    for (const watch of next) {
      if (event.origin_server_ts < watch.created) continue;
      if (done?.has(localpart(watch.target))) fire(watch, "done");
      else if (blocked?.has(localpart(watch.target))) fire(watch, "blocked");
    }
  }
  for (const watch of next) {
    if (issues[watch.target]?.toUpperCase() === "CLOSED") fire(watch, "done");
    if (!watch.stated) watch.deadline = byEta.get(watch.target)?.deadline ?? watch.deadline;
    if (watch.deadline !== null && now >= watch.deadline) fire(watch, "overdue");
  }
  return { watches: next, etas: [...byEta.values()], posts };
}

export function chatText(text: string): string {
  return text.replace(/\x1b\[(?:200|201)~/g, "").replace(/\r?\n/g, "\u2028").replace(/[\x00-\x1f\x7f-\x9f]/g, "");
}

export const dialogOpen = (pane: string) => /(?:Enter to select|Enter to confirm|Esc to cancel|Do you trust|permission dialog|Allow once|Yes, allow|Would you like to proceed|❯\s*\d+\.)/i.test(pane);
export const inputReady = (pane: string) => pane.trimEnd().split("\n").slice(-8).some((line) => /^(?:❯|›)[ \u00a0]/.test(line));

export type InboundDecision =
  | { kind: "drop"; reason: string }
  | { kind: "watch" }
  | { kind: "notice"; node: string; text: string }
  | { kind: "deliver" | "queue"; node: string; text: string; copy: string | null; notice: string | null; wake: { node: string; text: string } | null };

export function routeInbound(event: ChatEvent, roomId: string, nodes: Node[], rooms: Room[], tom: string,
  live: Record<string, boolean>, dialogs: Record<string, boolean>, proxyPane: boolean, reply = "", queued = 0): InboundDecision {
  const room = rooms.find((room) => room.room_id === roomId && room.state !== "finished");
  if (!room || event.type !== "m.room.message") return { kind: "drop", reason: "unowned room or non-message" };
  if (!tom || event.sender !== tom) return { kind: "drop", reason: "sender not Tom" };
  if (event.content.msgtype !== "m.text" || typeof event.content.body !== "string") return { kind: "notice", node: room.node, text: "only text is delivered" };
  if (parseWatch(event.content.body)) return { kind: "watch" };
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
