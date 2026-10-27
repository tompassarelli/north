use super::rust_modules::{Crate, expand_use, inline_modules, resolve_rust};
use super::source::{Source, dirname};
use regex::Regex;
use std::collections::BTreeSet;
use std::sync::LazyLock;

const TS_EXTS: &[&str] = &[
    "", ".ts", ".tsx", ".mts", ".cts", ".js", ".mjs", ".cjs", ".jsx", "/index.ts", "/index.tsx",
    "/index.js", "/index.mjs",
];

static TS_FROM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?:^|[\s}])from\s*['"]([^'"]+)['"]"#).unwrap());
static TS_BARE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?m)^\s*import\s*['"]([^'"]+)['"]"#).unwrap());
static TS_CALL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?-u:\b)(?:require|import)\s*\(\s*['"]([^'"]+)['"]\s*\)"#).unwrap());
static RS_USE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?use\s+([^;]+);").unwrap());
static C_INCLUDE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?m)^\s*#\s*include\s*"([^"]+)""#).unwrap());
static BLOCK_COMMENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)/\*.*?\*/").unwrap());
static LINE_COMMENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\s*//[^\r\n\u{2028}\u{2029}]*").unwrap());
static TS_STEM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(.*)\.(?:js|mjs|cjs|jsx)$").unwrap());

fn extension(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rfind('.') {
        Some(dot) if dot > 0 => &name[dot + 1..],
        _ => "",
    }
}

fn join_normalized(directory: &str, spec: &str) -> String {
    let joined = format!("{directory}/{spec}");
    let absolute = joined.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for part in joined.split('/') {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|last| *last != "..") => {
                parts.pop();
            }
            ".." if absolute => {}
            _ => parts.push(part),
        }
    }
    let mut out = parts.join("/");
    if absolute {
        out.insert(0, '/');
    }
    if out.is_empty() {
        out.push('.');
    }
    if joined.ends_with('/') && !out.ends_with('/') {
        out.push('/');
    }
    out
}

fn strip_block_comments(text: &str) -> String {
    BLOCK_COMMENT.replace_all(text, "").into_owned()
}

fn strip_line_comments(text: &str) -> String {
    LINE_COMMENT.replace_all(text, "").into_owned()
}

fn resolve_ts(from: &str, spec: &str, source: &Source) -> Option<String> {
    if !spec.starts_with('.') {
        return None;
    }
    let base = join_normalized(dirname(from), spec);
    if base.starts_with("..") {
        return None;
    }
    let mut stems = vec![base.clone()];
    if let Some(found) = TS_STEM.captures(&base) {
        stems.push(found[1].to_owned());
    }
    stems.iter().find_map(|stem| {
        TS_EXTS
            .iter()
            .map(|suffix| format!("{stem}{suffix}"))
            .find(|candidate| source.exists(candidate))
    })
}

fn resolve_c(from: &str, spec: &str, source: &Source) -> Option<String> {
    let local = join_normalized(dirname(from), spec);
    if source.exists(&local) {
        return Some(local);
    }
    source.exists(spec).then(|| spec.to_owned())
}

pub(super) fn imports_of(path: &str, text: &str, source: &Source, crates: &[Crate]) -> Vec<String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut found: Vec<String> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut add = |target: Option<String>| {
        if let Some(target) = target.filter(|target| target != path && seen.insert(target.clone())) {
            found.push(target);
        }
    };
    match extension(path) {
        "rs" => {
            let code = strip_line_comments(&strip_block_comments(text));
            for found_use in RS_USE.captures_iter(&code) {
                let start = found_use.get(0).map_or(0, |whole| whole.start());
                let inline = inline_modules(&code, start);
                for segments in expand_use(&found_use[1]) {
                    add(resolve_rust(path, &segments, source, crates, &inline));
                }
            }
        }
        "c" | "h" => {
            for include in C_INCLUDE.captures_iter(text) {
                add(resolve_c(path, &include[1], source));
            }
        }
        _ => {
            let code = strip_line_comments(text);
            for pattern in [&TS_FROM, &TS_BARE, &TS_CALL] {
                for specifier in pattern.captures_iter(&code) {
                    add(resolve_ts(path, &specifier[1], source));
                }
            }
        }
    }
    found
}
