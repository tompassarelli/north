import { Database } from "bun:sqlite";
import { createHash } from "node:crypto";

export interface AskInput {
  questions: { question: string; header: string; options: { label: string; description: string }[]; multiSelect?: boolean }[];
}
export interface QuestionPayload {
  version: 1;
  questions: { id: string; prompt: string; helpText: string; selectionMode: "single" | "multi"; allowOther: true; options: { id: string; label: string; description: string }[] }[];
}
export interface Interaction {
  id: string;
  status: string;
  result?: { answers?: { questionId: string; optionIds: string[]; otherText?: string | null }[] } | null;
}
export type DecisionApi = (path: string, method?: string, body?: Record<string, unknown>) => Promise<any>;
export interface PendingAnswer {
  id: string;
  interactionId: string;
  sourceKey: string;
  questionId: string;
  question: string;
  labels: string[];
  line: string;
}
interface Decision {
  sourceKey: string;
  targetKey: string;
  issueId: string;
  payload: QuestionPayload;
  interactionId: string | null;
  status: string;
  recordBody?: string;
  commentId?: string;
}

function stableId(value: string): string {
  const hex = createHash("sha256").update(value).digest("hex").slice(0, 32);
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-5${hex.slice(13, 16)}-a${hex.slice(17, 20)}-${hex.slice(20)}`;
}

export function questionPayload(input: AskInput): QuestionPayload {
  if (!input.questions?.length || input.questions.length > 20) throw new Error("Expected 1–20 questions");
  return { version: 1, questions: input.questions.map((q, i) => {
    if (!q.question?.trim() || q.question.length > 4000 || typeof q.header !== "string" || q.header.length > 4000 || !q.options?.length || q.options.length > 129) throw new Error("Invalid decision question");
    return { id: `q${i + 1}`, prompt: q.question, helpText: q.header, selectionMode: q.multiSelect ? "multi" : "single", allowOther: true,
      options: q.options.map((o, j) => {
        if (!o.label?.trim() || o.label.length > 1000 || typeof o.description !== "string" || o.description.length > 4000) throw new Error("Invalid decision option");
        return { id: `q${i + 1}o${j + 1}`, label: o.label, description: o.description };
      }) };
  }) };
}

export function decodeAnswers(payload: QuestionPayload, interaction: Interaction, sourceKey: string): PendingAnswer[] {
  if (interaction.status !== "answered" || !Array.isArray(interaction.result?.answers)) throw new Error("Missing answered interaction result");
  const seen = new Set<string>();
  return interaction.result.answers.map(answer => {
    const question = payload.questions.find(q => q.id === answer.questionId);
    if (!question || seen.has(answer.questionId) || !Array.isArray(answer.optionIds) || new Set(answer.optionIds).size !== answer.optionIds.length) throw new Error("Invalid answer question or options");
    seen.add(answer.questionId);
    const labels = answer.optionIds.map(id => {
      const option = question.options.find(o => o.id === id);
      if (!option) throw new Error("Unknown answer option");
      return option.label;
    });
    if (question.selectionMode === "single" && labels.length > 1) throw new Error("Multiple options for single selection");
    if (answer.otherText?.trim()) labels.push(answer.otherText);
    const id = `${interaction.id}:${question.id}`;
    const printable = (text: string) => text.replace(/[\r\n]/g, " ");
    return { id, interactionId: interaction.id, sourceKey, questionId: question.id, question: question.prompt, labels,
      line: `ANSWER ${interaction.id} ${question.id} ${printable(question.prompt)} -> ${labels.map(printable).join("; ")}` };
  });
}

export class DecisionsInbox {
  constructor(private api: DecisionApi, private db: Database, private targets: Readonly<Record<string, string>>, private fallbackKey = "decisions") {
    db.exec("CREATE TABLE IF NOT EXISTS paperclip_decisions (source_key TEXT PRIMARY KEY, data TEXT NOT NULL); CREATE TABLE IF NOT EXISTS paperclip_decision_answers (id TEXT PRIMARY KEY, source_key TEXT NOT NULL, data TEXT NOT NULL, delivered INTEGER NOT NULL DEFAULT 0)");
  }

  private held(text: string, targetKey: string, issueId?: string): boolean {
    const repos = new Set((process.env.PAPERCLIP_SYNC_HOLD_REPOS || "").split(",").map(r => r.trim()).filter(Boolean));
    const heldKey = (key: string) => repos.has(key.replace(/^gh:/, "").split("#")[0]);
    return heldKey(targetKey) || Object.entries(this.targets).some(([key, id]) => id === issueId && heldKey(key)) ||
      (text.match(/[A-Za-z0-9_.-]+#[1-9][0-9]*/g) || []).some(heldKey) ||
      [...repos].some(repo => text === repo || text.startsWith(`gh:${repo}#`));
  }

  private allowed(row: Decision, extra = ""): boolean {
    return !this.held(JSON.stringify(row) + extra, row.targetKey, row.issueId);
  }

  private load(sourceKey: string): Decision | null {
    const row = this.db.query("SELECT data FROM paperclip_decisions WHERE source_key = ?").get(sourceKey) as { data: string } | null;
    return row ? JSON.parse(row.data) : null;
  }

  private save(row: Decision): void {
    this.db.query("INSERT INTO paperclip_decisions (source_key,data) VALUES (?,?) ON CONFLICT(source_key) DO UPDATE SET data=excluded.data").run(row.sourceKey, JSON.stringify(row));
  }

  private prepare(sourceKey: string, input: AskInput, targetKey?: string): Decision {
    const payload = questionPayload(input);
    const text = JSON.stringify(input);
    const refs = text.match(/[A-Za-z0-9_.-]+#[1-9][0-9]*/g) || [];
    const key = targetKey || refs.map(ref => `gh:${ref}`).find(ref => this.targets[ref]) || this.fallbackKey;
    const issueId = this.targets[key];
    if (!issueId) throw new Error("Decision target is not owned");
    const row: Decision = { sourceKey, targetKey: key, issueId, payload, interactionId: null, status: "creating" };
    if (!this.allowed(row, text)) throw new Error("Decision is held by repository restriction");
    return row;
  }

  async ask(sourceKey: string, input: AskInput, targetKey?: string): Promise<Decision> {
    const proposed = this.prepare(sourceKey, input, targetKey);
    const row = this.load(sourceKey) || proposed;
    if (!this.allowed(row)) throw new Error("Decision is held by repository restriction");
    if (JSON.stringify(row.payload) !== JSON.stringify(proposed.payload) || row.issueId !== proposed.issueId) throw new Error("Decision source key already has different input");
    if (row.interactionId || row.status !== "creating") return row;
    this.save(row);
    const interaction: Interaction = await this.api(`/api/issues/${row.issueId}/interactions`, "POST", {
      kind: "ask_user_questions", resolverPolicy: "human_only", continuationPolicy: "none", idempotencyKey: stableId(`ask:${sourceKey}`),
      title: input.questions[0].header.trim().slice(0, 240) || "Decision for Tom", payload: row.payload,
    });
    if (!interaction.id || !interaction.status) throw new Error("Invalid created interaction");
    row.interactionId = interaction.id;
    this.accept(row, interaction);
    return row;
  }

  private accept(row: Decision, interaction: Interaction): void {
    if (!this.allowed(row) || row.status === "answered" || row.status === "cancelled" || row.status === "recorded") return;
    const answers = interaction.status === "answered" ? decodeAnswers(row.payload, interaction, row.sourceKey) : [];
    if (answers.some(answer => this.held(JSON.stringify(answer), row.targetKey, row.issueId))) return;
    this.db.transaction(() => {
      const saved = this.load(row.sourceKey);
      if (saved && ["answered", "cancelled", "recorded"].includes(saved.status)) {
        Object.assign(row, saved);
        return;
      }
      for (const answer of answers) this.db.query("INSERT OR IGNORE INTO paperclip_decision_answers (id,source_key,data) VALUES (?,?,?)").run(answer.id, row.sourceKey, JSON.stringify(answer));
      row.status = interaction.status;
      this.save(row);
    })();
  }

  async poll(): Promise<number> {
    const rows = this.db.query("SELECT data FROM paperclip_decisions").all() as { data: string }[];
    const pending = rows.map(r => JSON.parse(r.data) as Decision).filter(r => r.interactionId && !["answered", "cancelled", "recorded"].includes(r.status) && this.allowed(r));
    const byIssue = new Map<string, Decision[]>();
    for (const row of pending) byIssue.set(row.issueId, [...(byIssue.get(row.issueId) || []), row]);
    let resolved = 0;
    for (const [issueId, owned] of byIssue) {
      const interactions: Interaction[] = await this.api(`/api/issues/${issueId}/interactions`);
      if (!Array.isArray(interactions)) throw new Error("Invalid interactions response");
      for (const row of owned) {
        const interaction = interactions.find(i => i.id === row.interactionId);
        if (!interaction) continue;
        const before = row.status;
        this.accept(row, interaction);
        if (before !== row.status && ["answered", "cancelled"].includes(row.status)) resolved++;
      }
    }
    return resolved;
  }

  async withdraw(interactionId: string, reason = "Answered in terminal"): Promise<string> {
    const rows = this.db.query("SELECT data FROM paperclip_decisions").all() as { data: string }[];
    const row = rows.map(r => JSON.parse(r.data) as Decision).find(r => r.interactionId === interactionId);
    if (!row) throw new Error("Interaction is not owned");
    if (!this.allowed(row, reason)) throw new Error("Decision is held by repository restriction");
    if (["answered", "cancelled", "recorded"].includes(row.status)) return row.status;
    let interaction: Interaction;
    try {
      interaction = await this.api(`/api/issues/${row.issueId}/interactions/${interactionId}/withdraw`, "POST", { reason });
    } catch (error) {
      if (!this.allowed(row)) throw error;
      const interactions: Interaction[] = await this.api(`/api/issues/${row.issueId}/interactions`);
      const resolved = Array.isArray(interactions) && interactions.find(i => i.id === interactionId && ["answered", "cancelled"].includes(i.status));
      if (!resolved) throw error;
      interaction = resolved;
    }
    this.accept(row, interaction);
    return row.status;
  }

  async record(sourceKey: string, input: AskInput, answer: string, targetKey?: string): Promise<string | null> {
    const proposed = this.prepare(sourceKey, input, targetKey);
    const row = this.load(sourceKey) || proposed;
    if (JSON.stringify(row.payload) !== JSON.stringify(proposed.payload) || row.issueId !== proposed.issueId) throw new Error("Decision source key already has different input");
    if (!this.allowed(row, answer)) throw new Error("Decision is held by repository restriction");
    if (row.interactionId && !["answered", "cancelled", "recorded"].includes(row.status)) await this.withdraw(row.interactionId);
    const current = this.load(sourceKey) || row;
    if (current.status === "answered") return null;
    if (current.status === "recorded") return current.commentId!;
    current.recordBody ||= input.questions.map(q => q.question).join("\n") + `\n\nTom's terminal answer: ${answer}`;
    current.commentId ||= stableId(`record:${sourceKey}`);
    this.save(current);
    await this.api(`/api/issues/${current.issueId}/comments`, "POST", {
      body: current.recordBody, clientRequestId: current.commentId, resume: false, interrupt: false,
      presentation: { kind: "system_notice", tone: "info" },
      metadata: { version: 1, sections: [{ rows: [{ type: "text", label: "Decision", text: "Terminal answer recorded" }] }] },
    });
    current.status = "recorded";
    this.save(current);
    return current.commentId;
  }

  pendingAnswers(): PendingAnswer[] {
    const rows = this.db.query("SELECT data FROM paperclip_decision_answers WHERE delivered = 0 ORDER BY rowid").all() as { data: string }[];
    return rows.map(r => JSON.parse(r.data) as PendingAnswer).filter(answer => {
      const row = this.load(answer.sourceKey);
      return row && this.allowed(row, JSON.stringify(answer));
    });
  }

  ackDelivered(id: string): boolean {
    if (!this.pendingAnswers().some(answer => answer.id === id)) return false;
    return this.db.query("UPDATE paperclip_decision_answers SET delivered = 1 WHERE id = ? AND delivered = 0").run(id).changes === 1;
  }
}
