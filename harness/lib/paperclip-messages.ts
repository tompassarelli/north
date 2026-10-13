import { Database } from "bun:sqlite";
import { commentId, type OrgNode } from "./paperclip-core";
import type { DecisionApi } from "./paperclip-decisions";

export type LeadMessage = { id: string; issueId: string; leadId: string; body: string; receivedAt: string };
export type LeadDelivery = { id: string; leadId: string; session: string; receivedAt: string; deliveredAt: string };
export type LeadReply = { id: string; issueId: string; leadId: string; session: string; body: string };
type Issue = { id: string; parentId?: string | null; assigneeAgentId?: string | null; status: string; identifier?: string; updatedAt?: string };

export class LeadInbox {
  constructor(private api: DecisionApi, private db: Database, private now = () => new Date().toISOString()) {
    db.exec("CREATE TABLE IF NOT EXISTS paperclip_lead_messages (id TEXT PRIMARY KEY, data TEXT NOT NULL, delivery TEXT, reply_id TEXT); CREATE TABLE IF NOT EXISTS paperclip_child_closures (id TEXT PRIMARY KEY, status TEXT NOT NULL, sequence INTEGER NOT NULL)");
    db.query("INSERT OR IGNORE INTO sync_meta(key,value) VALUES ('lead-message-start',?)").run(now());
  }

  async poll(issues: Issue[], agents: Readonly<Record<string, string>>, nodes: readonly OrgNode[]): Promise<void> {
    const started = (this.db.query("SELECT value FROM sync_meta WHERE key='lead-message-start'").get() as { value: string }).value;
    const replyRequests = new Set((this.db.query("SELECT id FROM paperclip_lead_messages").all() as { id: string }[]).map(row => commentId("north:lead-reply", row.id)));
    const parents = new Set(issues.flatMap(issue => issue.parentId ? [issue.parentId] : []));
    for (const issue of issues) {
      const leadId = issue.assigneeAgentId ? agents[issue.assigneeAgentId] : undefined;
      if (!parents.has(issue.id) || !nodes.some(node => node.id === leadId && node.role !== "worker" && node.state === "active" && !node.died)) continue;
      const comments = await this.api(`/api/issues/${issue.id}/comments`);
      if (!Array.isArray(comments)) throw new Error("Invalid lead comments response");
      for (const comment of comments) {
        if (typeof comment.id !== "string" || typeof comment.authorUserId !== "string" || comment.authorAgentId || typeof comment.body !== "string" || !comment.body.trim() || !Number.isFinite(Date.parse(comment.createdAt)) || Date.parse(comment.createdAt) < Date.parse(started)) continue;
        if (this.db.query("SELECT id FROM paperclip_lead_messages WHERE reply_id=?").get(comment.id)) continue;
        if (replyRequests.has(comment.clientRequestId) || comment.presentation?.kind === "system_notice") continue;
        const message: LeadMessage = { id: comment.id, issueId: issue.id, leadId: leadId!, body: comment.body, receivedAt: this.now() };
        this.db.query("INSERT OR IGNORE INTO paperclip_lead_messages(id,data) VALUES (?,?)").run(message.id, JSON.stringify(message));
      }
    }
  }

  pending(): LeadMessage[] {
    return (this.db.query("SELECT data FROM paperclip_lead_messages WHERE delivery IS NULL ORDER BY rowid").all() as { data: string }[]).map(row => JSON.parse(row.data));
  }

  acknowledge(delivery: LeadDelivery): void {
    const row = this.db.query("SELECT data FROM paperclip_lead_messages WHERE id=?").get(delivery.id) as { data: string } | null;
    if (!row || typeof delivery.session !== "string" || !delivery.session || !Number.isFinite(Date.parse(delivery.receivedAt)) || !Number.isFinite(Date.parse(delivery.deliveredAt)) || JSON.parse(row.data).leadId !== delivery.leadId) throw new Error("Invalid lead delivery receipt");
    this.db.query("UPDATE paperclip_lead_messages SET delivery=COALESCE(delivery,?) WHERE id=?").run(JSON.stringify(delivery), delivery.id);
  }

  async reply(reply: LeadReply): Promise<string> {
    const row = this.db.query("SELECT data,delivery,reply_id FROM paperclip_lead_messages WHERE id=?").get(reply.id) as { data: string; delivery: string | null; reply_id: string | null } | null;
    if (!row?.delivery) throw new Error("Reply has no delivered originating comment");
    const message: LeadMessage = JSON.parse(row.data), delivery: LeadDelivery = JSON.parse(row.delivery);
    if (reply.issueId !== message.issueId || reply.leadId !== message.leadId || reply.session !== delivery.session || typeof reply.body !== "string" || !reply.body.trim()) throw new Error("Reply does not match originating comment and lead session");
    if (row.reply_id) return reply.id;
    const repliedAt = this.now();
    const result = await this.api(`/api/issues/${message.issueId}/comments`, "POST", {
      body: reply.body, clientRequestId: commentId("north:lead-reply", reply.id), presentation: { kind: "system_notice", tone: "info" },
      metadata: { version: 1, sections: [{ rows: Object.entries({ lead: reply.leadId, origin: reply.id, receivedAt: delivery.receivedAt, deliveredAt: delivery.deliveredAt }).map(([label, value]) => ({ type: "key_value", label, value })) }] },
    });
    if (typeof result.id !== "string") throw new Error("Invalid lead reply response");
    this.db.query("UPDATE paperclip_lead_messages SET reply_id=? WHERE id=?").run(result.id, reply.id);
    console.error(`paperclip-sync lead reply ${JSON.stringify({ id: reply.id, leadId: reply.leadId, session: reply.session, repliedAt, commentId: result.id })}`);
    return reply.id;
  }

  async closures(issues: Issue[], managedIds: ReadonlySet<string>): Promise<void> {
    for (const issue of issues.filter(issue => managedIds.has(issue.id))) {
      const old = this.db.query("SELECT status,sequence FROM paperclip_child_closures WHERE id=?").get(issue.id) as { status: string; sequence: number } | null;
      const closing = old && old.status !== "done" && issue.status === "done" && issue.parentId;
      const sequence = (old?.sequence ?? 0) + (closing ? 1 : 0);
      if (closing) await this.api(`/api/issues/${issue.parentId}/comments`, "POST", {
        body: `Done: ${issue.identifier ?? issue.id}.`, clientRequestId: commentId("north:child-close", `${issue.id}:${sequence}`),
        presentation: { kind: "system_notice", tone: "success" },
      });
      this.db.query("INSERT INTO paperclip_child_closures(id,status,sequence) VALUES (?,?,?) ON CONFLICT(id) DO UPDATE SET status=excluded.status,sequence=excluded.sequence").run(issue.id, issue.status, sequence);
    }
  }
}
