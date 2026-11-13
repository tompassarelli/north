use super::rust_text::mask;
use super::snapshot::{git, line_count};
use regex::Regex;
use std::io::Write;
use std::num::NonZero;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::LazyLock;
use std::thread;

static CFG_TEST: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]").expect("cfg(test) attribute pattern"));
static MOD_HEADER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+[A-Za-z_][A-Za-z0-9_]*\s*\z").expect("inline module header pattern"));

const ALGORITHM: &str = "cut-format-1";
const FORMAT_ARGS: [&str; 4] = ["--edition", "2024", "--emit", "stdout"];

pub(super) fn count(cache_root: &Path, repo: &Path, rev: &str, keep: impl Fn(&str) -> bool) -> Result<usize, String> {
    let listing = git(repo, &["ls-tree", "-r", "-z", "--full-tree", rev])?;
    let blobs: Vec<String> = listing
        .split('\0')
        .filter_map(|entry| {
            let (meta, path) = entry.split_once('\t')?;
            let mut fields = meta.split(' ').skip(1);
            let (kind, blob) = (fields.next()?, fields.next()?);
            (kind == "blob" && path.ends_with(".rs") && keep(path)).then(|| blob.to_owned())
        })
        .collect();
    let cache = Cache::open(cache_root)?;
    let jobs = thread::available_parallelism().map_or(1, NonZero::get);
    let per_job = blobs.len().div_ceil(jobs).max(1);
    thread::scope(|scope| {
        let workers: Vec<_> = blobs
            .chunks(per_job)
            .map(|part| scope.spawn(|| part.iter().map(|blob| cache.lines(repo, blob)).sum::<Result<usize, String>>()))
            .collect();
        workers.into_iter().map(|worker| worker.join().unwrap_or_else(|_| Err("a Rust line worker panicked".to_owned()))).sum::<Result<usize, String>>()
    })
}

struct Cache {
    dir: PathBuf,
}

impl Cache {
    fn open(root: &Path) -> Result<Self, String> {
        let dir = root.join(format!("{ALGORITHM}-{}", file_name_safe(&rustfmt_version()?)));
        std::fs::create_dir_all(&dir).map_err(|error| format!("{}: {error}", dir.display()))?;
        Ok(Self { dir })
    }

    fn lines(&self, repo: &Path, blob: &str) -> Result<usize, String> {
        let entry = self.dir.join(blob);
        if let Some(lines) = std::fs::read_to_string(&entry).ok().and_then(|text| text.trim().parse().ok()) {
            return Ok(lines);
        }
        let source = git(repo, &["cat-file", "blob", blob])?;
        let lines = line_count(&normalised(&without_inline_test_modules(&source), &self.dir));
        std::fs::write(&entry, format!("{lines}\n")).map_err(|error| format!("{}: {error}", entry.display()))?;
        Ok(lines)
    }
}

fn rustfmt_version() -> Result<String, String> {
    let output = Command::new("rustfmt").arg("--version").output().map_err(|error| format!("cannot run rustfmt for the Rust line budget: {error}"))?;
    if !output.status.success() {
        return Err(format!("rustfmt --version failed: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn file_name_safe(text: &str) -> String {
    text.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' }).collect()
}

fn without_inline_test_modules(text: &str) -> String {
    let code = mask(text);
    let mut kept = String::with_capacity(text.len());
    let mut from = 0;
    for attribute in CFG_TEST.find_iter(&code) {
        if attribute.start() < from {
            continue;
        }
        let Some(end) = inline_module_end(&code, attribute.end()) else { continue };
        let start = cut_start(text, attribute.start());
        kept.push_str(&text[from..start]);
        from = cut_end(text, end);
    }
    kept.push_str(&text[from..]);
    kept
}

fn inline_module_end(code: &str, from: usize) -> Option<usize> {
    let rest = &code[from..];
    let open = rest.find(['{', ';'])?;
    if !rest[open..].starts_with('{') || !MOD_HEADER.is_match(&rest[..open]) {
        return None;
    }
    let start = from + open;
    let mut depth = 0usize;
    for (offset, byte) in code.as_bytes()[start..].iter().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(start + offset + 1);
                }
            }
            _ => {}
        }
    }
    None
}

fn cut_start(text: &str, at: usize) -> usize {
    let line = text[..at].rfind('\n').map_or(0, |newline| newline + 1);
    if text[line..at].trim().is_empty() { line } else { at }
}

fn cut_end(text: &str, at: usize) -> usize {
    let line = text[at..].find('\n').map_or(text.len(), |newline| at + newline + 1);
    if text[at..line].trim().is_empty() { line } else { at }
}

fn normalised(source: &str, cwd: &Path) -> String {
    rustfmt(source, cwd).unwrap_or_else(|| source.to_owned())
}

fn rustfmt(source: &str, cwd: &Path) -> Option<String> {
    let mut child = Command::new("rustfmt").args(FORMAT_ARGS).current_dir(cwd).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let fed = child.stdin.take()?.write_all(source.as_bytes()).is_ok();
    let output = child.wait_with_output().ok()?;
    (fed && output.status.success()).then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}
