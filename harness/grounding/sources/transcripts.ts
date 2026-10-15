import type { Doc } from "../store";
import { convo, epoch, iso, stamp } from "./_convo";

export const RETAIN_DAYS = 14;
export const REFRESH_SECONDS = 300;
const HANDOFFS = String.raw`(?:\/home\/tom|~|\$HOME)\/\.local\/state\/agents\/handoffs`;
const HARNESS = /^\s*(<(?!pasted_content|north_machine_relay)|\/|\[Request interrupted|\[routine:|Caveat:|This session is being continued|Stop hook feedback)/;
const BRIEF = new RegExp(
  String.raw`^\s*(Delegation:|Read (and follow|\S+ and follow|${HANDOFFS})|You are (a|an|the) |(LEAD )?COACH\b|Coach repl|\S+-lead -> |From (\S+-lead|the proxy)|\S+-lead box |Run exactly this |New priority:|Use (the Agent tool|spawn_agent|the automation-chrome)|(Review|Adversarial review) (this|of a) diff|─{5})`, "i");
const DIRECTED = /^\s*(Claude[:,] |\S+-lead[,:] )/;
const FACETS = /^(Category|Scope|Surface|Verify|Spec|Parent|Setup|Done when):/gm;
const PEER = /Another Claude session sent a message/;
const PROXY_OPENING = new RegExp(String.raw`handoffs\/proxy-handoff|take over as the proxy|You are the proxy`, "i");
const RELAY = /(from=proxy|^<north_machine_relay\/>\s*(EVENT \d+ \w+\s+from=proxy|Proxy)|\bTom\b|\bTOM\b)/;
const TEXT_LIMIT = 1500;

export type SessionKind = "tom" | "proxy" | "agent";

export const isBrief = (text: string) =>
  HARNESS.test(text) || BRIEF.test(text) || PEER.test(text.slice(0, 400)) || (text.match(FACETS)?.length ?? 0) >= 2;

export function sessionKind(opening: string | null | undefined): SessionKind {
  if (typeof opening !== "string") return "tom";
  if (PROXY_OPENING.test(opening.slice(0, 600))) return "proxy";
  return isBrief(opening) ? "agent" : "tom";
}

export function speaker(role: string, kind: SessionKind, text: string): string | null {
  if (role === "user") {
    if (DIRECTED.test(text) && RELAY.test(text.slice(0, 600))) return "relay";
    if (/^\s*<north_machine_relay/.test(text)) return RELAY.test(text.slice(0, 600)) && !isBrief(text.replace(/^\s*<north_machine_relay\/>\s*/, "")) ? "relay" : null;
    return isBrief(text) || text.trim().split(/\s+/).length < 2 ? null : "Tom";
  }
  if (kind === "agent") return null;
  if (role === "assistant") return text.trim().split(/\s+/).length >= 4 ? "proxy" : null;
  if (role === "tool" && text.includes("agents event")) return "proxy event";
  return null;
}

export function* iterDocs(since: number | null): Iterable<Doc> {
  const db = convo();
  if (!db) return;
  try {
    const floor = Math.max(since ?? 0, Date.now() / 1000 - RETAIN_DAYS * 86400);
    const rows = db.query(
      "select m.id, m.ts, m.role, m.file_id, f.session_id, b.text from msg m join files f on f.id = m.file_id " +
      "join body b on b.id = m.body where m.ts > ? and f.agent is null and m.dup = 0 and m.role in ('user','assistant','tool') " +
      "order by m.ts").values(iso(floor)) as [number, string, string, number, string | null, string][];
    const first = db.query("select b.text from msg m join body b on b.id = m.body where m.file_id = ? and m.role = 'user' order by m.line limit 1");
    const kinds = new Map<number, SessionKind>();
    for (const [id, ts, role, fileId, session, text] of rows) {
      if (!text || (role === "tool" && !text.includes("agents event"))) continue;
      if (!kinds.has(fileId)) kinds.set(fileId, sessionKind(first.values(fileId)[0]?.[0] as string | undefined));
      const who = speaker(role, kinds.get(fileId)!, text);
      if (!who) continue;
      const at = epoch(ts);
      yield { source: "transcripts", id: String(id), ref: `convo:${id}`, title: `${who} ${stamp(at)} session ${(session ?? "").slice(0, 8)}`,
        text: text.slice(0, TEXT_LIMIT), at, decays: true };
    }
  } finally {
    db.close();
  }
}
