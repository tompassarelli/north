import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { ext, SOURCE_LANG, Source, ToolError, withTempTree } from "./files";

export interface Fn { name: string; line: number; lines: number; complexity: number }

const LOGICAL = String.raw`'^(&&|\|\||\?\?)$'`;

const JS_KINDS = {
  unit: ["function_declaration", "generator_function_declaration", "method_definition"],
  scope: ["class_declaration"],
  holder: ["variable_declarator", "pair"],
  value: ["arrow_function", "function_expression", "generator_function"],
};

function jsRules(lang: string, extraScope: string[], fieldKind: string): string {
  const any = (kinds: string[]) => kinds.map((k) => `{ kind: ${k} }`).join(", ");
  return `id: unit
language: ${lang}
rule:
  any: [${any(JS_KINDS.unit)}]
  has: { field: name, pattern: $N }
---
id: scope
language: ${lang}
rule:
  any: [${any([...JS_KINDS.scope, ...extraScope])}]
  has: { field: name, pattern: $N }
---
id: unit
language: ${lang}
rule:
  all:
    - any: [${any([...JS_KINDS.holder, fieldKind])}]
    - has: { field: value, any: [${any(JS_KINDS.value)}] }
    - any: [{ has: { field: name, pattern: $N } }, { has: { field: key, pattern: $N } }, { has: { field: property, pattern: $N } }]
---
id: branch
language: ${lang}
rule:
  any:
    - { kind: parenthesized_expression, inside: { any: [{ kind: if_statement }, { kind: while_statement }, { kind: do_statement }], field: condition } }
    - { kind: for_statement }
    - { kind: for_in_statement }
    - { kind: ternary_expression }
    - { kind: catch_clause }
    - { kind: switch_case }
    - { kind: binary_expression, has: { field: operator, regex: ${LOGICAL} } }
`;
}

const RULES: Record<string, string> = {
  TypeScript: jsRules("TypeScript", ["abstract_class_declaration"], "public_field_definition"),
  Tsx: jsRules("Tsx", ["abstract_class_declaration"], "public_field_definition"),
  JavaScript: jsRules("JavaScript", [], "field_definition"),
  Rust: `id: unit
language: Rust
rule:
  kind: function_item
  has: { field: name, pattern: $N }
---
id: scope
language: Rust
rule:
  any:
    - { kind: impl_item, has: { field: type, pattern: $N } }
    - { kind: trait_item, has: { field: name, pattern: $N } }
    - { kind: mod_item, has: { field: name, pattern: $N } }
---
id: branch
language: Rust
rule:
  any:
    - { kind: if_expression }
    - { kind: while_expression }
    - { kind: for_expression }
    - { kind: match_arm }
    - { kind: binary_expression, has: { field: operator, regex: ${LOGICAL} } }
`,
  C: `id: unit
language: C
rule:
  kind: function_definition
  has:
    field: declarator
    any:
      - { kind: function_declarator, has: { field: declarator, pattern: $N } }
      - { kind: pointer_declarator, has: { field: declarator, kind: function_declarator, has: { field: declarator, pattern: $N } } }
---
id: branch
language: C
rule:
  any:
    - { kind: parenthesized_expression, inside: { any: [{ kind: if_statement }, { kind: while_statement }, { kind: do_statement }], field: condition } }
    - { kind: for_statement }
    - { kind: conditional_expression }
    - { kind: case_statement, has: { field: value, pattern: $V } }
    - { kind: binary_expression, has: { field: operator, regex: ${LOGICAL} } }
`,
};

interface Match { ruleId: string; file: string; start: number; end: number; line: number; endLine: number; name?: string }

export function astGrepAvailable(): boolean {
  return Bun.which("ast-grep") !== null;
}

function scan(root: string, langs: Set<string>): Match[] {
  const rulesFile = join(root, ".rules.yml");
  writeFileSync(rulesFile, [...langs].map((l) => RULES[l]).join("---\n"));
  const r = Bun.spawnSync(
    ["ast-grep", "scan", "-r", rulesFile, "--json=stream", "--no-ignore", "hidden", "--no-ignore", "vcs", "--no-ignore", "dot", "--no-ignore", "parent", "--no-ignore", "global", "--no-ignore", "exclude", root],
    { stdout: "pipe", stderr: "pipe" },
  );
  if (r.exitCode !== 0 && r.exitCode !== 1) throw new ToolError(`ast-grep: ${r.stderr.toString().trim()}`);
  const out: Match[] = [];
  for (const line of r.stdout.toString().split("\n")) {
    if (!line) continue;
    const m = JSON.parse(line);
    out.push({
      ruleId: m.ruleId, file: m.file, start: m.range.byteOffset.start, end: m.range.byteOffset.end,
      line: m.range.start.line + 1, endLine: m.range.end.line + 1, name: m.metaVariables?.single?.N?.text,
    });
  }
  return out;
}

function assemble(matches: Match[]): Fn[] {
  const containers = matches.filter((m) => m.ruleId !== "branch").sort((a, b) => a.start - b.start || b.end - a.end);
  const fns: (Fn & { start: number; end: number })[] = [];
  const stack: { m: Match; qname: string; fn?: Fn & { start: number; end: number } }[] = [];
  const seen = new Map<string, number>();
  for (const m of containers) {
    while (stack.length && stack[stack.length - 1].m.end <= m.start) stack.pop();
    const parent = stack[stack.length - 1];
    if (parent && parent.m.start === m.start && parent.m.end === m.end) continue;
    let qname = parent ? `${parent.qname}.${m.name ?? "?"}` : (m.name ?? "?");
    const n = (seen.get(qname) ?? 0) + 1;
    seen.set(qname, n);
    if (n > 1) qname = `${qname}#${n}`;
    const entry: (typeof stack)[number] = { m, qname };
    if (m.ruleId === "unit") {
      entry.fn = { name: qname, line: m.line, lines: m.endLine - m.line + 1, complexity: 1, start: m.start, end: m.end };
      fns.push(entry.fn);
    }
    stack.push(entry);
  }
  const branches = matches.filter((m) => m.ruleId === "branch").sort((a, b) => a.start - b.start);
  const byStart = [...fns].sort((a, b) => a.start - b.start);
  for (const b of branches) {
    let best: (typeof byStart)[number] | undefined;
    for (const f of byStart) {
      if (f.start > b.start) break;
      if (f.end >= b.end && (!best || f.start >= best.start)) best = f;
    }
    if (best) best.complexity++;
  }
  return fns.map(({ name, line, lines, complexity }) => ({ name, line, lines, complexity }));
}

export function measure(groups: Record<string, Source[]>): Map<string, Fn[]> {
  const langs = new Set<string>();
  for (const files of Object.values(groups)) for (const f of files) langs.add(SOURCE_LANG[ext(f.path)]);
  const result = new Map<string, Fn[]>();
  for (const [prefix, files] of Object.entries(groups)) for (const f of files) result.set(`${prefix}/${f.path}`, []);
  if (langs.size === 0) return result;
  return withTempTree(groups, (root) => {
    const byFile = new Map<string, Match[]>();
    for (const m of scan(root, langs)) {
      const rel = m.file.startsWith(root) ? m.file.slice(root.length + 1) : m.file;
      if (!byFile.has(rel)) byFile.set(rel, []);
      byFile.get(rel)!.push(m);
    }
    for (const [rel, ms] of byFile) if (result.has(rel)) result.set(rel, assemble(ms));
    return result;
  });
}
