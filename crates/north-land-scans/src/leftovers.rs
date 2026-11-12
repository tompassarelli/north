use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use super::{Range, git};

pub const USAGE: &str = "usage: north land scan leftovers BASE..HEAD";

const DECLARATIONS: &str = r#"(^\s*(#\[[^]]*\]\s*)*(pub(\([^)]*\))?\s+)?mod\s+[A-Za-z_#][A-Za-z0-9_]*\s*;)|path\s*=\s*"[^"]+\.rs"|include!"#;

static MOD_DECLARATION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?mod\s+(?:r#)?([A-Za-z_][A-Za-z0-9_]*)\s*;").expect("mod pattern"));
static PATH_ATTRIBUTE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\bpath\s*=\s*"([^"]+\.rs)""#).expect("path pattern"));
static INCLUDE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\binclude!\(\s*"([^"]+\.rs)""#).expect("include pattern"));

struct Tree {
    files: BTreeSet<String>,
    crates: BTreeSet<String>,
}

fn normalized(path: &Path) -> String {
    let mut parts: Vec<&std::ffi::OsStr> = Vec::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                parts.pop();
            }
            Component::Normal(part) => parts.push(part),
            _ => {}
        }
    }
    parts.iter().collect::<PathBuf>().to_string_lossy().into_owned()
}

fn parent(path: &str) -> &Path {
    Path::new(path).parent().unwrap_or(Path::new(""))
}

impl Tree {
    fn read(directory: &Path, revision: &str) -> Tree {
        let listing = git::text(directory, &["ls-tree", "-r", "--name-only", revision]).unwrap_or_default();
        let files: BTreeSet<String> = listing.lines().map(str::to_owned).collect();
        let crates = files.iter().filter_map(|file| file.strip_suffix("Cargo.toml").map(|root| root.trim_end_matches('/').to_owned())).collect();
        Tree { files, crates }
    }

    fn inner<'a>(&self, file: &'a str) -> Option<&'a str> {
        let path = Path::new(file);
        path.ancestors().skip(1).find(|ancestor| self.crates.contains(ancestor.to_string_lossy().as_ref())).and_then(|ancestor| path.strip_prefix(ancestor).ok()).and_then(Path::to_str)
    }

    fn root(&self, file: &str) -> bool {
        let Some(inner) = self.inner(file) else { return true };
        let parts: Vec<&str> = inner.split('/').collect();
        match parts.as_slice() {
            ["src", "lib.rs" | "main.rs"] | ["src", "bin", _] | ["src", "bin", _, "main.rs"] => true,
            ["tests" | "benches" | "examples", _] | ["tests" | "benches" | "examples", _, "main.rs"] => true,
            ["src" | "tests" | "benches" | "examples", ..] => false,
            _ => true,
        }
    }

    fn children_directory(&self, file: &str) -> String {
        let here = parent(file);
        let stem = Path::new(file).file_stem().unwrap_or_default().to_string_lossy();
        match stem.as_ref() {
            "lib" | "main" | "mod" => normalized(here),
            _ if self.root(file) => normalized(here),
            _ => normalized(&here.join(stem.as_ref())),
        }
    }

    fn declared(&self, directory: &Path, revision: &str) -> Declared {
        let grep = git::text(directory, &["grep", "--null", "-n", "-E", DECLARATIONS, revision, "--", "*.rs", "*Cargo.toml"]).unwrap_or_default();
        let mut found = Declared::default();
        for line in grep.lines() {
            let mut fields = line.splitn(3, '\0');
            let (Some(named), Some(_), Some(text)) = (fields.next(), fields.next(), fields.next()) else { continue };
            let file = named.split_once(':').map_or(named, |(_, file)| file);
            let explicit: Vec<String> = PATH_ATTRIBUTE.captures_iter(text).chain(INCLUDE.captures_iter(text)).map(|target| normalized(&parent(file).join(&target[1]))).collect();
            match (explicit.is_empty(), MOD_DECLARATION.captures(text)) {
                (true, Some(module)) => {
                    found.modules.insert((self.children_directory(file), module[1].to_owned()));
                }
                _ => found.paths.extend(explicit),
            }
        }
        found
    }

    fn orphans(&self, directory: &Path, revision: &str) -> BTreeSet<String> {
        let declared = self.declared(directory, revision);
        self.files.iter().filter(|file| file.ends_with(".rs") && !self.root(file) && !declared.includes(file)).cloned().collect()
    }
}

#[derive(Default)]
struct Declared {
    paths: BTreeSet<String>,
    modules: BTreeSet<(String, String)>,
}

impl Declared {
    fn includes(&self, file: &str) -> bool {
        let path = Path::new(file);
        let (name, home) = match path.file_stem().and_then(|stem| stem.to_str()) {
            Some("mod") => (path.parent().and_then(Path::file_name).and_then(|name| name.to_str()), path.parent().and_then(Path::parent)),
            stem => (stem, path.parent()),
        };
        let Some(name) = name else { return true };
        self.paths.contains(file) || home.into_iter().flat_map(Path::ancestors).any(|above| self.modules.contains(&(normalized(above), name.to_owned())))
    }
}

pub fn refusals(directory: &Path, base: &str, head: &str) -> Vec<String> {
    let fork = git::merge_base(directory, base, head);
    let before = Tree::read(directory, &fork).orphans(directory, &fork);
    Tree::read(directory, head)
        .orphans(directory, head)
        .difference(&before)
        .map(|file| format!("leftovers: {file}:1 is a module file no `mod` declaration includes, so nothing compiles it; add its `mod` line or delete the file\n"))
        .collect()
}

pub fn scan(directory: &Path, range: &str, out: &mut impl Write) -> u8 {
    let Some(Range { base, head }) = Range::parse(range) else {
        let _ = writeln!(out, "{USAGE}");
        return 2;
    };
    let refused = refusals(directory, base, head);
    for refusal in &refused {
        let _ = out.write_all(refusal.as_bytes());
    }
    u8::from(!refused.is_empty())
}
