import type { Doc } from "../store";
import { convo, epoch, iso, stamp } from "./_convo";

export const RETAIN_DAYS = 90;
export const REFRESH_SECONDS = 600;
const REPORT = /^\s*(\*\*)?(Done|Not done|Blocked)\b|\bLANDED\b/;

export function* iterDocs(since: number | null): Iterable<Doc> {
  const db = convo();
  if (!db) return;
  try {
    const floor = Math.max(since ?? 0, Date.now() / 1000 - RETAIN_DAYS * 86400);
    const rows = db.query(
      "select m.id, m.ts, f.project, b.text from msg m join files f on f.id = m.file_id join body b on b.id = m.body " +
      "where m.ts > ? and m.role = 'assistant' and m.dup = 0 and (b.text like 'Done%' or b.text like 'Not done%' " +
      "or b.text like 'Blocked%' or b.text like '**Done%' or b.text like '%LANDED%')").values(iso(floor)) as [number, string, string | null, string][];
    for (const [id, ts, project, text] of rows) {
      if (!text || !REPORT.test(text.slice(0, 4000))) continue;
      const at = epoch(ts);
      yield { source: "builds", id: String(id), ref: `convo:${id}`,
        title: `report ${stamp(at).slice(0, 6)} ${project ?? ""}: ${text.split(/\s+/).join(" ").slice(0, 110)}`,
        text: text.slice(0, 1500), at, decays: true };
    }
  } finally {
    db.close();
  }
}
