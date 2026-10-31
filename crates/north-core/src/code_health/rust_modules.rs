use super::source::{Source, dirname};
use regex::Regex;
use std::sync::LazyLock;
use toml_edit::DocumentMut;

static AS_CLAUSE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+as\s+").unwrap());
static INLINE_MOD_TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#""(?:[^"\\]|\\.)*"|(?-u:\b)mod\s+([A-Za-z0-9_]+)\s*\{|\{|\}"#).unwrap()
});

pub(in crate::code_health) struct Crate {
    name: String,
    dir: String,
}

impl Crate {
    pub(in crate::code_health) fn from_manifests<'a>(
        manifests: impl Iterator<Item = (&'a str, &'a str)>,
    ) -> Vec<Self> {
        let mut crates: Vec<Self> = manifests
            .filter_map(|(path, text)| {
                let dir = if path == "Cargo.toml" {
                    ""
                } else {
                    path.strip_suffix("/Cargo.toml")?
                };
                let document = text.parse::<DocumentMut>().ok()?;
                let name = document.get("package")?.get("name")?.as_str()?;
                (!name.is_empty()).then(|| Self {
                    name: name.replace('-', "_"),
                    dir: dir.to_owned(),
                })
            })
            .collect();
        crates.sort_by_key(|krate| std::cmp::Reverse(krate.dir.len()));
        crates
    }

    fn src(&self) -> String {
        if self.dir.is_empty() { "src".into() } else { format!("{}/src", self.dir) }
    }
}

struct Location {
    root: String,
    module: Vec<String>,
}

pub(super) fn expand_use(tree: &str) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    walk_use(&[], tree, &mut out);
    out
}

fn segments(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split("::").map(str::trim).filter(|part| !part.is_empty()).map(str::to_owned)
}

fn walk_use(prefix: &[String], text: &str, out: &mut Vec<Vec<String>>) {
    let text = text.trim();
    let Some(brace) = text.find('{') else {
        let path = AS_CLAUSE.split(text).next().unwrap_or_default();
        let leading = prefix.is_empty();
        let tail = segments(path)
            .enumerate()
            .filter(|(at, part)| part != "*" && (part != "self" || (leading && *at == 0)))
            .map(|(_, part)| part);
        out.push(prefix.iter().cloned().chain(tail).collect());
        return;
    };
    let head: Vec<String> = prefix.iter().cloned().chain(segments(&text[..brace])).collect();
    let end = text.rfind('}').unwrap_or_else(|| {
        text.char_indices().next_back().map_or(0, |(at, _)| at)
    });
    let inner = text.get(brace + 1..end).unwrap_or("");
    let mut depth = 0i32;
    let mut start = 0;
    let mut cut = |until: usize, next: usize| {
        let piece = &inner[start..until];
        if !piece.trim().is_empty() {
            walk_use(&head, piece, out);
        }
        start = next;
    };
    for (at, character) in inner.char_indices() {
        match character {
            '{' => depth += 1,
            '}' => depth -= 1,
            ',' if depth == 0 => cut(at, at + 1),
            _ => {}
        }
    }
    cut(inner.len(), inner.len());
}

pub(super) fn inline_modules(text: &str, until: usize) -> Vec<String> {
    let mut stack: Vec<Option<String>> = Vec::new();
    for token in INLINE_MOD_TOKEN.captures_iter(text) {
        let whole = token.get(0).expect("whole match");
        if whole.start() >= until {
            break;
        }
        match whole.as_str() {
            "}" => {
                stack.pop();
            }
            "{" => stack.push(None),
            _ => {
                if let Some(name) = token.get(1) {
                    stack.push(Some(name.as_str().to_owned()));
                }
            }
        }
    }
    stack.into_iter().flatten().collect()
}

fn module_of(path: &str, krate: &Crate) -> Location {
    let relative = if krate.dir.is_empty() { path } else { &path[krate.dir.len() + 1..] };
    let root = krate.src();
    let Some(inside) = relative.strip_prefix("src/") else {
        return Location { root: dirname(path).to_owned(), module: Vec::new() };
    };
    let stem = inside.strip_suffix(".rs").unwrap_or(inside);
    let mut parts: Vec<&str> = stem.split('/').collect();
    let last = parts.last().copied().unwrap_or_default();
    if parts.len() == 1 && (last == "lib" || last == "main") {
        return Location { root, module: Vec::new() };
    }
    if last == "mod" {
        parts.pop();
    }
    Location { root, module: parts.into_iter().map(str::to_owned).collect() }
}

fn module_file(root: &str, module: &[String], source: &Source) -> Option<String> {
    if module.is_empty() {
        return ["lib.rs", "main.rs"]
            .iter()
            .map(|name| format!("{root}/{name}"))
            .find(|file| source.exists(file));
    }
    let base = format!("{root}/{}", module.join("/"));
    [format!("{base}.rs"), format!("{base}/mod.rs")]
        .into_iter()
        .find(|file| source.exists(file))
}

pub(super) fn resolve_rust(
    from: &str,
    segments: &[String],
    source: &Source,
    crates: &[Crate],
    inline: &[String],
) -> Option<String> {
    let own = crates
        .iter()
        .find(|krate| krate.dir.is_empty() || from.starts_with(&format!("{}/", krate.dir)))?;
    let here = module_of(from, own);
    let mut root = here.root.clone();
    let mut base: Vec<String>;
    let mut file_depth = 0;
    let mut rest = segments;
    match segments.first().map(String::as_str) {
        Some("crate") => {
            base = Vec::new();
            rest = &segments[1..];
            root = own.src();
        }
        Some("self") => {
            base = [here.module.clone(), inline.to_vec()].concat();
            file_depth = here.module.len();
            rest = &segments[1..];
        }
        Some("super") => {
            base = [here.module.clone(), inline.to_vec()].concat();
            file_depth = here.module.len();
            while rest.first().is_some_and(|first| first == "super") {
                base.pop();
                file_depth = file_depth.min(base.len());
                rest = &rest[1..];
            }
        }
        Some(name) => {
            let other = crates.iter().find(|krate| krate.name == name)?;
            root = other.src();
            base = Vec::new();
            rest = &segments[1..];
        }
        None => return None,
    }
    (0..=rest.len())
        .rev()
        .find_map(|k| module_file(&root, &[base.clone(), rest[..k].to_vec()].concat(), source))
        .or_else(|| module_file(&root, &base[..file_depth], source))
}
