import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { HOME } from "../store";

export const CODE = process.env.NORTH_GROUNDING_CODE ?? join(HOME, "code");
const OWNER = process.env.NORTH_GROUNDING_OWNER ?? "tompassarelli";

export function ownedMains(): string[] {
  let names: string[] = [];
  try {
    names = readdirSync(CODE).sort();
  } catch {
    return [];
  }
  return names.map((n) => join(CODE, n, "main")).filter((main) => {
    if (!existsSync(join(main, ".git"))) return false;
    const url = Bun.spawnSync(["git", "-C", main, "remote", "get-url", "origin"], { stdout: "pipe", stderr: "ignore" });
    return url.success && url.stdout.toString().includes(OWNER);
  });
}

export function tracked(repo: string, prefix = "."): string[] {
  const out = Bun.spawnSync(["git", "-C", repo, "ls-files", "-z", "--", prefix], { stdout: "pipe", stderr: "ignore" });
  if (!out.success) return [];
  return out.stdout.toString().split("\0").filter(Boolean).map((p) => join(repo, p));
}

type Entry = { text: string; full: boolean; priority: number; order: number };
type Corpus = { root: string; files: string[] };

const MENTION_LIMIT = 5;
const MENTION_WIDTH = 200;
const TOKEN = /[A-Za-z0-9_.~$@\/-]+/g;

const priorityOf = (rel: string) => (/(^|\/)skills\//.test(rel) ? 0 : /rules?[^/]*\.toml$/.test(rel) || /AGENTS\.md$/.test(rel) ? 1 : /(^|\/)docs\//.test(rel) ? 2 : 3);
const labelOf = (rel: string) => {
  const skill = /(^|\/)skills\/([^/]+)\//.exec(rel)?.[2];
  return skill ? `skill ${skill}` : rel.split("/").slice(-2).join("/");
};

export function mentionIndex(code: { root: string; files: string[] }[], corpora: Corpus[]): Map<string, string[]> {
  const owners = new Map<string, string | null>();
  const claim = (key: string, path: string) => owners.set(key, owners.has(key) && owners.get(key) !== path ? null : path);
  for (const { root, files } of code)
    for (const path of files) {
      const rel = path.slice(root.length + 1);
      const parts = rel.split("/");
      for (let i = 0; i < parts.length - 1; i++) claim(parts.slice(i).join("/"), path);
      const base = parts[parts.length - 1];
      if (base.length >= 5 && /[-_.]/.test(base)) claim(base, path);
      else if (base.length >= 3 && parts.includes("bin")) claim(`\`${base}\``, path);
    }
  const found = new Map<string, Entry[]>();
  let order = 0;
  for (const { root, files } of corpora)
    for (const path of files) {
      if (!/\.(md|toml)$/.test(path)) continue;
      let text: string;
      try {
        text = readFileSync(path, "utf8").slice(0, 262144);
      } catch {
        continue;
      }
      const rel = path.slice(root.length + 1);
      const priority = priorityOf(rel);
      const label = labelOf(rel);
      for (const raw of text.split("\n")) {
        const line = raw.trim().replace(/^[-*>\s]+/, "");
        if (line.length < 20 || line.startsWith("```")) continue;
        if (line.replace(TOKEN, (t) => (/[/.]/.test(t) ? " " : t)).match(/[A-Za-z]{3,}/g)?.length! < 4) continue;
        order++;
        const seen = new Set<string>();
        for (const m of line.matchAll(TOKEN)) {
          const token = m[0].replace(/[.,:;)-]+$/, "").replace(/^(~|\.|\$[A-Z_]+|\/home\/[^/]+)\//, "");
          let key: string | undefined;
          let full = false;
          if (token.includes("/")) {
            const parts = token.split("/");
            for (let i = 0; i < parts.length - 1 && !key; i++) {
              const suffix = parts.slice(i).join("/");
              if (owners.get(suffix)) {
                key = suffix;
                full = true;
              }
            }
          } else if (owners.get(token)) key = token;
          else if (owners.get(`\`${token}\``) && line.includes(`\`${token}\``)) key = `\`${token}\``;
          const owner = key ? owners.get(key) : null;
          if (!key || !owner || seen.has(owner)) continue;
          seen.add(owner);
          const start = line.length > MENTION_WIDTH ? Math.max(0, Math.min(line.indexOf(m[0]) - 80, line.length - MENTION_WIDTH)) : 0;
          const entries = found.get(owner) ?? [];
          entries.push({ text: `${label}: ${line.slice(start, start + MENTION_WIDTH)}`, full, priority, order });
          found.set(owner, entries);
        }
      }
    }
  const out = new Map<string, string[]>();
  for (const [owner, entries] of found) {
    const all = entries.sort((a, b) => Number(!a.full) - Number(!b.full) || a.priority - b.priority || a.order - b.order);
    const picked: string[] = [];
    const perLabel = new Map<string, number>();
    for (const e of all) {
      const label = e.text.split(": ", 1)[0];
      if (picked.includes(e.text) || picked.length >= MENTION_LIMIT || (perLabel.get(label) ?? 0) >= 2) continue;
      perLabel.set(label, (perLabel.get(label) ?? 0) + 1);
      picked.push(e.text);
    }
    out.set(owner, picked);
  }
  return out;
}
