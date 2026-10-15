import { existsSync, lstatSync, readdirSync, readFileSync, statSync } from "node:fs";
import { basename, dirname, extname, join, relative } from "node:path";
import type { Doc } from "../store";
import { HOME } from "../store";
import { mentionIndex, tracked } from "./_repos";

export const COMPLETE = true;
export const REFRESH_SECONDS = 900;
const NORTH = process.env.NORTH_GROUNDING_NORTH ?? join(HOME, "code/north/main");
const SOUTH = process.env.NORTH_GROUNDING_SOUTH ?? join(HOME, "code/south/main");
const HANDOFFS = process.env.NORTH_GROUNDING_HANDOFFS ?? join(HOME, ".local/state/agents/handoffs");
const CODE = new Set([".py", ".sh", ".ts", ".mjs", ".rs", ".js", ""]);
const SECTION = 1800;
const MAX_BYTES = 262144;

export const display = (path: string) => (path.startsWith(HOME + "/") ? "~" + path.slice(HOME.length) : path);

export { tracked };

const doc = (id: string, ref: string, title: string, text: string, at: number, decays = false, source = "policy"): Doc =>
  ({ source, id, ref, title, text: text.trim(), at, decays });

function read(path: string): string {
  try {
    return readFileSync(path, "utf8").slice(0, MAX_BYTES);
  } catch {
    return "";
  }
}

export function frontmatter(text: string): [Record<string, string>, string] {
  if (!text.startsWith("---\n")) return [{}, text];
  const end = text.indexOf("\n---", 4);
  if (end < 0) return [{}, text];
  const meta: Record<string, string> = {};
  let key = "";
  for (const line of text.slice(4, end).split("\n")) {
    const m = /^([A-Za-z_-]+):\s*(.*)$/.exec(line);
    if (m) {
      key = m[1];
      meta[key] = m[2].trim().replace(/^[>|-]+/, "").trim();
    } else if (key && line.startsWith(" ")) meta[key] = `${meta[key]} ${line.trim()}`.trim();
  }
  return [meta, text.slice(end + 4)];
}

export function markdownSections(text: string): [string, string][] {
  const sections: [string, string][] = [];
  let heading = "";
  let body: string[] = [];
  const flush = () => {
    if (body.join("").trim()) sections.push([heading, body.join("\n")]);
  };
  for (const line of text.split("\n")) {
    if (/^#{1,4} /.test(line)) {
      flush();
      heading = line.replace(/^#+/, "").trim();
      body = [];
    } else body.push(line);
  }
  flush();
  const out: [string, string][] = [];
  for (const [h, b] of sections) for (let i = 0; i < Math.max(b.length, 1); i += SECTION) out.push([h, b.slice(i, i + SECTION)]);
  return out;
}

const PY_DOCSTRING = /^[rRuU]?("""|''')([\s\S]*?)\1/m;

function leadingComments(text: string): string[] {
  const keep: string[] = [];
  for (const line of text.split("\n").slice(0, 120)) {
    const s = line.trim();
    if (!keep.length && (s.startsWith("#!") || !s)) continue;
    if (/^(#|\/\/|\/\*|\*|--)/.test(s)) {
      keep.push(s.replace(/^[#/*\- ]+/, "").trim());
      continue;
    }
    if (!s || /^(set -|import |from |use |const |export |source |\. )/.test(s)) continue;
    break;
  }
  return keep.filter((l) => l && !l.startsWith("shellcheck"));
}

function usageLines(text: string): string[] {
  const lines = text.split("\n").slice(0, 400);
  const at = lines.findIndex((l) => /usage:/i.test(l));
  if (at < 0) return [];
  const out: string[] = [];
  for (const line of lines.slice(at, at + 9))
    for (const m of line.replace(/\\"/g, "'").matchAll(/"([^"]{8,400})"|'([^']{8,400})'/g)) out.push((m[1] ?? m[2]).replace(/\\n/g, " ").trim());
  return out.slice(0, 9);
}

function messageStrings(text: string): string[] {
  const out: string[] = [];
  for (const m of text.matchAll(/(?:raise\s+\w+\(|die\s+|fail\s+|error\(|Error\(|exit\([^'"`\n]*|print\(|console\.(?:log|error)\(|echo\s+(?:-e\s+)?)\s*f?(?:"([^"\n]{25,200})"|'([^'\n]{25,200})')/g)) {
    const line = (m[1] ?? m[2]).trim();
    if (/\s/.test(line) && !out.includes(line)) out.push(line);
    if (out.length >= 8) break;
  }
  return out;
}

function constantNames(text: string): string[] {
  const out: string[] = [];
  for (const m of text.matchAll(/^(?:export\s+)?(?:const\s+)?([A-Z][A-Z0-9_]{3,})\s*[:=]([^\n]{0,240})/gm)) {
    const members = [...m[2].matchAll(/["']([A-Za-z][\w-]{2,})["']/g)].map((x) => x[1]).slice(0, 12);
    out.push(m[1].toLowerCase(), ...members);
  }
  return out.slice(0, 40);
}

const words = (name: string) => name.replace(/([a-z0-9])([A-Z])/g, "$1 $2").replace(/[_-]+/g, " ").toLowerCase();

function definedNames(text: string, ext: string): string[] {
  const found: string[] = [];
  const add = (re: RegExp) => {
    for (const m of text.matchAll(re)) found.push(m[1]);
  };
  if (ext === ".py") {
    add(/^(?:async\s+)?(?:def|class)\s+([A-Za-z]\w*)/gm);
    add(/add_parser\(\s*["']([\w-]+)["']/g);
    add(/add_argument\(\s*["'](--?[\w-]+)["']/g);
  } else if (ext === ".rs") add(/^\s*(?:pub(?:\([a-z]+\))?\s+)?(?:fn|struct|enum|trait)\s+([A-Za-z]\w*)/gm);
  else {
    add(/^(?:export\s+)?(?:default\s+)?(?:async\s+)?(?:function\*?|class)\s+([A-Za-z]\w*)/gm);
    add(/^(?:export\s+)?const\s+([A-Za-z]\w*)\s*=\s*(?:async\s*)?(?:\([^)]*\)|[A-Za-z]\w*)\s*(?::[^=]+)?=>/gm);
    add(/^([A-Za-z][\w-]*)\(\)\s*\{/gm);
    add(/\.command\(\s*["'`]([\w-]+)/g);
    for (const m of text.matchAll(/^\s{0,8}([a-z][\w-]*(?:\|[a-z][\w-]*)*)\)\s*(?:#.*)?$/gm)) found.push(...m[1].split("|"));
    for (const m of text.matchAll(/^\s{0,8}(-{1,2}[a-z][\w-]*(?:\|-{1,2}[a-z][\w-]*)*)\)/gm)) found.push(...m[1].split("|"));
  }
  const seen = new Set<string>();
  const out: string[] = [];
  for (const raw of found) {
    const name = raw.replace(/^-+/, "");
    if (name.length < 3 || name.startsWith("_") || seen.has(name)) continue;
    seen.add(name);
    out.push(name);
  }
  return out.slice(0, 90);
}

function language(text: string, path: string): string {
  const ext = extname(path);
  if (ext) return ext;
  const shebang = text.startsWith("#!") ? text.slice(0, text.indexOf("\n")) : "";
  return /python/.test(shebang) ? ".py" : /\b(bun|node|deno)\b/.test(shebang) ? ".ts" : ".sh";
}

export function codeHeader(text: string, path = "", mentions: string[] = []): string {
  const ext = language(text, path);
  const docstring = ext === ".py" ? PY_DOCSTRING.exec(text.split("\n").slice(0, 200).join("\n")) : null;
  const help = [...text.matchAll(/(?:description|help)\s*[=:]\s*['"`]([^'"`]{12,200})['"`]/g)].map((m) => m[1]).slice(0, 14);
  const prose = [docstring ? docstring[2].trim().slice(0, 1200) : "", ...leadingComments(text), ...help, ...usageLines(text)].filter(Boolean);
  const names = definedNames(text, ext);
  const constants = constantNames(text);
  const detail = [...messageStrings(text), ...(constants.length ? [`constants: ${constants.join(", ")}`] : [])];
  const parts = [
    ...prose,
    names.length ? `defines: ${names.map((n) => (words(n) === n ? n : `${n} ${words(n)}`)).join(", ")}` : "",
    ...detail,
    path ? `file: ${words(basename(path, extname(path)))} ${basename(path)}` : "",
    mentions.length ? `Mentioned in:\n${mentions.join("\n")}` : "",
  ];
  return parts.filter(Boolean).join("\n").trim();
}

export function markdownDocs(path: string, label: string, at: number, decays = false, source = "policy"): Doc[] {
  const text = read(path);
  if (!text) return [];
  const ref = display(path);
  const out: Doc[] = [];
  const [meta, body] = frontmatter(text);
  if (basename(path) === "SKILL.md") {
    const name = meta.name ?? basename(dirname(path));
    out.push(doc(`${ref}#skill`, ref, `skill ${name}: ${meta.description ?? ""}`, `${name} ${meta.description ?? ""}`, at, false, source));
  }
  markdownSections(body).forEach(([heading, section], i) =>
    out.push(doc(`${ref}#${i}`, ref, heading ? `${label} § ${heading}` : label, section, at, decays, source)));
  return out;
}

function rulesDocs(path: string, at: number): Doc[] {
  let data: { rule?: Record<string, string>[] };
  try {
    data = Bun.TOML.parse(read(path)) as typeof data;
  } catch {
    return [];
  }
  return (data.rule ?? []).map((rule) => {
    const ref = `${display(path)}#${rule.id}`;
    return doc(ref, ref, `rule ${rule.id} (${rule.section ?? ""}, ${rule.placement ?? ""})`, `${rule.text ?? ""}\nFixes: ${rule.fixes ?? ""}`, at);
  });
}

function tomlDocs(path: string, at: number): Doc[] {
  const ref = display(path);
  const parts: [string, string][] = [];
  let heading = basename(path);
  let body: string[] = [];
  for (const line of read(path).split("\n")) {
    if (/^\s*\[\[?[^\]]+\]\]?\s*$/.test(line)) {
      if (body.join("").trim()) parts.push([heading, body.join("\n")]);
      heading = line.trim();
      body = [];
    } else body.push(line);
  }
  if (body.join("").trim()) parts.push([heading, body.join("\n")]);
  const out: Doc[] = [];
  parts.forEach(([h, section], i) => {
    for (let s = 0; s < Math.max(section.length, 1); s += SECTION)
      out.push(doc(`${ref}#${i}.${s}`, ref, `${basename(path)} ${h}`, section.slice(s, s + SECTION), at));
  });
  return out;
}

function ownersDocs(path: string, at: number): Doc[] {
  let data: { guard?: Record<string, unknown>[] };
  try {
    data = Bun.TOML.parse(read(path)) as typeof data;
  } catch {
    return [];
  }
  const hooks = join(dirname(path), "hooks");
  return (data.guard ?? []).map((g) => {
    const command = String(g.command ?? "");
    const source = join(hooks, command);
    const header = existsSync(source) ? codeHeader(read(source), source) : "";
    const events = [...((g.claude as string[]) ?? []), ...((g.codex as string[]) ?? []).map((e) => `codex ${e}`)].join(", ");
    return doc(`${display(path)}#${g.unit}`, display(source), `hook ${g.unit} (${events})`,
      `${header}\n${g.claude_absent ?? ""} ${g.codex_absent ?? ""}`, at);
  });
}

export function fileDocs(path: string, label: string, mentions: Map<string, string[]> = new Map()): Doc[] {
  let st;
  try {
    st = statSync(path);
  } catch {
    return [];
  }
  const at = st.mtimeMs / 1000;
  const name = basename(path);
  const ext = extname(path);
  if (name === "rules.toml") return rulesDocs(path, at);
  if (name === "policy-owners.toml") return [...ownersDocs(path, at), ...tomlDocs(path, at)];
  if (ext === ".md" || ext === ".txt" || ext === ".yml" || ext === ".yaml") return markdownDocs(path, label, at);
  if (ext === ".toml") return tomlDocs(path, at);
  if (CODE.has(ext) && st.size < 2_000_000) {
    const text = read(path);
    if (ext === "" && !text.startsWith("#!")) return [];
    const said = mentions.get(path) ?? [];
    const hook = /\/hooks\/[^/]+\.sh$/.test(path);
    const body = hook ? [text.slice(0, 4000), said.length ? `Mentioned in:\n${said.join("\n")}` : ""].join("\n") : codeHeader(text, path, said);
    return [doc(display(path), display(path), label, body, at)];
  }
  return [];
}

export function repoDocs(repo: string, prefix = ".", mentions?: Map<string, string[]>, files = tracked(repo, prefix)): Doc[] {
  const out: Doc[] = [];
  const name = basename(dirname(repo));
  for (const path of files) {
    try {
      if (lstatSync(path).isSymbolicLink() || !lstatSync(path).isFile()) continue;
    } catch {
      continue;
    }
    out.push(...fileDocs(path, `${name}:${relative(repo, path)}`, mentions));
  }
  return out;
}

function handoffDocs(): Doc[] {
  const out: Doc[] = [];
  let names: string[] = [];
  try {
    names = readdirSync(HANDOFFS);
  } catch {
    return out;
  }
  for (const name of names) {
    if (!/(brief|protocol)/i.test(name) || !/\.(md|txt)$/.test(name)) continue;
    const path = join(HANDOFFS, name);
    try {
      if (!statSync(path).isFile()) continue;
      out.push(...markdownDocs(path, `handoff ${name}`, statSync(path).mtimeMs / 1000, true));
    } catch {
      continue;
    }
  }
  return out;
}

export function* iterDocs(_since: number | null): Iterable<Doc> {
  const northFiles = tracked(NORTH);
  const southFiles = tracked(SOUTH, "agents");
  const isCode = (path: string) => CODE.has(extname(path)) && (extname(path) !== "" || /\/bin\//.test(path));
  const mentions = mentionIndex(
    [{ root: NORTH, files: northFiles.filter(isCode) }, { root: SOUTH, files: southFiles.filter(isCode) }],
    [{ root: NORTH, files: northFiles }, { root: SOUTH, files: southFiles }]);
  yield* repoDocs(NORTH, ".", mentions, northFiles);
  yield* repoDocs(SOUTH, "agents", mentions, southFiles);
  yield* handoffDocs();
}
