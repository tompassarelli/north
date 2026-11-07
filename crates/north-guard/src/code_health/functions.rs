use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};

use north_base::platform::ast_grep::{self, Match};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct Function {
    pub(super) name: String,
    pub(super) line: usize,
    pub(super) lines: usize,
    pub(super) complexity: usize,
}

pub(super) struct SourceText {
    pub(super) path: String,
    pub(super) text: std::rc::Rc<str>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Language {
    TypeScript,
    Tsx,
    JavaScript,
    Rust,
    C,
}

pub(super) fn language(path: &str) -> Option<Language> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let extension = match name.rfind('.') {
        Some(dot) if dot > 0 => &name[dot + 1..],
        _ => return None,
    };
    Some(match extension {
        "ts" | "mts" | "cts" => Language::TypeScript,
        "tsx" => Language::Tsx,
        "js" | "mjs" | "cjs" | "jsx" => Language::JavaScript,
        "rs" => Language::Rust,
        "c" | "h" => Language::C,
        _ => return None,
    })
}

fn logical_runs() -> String {
    [r"&&", r"\|\|", r"\?\?"]
        .iter()
        .map(|op| {
            let node = format!("{{ kind: binary_expression, has: {{ field: operator, regex: '^{op}$' }} }}");
            format!("    - {{ all: [{node}, {{ not: {{ inside: {{ all: [{node}], stopBy: {{ not: {{ kind: parenthesized_expression }} }} }} }} }}] }}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn kinds(kinds: &[&str]) -> String {
    kinds.iter().map(|kind| format!("{{ kind: {kind} }}")).collect::<Vec<_>>().join(", ")
}

fn js_rules(language: &str, extra_scope: &[&str], field_kind: &str) -> String {
    let units = kinds(&["function_declaration", "generator_function_declaration", "method_definition"]);
    let scopes = kinds(&[&["class_declaration"][..], extra_scope].concat());
    let holders = kinds(&["variable_declarator", "pair", field_kind]);
    let values = kinds(&["arrow_function", "function_expression", "generator_function"]);
    let runs = logical_runs();
    format!(
        "id: unit\nlanguage: {language}\nrule:\n  any: [{units}]\n  has: {{ field: name, pattern: $N }}\n---\nid: scope\nlanguage: {language}\nrule:\n  any: [{scopes}]\n  has: {{ field: name, pattern: $N }}\n---\nid: unit\nlanguage: {language}\nrule:\n  all:\n    - any: [{holders}]\n    - has: {{ field: value, any: [{values}] }}\n    - any: [{{ has: {{ field: name, pattern: $N }} }}, {{ has: {{ field: key, pattern: $N }} }}, {{ has: {{ field: property, pattern: $N }} }}]\n---\nid: branch\nlanguage: {language}\nrule:\n  any:\n    - {{ kind: parenthesized_expression, inside: {{ any: [{{ kind: if_statement }}, {{ kind: while_statement }}, {{ kind: do_statement }}], field: condition }} }}\n    - {{ kind: for_statement }}\n    - {{ kind: for_in_statement }}\n    - {{ kind: ternary_expression }}\n    - {{ kind: catch_clause }}\n    - {{ kind: switch_case }}\n{runs}\n"
    )
}

fn rules(language: Language) -> String {
    let runs = logical_runs();
    match language {
        Language::TypeScript => js_rules("TypeScript", &["abstract_class_declaration"], "public_field_definition"),
        Language::Tsx => js_rules("Tsx", &["abstract_class_declaration"], "public_field_definition"),
        Language::JavaScript => js_rules("JavaScript", &[], "field_definition"),
        Language::Rust => format!("id: unit\nlanguage: Rust\nrule:\n  kind: function_item\n  has: {{ field: name, pattern: $N }}\n---\nid: scope\nlanguage: Rust\nrule:\n  any:\n    - {{ kind: impl_item, has: {{ field: type, pattern: $N }} }}\n    - {{ kind: trait_item, has: {{ field: name, pattern: $N }} }}\n    - {{ kind: mod_item, has: {{ field: name, pattern: $N }} }}\n---\nid: branch\nlanguage: Rust\nrule:\n  any:\n    - {{ kind: if_expression }}\n    - {{ kind: while_expression }}\n    - {{ kind: for_expression }}\n    - {{ kind: match_arm }}\n{runs}\n"),
        Language::C => format!("id: unit\nlanguage: C\nrule:\n  kind: function_definition\n  has:\n    field: declarator\n    any:\n      - {{ kind: function_declarator, has: {{ field: declarator, pattern: $N }} }}\n      - {{ kind: pointer_declarator, has: {{ field: declarator, kind: function_declarator, has: {{ field: declarator, pattern: $N }} }} }}\n---\nid: branch\nlanguage: C\nrule:\n  any:\n    - {{ kind: parenthesized_expression, inside: {{ any: [{{ kind: if_statement }}, {{ kind: while_statement }}, {{ kind: do_statement }}], field: condition }} }}\n    - {{ kind: for_statement }}\n    - {{ kind: conditional_expression }}\n    - {{ kind: case_statement, has: {{ field: value, pattern: $V }} }}\n{runs}\n"),
    }
}

pub(super) fn ast_grep_available() -> bool {
    ast_grep::available()
}

fn scan(files: &[ast_grep::File], languages: &BTreeSet<Language>) -> Result<Vec<Match>, String> {
    ast_grep::scan(files, &languages.iter().map(|language| rules(*language)).collect::<Vec<_>>().join("---\n"))
}

struct Unit {
    function: Function,
    start: usize,
    end: usize,
}

fn assemble(matches: &[Match]) -> Vec<Function> {
    let mut containers: Vec<&Match> = matches.iter().filter(|found| found.rule != "branch").collect();
    containers.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
    let mut units: Vec<Unit> = Vec::new();
    let mut stack: Vec<(&Match, String)> = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    for found in containers {
        while stack.last().is_some_and(|(top, _)| top.end <= found.start) {
            stack.pop();
        }
        let parent = stack.last();
        if parent.is_some_and(|(top, _)| top.start == found.start && top.end == found.end) {
            continue;
        }
        let name = found.single.get("N").map_or("?", String::as_str);
        let mut qualified = parent.map_or_else(|| name.to_owned(), |(_, qualified)| format!("{qualified}.{name}"));
        let count = seen.entry(qualified.clone()).or_insert(0);
        *count += 1;
        if *count > 1 {
            qualified = format!("{qualified}#{count}");
        }
        if found.rule == "unit" {
            units.push(Unit { function: Function { name: qualified.clone(), line: found.line, lines: found.end_line - found.line + 1, complexity: 1 }, start: found.start, end: found.end });
        }
        stack.push((found, qualified));
    }
    let mut branches: Vec<&Match> = matches.iter().filter(|found| found.rule == "branch").collect();
    branches.sort_by_key(|found| found.start);
    let mut by_start: Vec<usize> = (0..units.len()).collect();
    by_start.sort_by_key(|&index| units[index].start);
    for branch in branches {
        let mut best: Option<usize> = None;
        for &index in &by_start {
            let unit = &units[index];
            if unit.start > branch.start {
                break;
            }
            if unit.end >= branch.end && best.is_none_or(|best| unit.start >= units[best].start) {
                best = Some(index);
            }
        }
        if let Some(best) = best {
            units[best].function.complexity += 1;
        }
    }
    units.into_iter().map(|unit| unit.function).collect()
}

pub(super) fn measure(groups: &[(&str, &[SourceText])]) -> Result<BTreeMap<String, Vec<Function>>, String> {
    let mut result = BTreeMap::new();
    let mut languages = BTreeSet::new();
    for (prefix, files) in groups {
        for file in *files {
            languages.extend(language(&file.path));
            result.insert(format!("{prefix}/{}", file.path), Vec::new());
        }
    }
    if languages.is_empty() {
        return Ok(result);
    }
    let paths: Vec<String> = groups.iter().flat_map(|(prefix, files)| files.iter().map(move |file| format!("{prefix}/{}", file.path))).collect();
    let texts = groups.iter().flat_map(|(_, files)| files.iter().map(|file| &*file.text));
    let files: Vec<ast_grep::File> = paths.iter().zip(texts).map(|(path, text)| ast_grep::File { path, text }).collect();
    let mut by_file: BTreeMap<String, Vec<Match>> = BTreeMap::new();
    for found in scan(&files, &languages)? {
        by_file.entry(found.file.clone()).or_default().push(found);
    }
    for (relative, matches) in by_file {
        if let Some(slot) = result.get_mut(&relative) {
            *slot = assemble(&matches);
        }
    }
    Ok(result)
}
