import { Database } from "bun:sqlite";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { HOME } from "../store";

export const CONVO_DB = process.env.NORTH_GROUNDING_CONVO_DB ?? join(HOME, ".local/state/convo/index-v4.db");

export const iso = (at: number) => new Date(at * 1000).toISOString();
export const epoch = (ts: string) => {
  const t = Date.parse(ts);
  return Number.isNaN(t) ? 0 : t / 1000;
};
export const stamp = (at: number) => {
  const d = new Date(at * 1000);
  const month = d.toLocaleString("en-GB", { month: "short", timeZone: "UTC" });
  return `${String(d.getUTCDate()).padStart(2, "0")} ${month} ${String(d.getUTCHours()).padStart(2, "0")}:${String(d.getUTCMinutes()).padStart(2, "0")}Z`;
};

export function convo(): Database | null {
  if (!existsSync(CONVO_DB)) return null;
  const db = new Database(CONVO_DB, { readonly: true });
  db.exec("PRAGMA busy_timeout=10000");
  return db;
}
