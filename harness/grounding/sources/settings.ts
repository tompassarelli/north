import { existsSync, readFileSync } from "node:fs";
import { basename, join } from "node:path";
import type { Doc } from "../store";
import { HOME } from "../store";

export const COMPLETE = true;
export const REFRESH_SECONDS = 900;

const SETTINGS = () => process.env.NORTH_GROUNDING_SETTINGS ?? join(HOME, ".claude/settings.json");
const DURABLE_EVENTS = join(import.meta.dir, "../../lib/durable_events.py");
const HEADER_LINES = 25;

type Hook = { command?: string };
type Registration = { matcher?: string; hooks?: Hook[] };

export function headerComment(path: string): string {
  let text: string;
  try {
    text = readFileSync(path, "utf8");
  } catch {
    return "";
  }
  if (text.slice(0, 4096).includes("\0")) return "";
  const out: string[] = [];
  for (const line of text.split("\n").slice(0, HEADER_LINES)) {
    if (line.startsWith("#!")) continue;
    const comment = line.match(/^\s*(?:#|\/\/)\s?(.*)$/);
    if (!comment) {
      if (out.length) break;
      if (line.trim()) break;
      continue;
    }
    out.push(comment[1].replace(/^[=\-_]{4,}$/, "").trimEnd());
  }
  return out.filter((l, i) => l || (i > 0 && out[i - 1])).join("\n").trim();
}

function hookFile(command: string): string {
  const tokens = command.trim().split(/\s+/);
  for (let i = tokens.length - 1; i >= 0; i--) if (tokens[i].startsWith("/")) return tokens[i];
  return tokens[tokens.length - 1] ?? "";
}

export function* hookDocs(settingsPath: string, at: number): Iterable<Doc> {
  let settings: { hooks?: Record<string, Registration[]> };
  try {
    settings = JSON.parse(readFileSync(settingsPath, "utf8"));
  } catch {
    return;
  }
  const seen = new Set<string>();
  for (const [event, registrations] of Object.entries(settings.hooks ?? {})) {
    for (const registration of registrations) {
      for (const hook of registration.hooks ?? []) {
        if (!hook.command) continue;
        const file = hookFile(hook.command);
        const name = basename(file);
        const matcher = registration.matcher ?? "";
        const id = `hook:${event}:${matcher}:${name}`;
        if (seen.has(id)) continue;
        seen.add(id);
        const header = headerComment(file);
        yield { source: "settings", id, ref: `.claude/settings.json ${event}${matcher ? ` ${matcher}` : ""} ${name}`,
          title: `registered ${event} hook ${name}${matcher ? ` on ${matcher}` : ""}`,
          text: [`Claude Code settings.json registers the ${name} hook (${file}) for the ${event} event${matcher ? `, matcher ${matcher}` : ", every tool"}.`, header].filter(Boolean).join("\n"),
          at, decays: false };
      }
    }
  }
}

export function eventsDoc(durablePath: string, at: number): Doc | null {
  if (!existsSync(durablePath)) return null;
  const source = readFileSync(durablePath, "utf8");
  const kinds = [...(source.match(/^KINDS = \(([^)]*)\)/m)?.[1] ?? "").matchAll(/'([^']+)'/g)].map((m) => m[1]);
  const fields = [...(source.match(/^FIELDS = \(([^)]*)\)/m)?.[1] ?? "").matchAll(/'([^']+)'/g)].map((m) => m[1]);
  return { source: "settings", id: "events-db", ref: "~/.local/state/threads/threads.db",
    title: "durable events DB: the events table in the threads ledger threads.db",
    text: [
      "The durable event system (harness/lib/durable_events.py) stores supervisor events in the events table of the threads ledger SQLite database, ~/.local/state/threads/threads.db on the node that owns the ledger and snapshot.db on remote nodes; THREADS_DB overrides the path. Test fixtures name that file events.db.",
      "Table events(seq, ts, item, kind, actor, tier, data): data is JSON carrying the event fields and delivery state.",
      kinds.length ? `Event kinds: ${kinds.join(", ")}.` : "",
      fields.length ? `Event fields: ${fields.join(", ")}.` : "",
      "Appended through bin/threads and read by harness/lib/events.py.",
    ].filter(Boolean).join("\n"), at, decays: false };
}

export function* iterDocs(_since: number | null): Iterable<Doc> {
  const now = Math.floor(Date.now() / 1000);
  yield* hookDocs(SETTINGS(), now);
  const events = eventsDoc(DURABLE_EVENTS, now);
  if (events) yield events;
}
