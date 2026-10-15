import { readFileSync } from "node:fs";
import { join } from "node:path";
import type { Doc } from "../store";
import { stateDir } from "../store";

export const COMPLETE = true;
export const REFRESH_SECONDS = 120;
const ledger = () => process.env.NORTH_GROUNDING_LEDGER ?? join(stateDir(), "ledger.jsonl");

type Row = { id?: string; at?: string; kind?: string; topic?: string; quote?: string; source?: string; superseded_by?: string | null };

export function* iterDocs(_since: number | null): Iterable<Doc> {
  let lines: string[];
  try {
    lines = readFileSync(ledger(), "utf8").split("\n");
  } catch {
    return;
  }
  for (const line of lines) {
    let row: Row;
    try {
      row = JSON.parse(line);
    } catch {
      continue;
    }
    if (!row || typeof row !== "object" || row.superseded_by || !row.quote || !row.id) continue;
    const source = row.source ?? "";
    const speaker = source.split(/\s+/)[0] || "tom";
    const at = Date.parse(row.at ?? "");
    yield { source: "ledger", id: row.id, ref: `ledger:${row.id}`,
      title: `${speaker} ${row.kind ?? "decision"} ${(row.at ?? "").slice(0, 16)} ${row.topic ?? ""} (${source})`,
      text: row.quote, at: Number.isNaN(at) ? 0 : at / 1000, decays: false };
  }
}
