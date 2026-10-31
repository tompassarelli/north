use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use crate::platform::gitleaks::{Finding, Scanner};
use crate::platform::git as platform_git;

#[path = "denylist_archive.rs"]
mod archive;

use archive::Unpacker;

type Staged = BTreeMap<PathBuf, BTreeMap<String, String>>;

const USAGE: &str = "usage: north denylist scan (--config FILE | --south DIR) BASE HEAD [--ref NAME]\n       north denylist attest --south DIR --repo OWNER/NAME [--repo ...] [--branch claude/X]";
const SOUTH_DENYLIST: &str = "origin/main:privacy/public-denylist.toml";
const IGNORE_FILE: &[u8] = b".gitleaksignore";
const NON_FILE_MODES: [&str; 2] = ["160000", "040000"];

#[derive(Debug, Eq, PartialEq)]
struct Request {
    config: Source,
    base: String,
    head: String,
    ref_name: Option<String>,
}

#[derive(Debug, Eq, PartialEq)]
enum Source {
    Config(String),
    South(String),
}

#[derive(Debug, Eq, PartialEq)]
struct Segment {
    label: String,
    text: String,
}

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Clone)]
struct Blob {
    path: Vec<u8>,
    oid: String,
    file: bool,
}

struct Gitleaks<'a> {
    config: &'a str,
    scratch: &'a Path,
    ignore: PathBuf,
}

fn parse(args: &[String]) -> Option<Request> {
    let (command, rest) = args.split_first()?;
    if command != "scan" {
        return None;
    }
    let (mut config, mut ref_name, mut revisions) = (None, None, Vec::new());
    let mut rest = rest.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--config" if config.is_none() => config = Some(Source::Config(rest.next()?.clone())),
            "--south" if config.is_none() => config = Some(Source::South(rest.next()?.clone())),
            "--ref" => ref_name = Some(rest.next()?.clone()),
            _ => revisions.push(arg.clone()),
        }
    }
    let [base, head] = <[String; 2]>::try_from(revisions).ok()?;
    Some(Request { config: config?, base, head, ref_name })
}

fn git_bytes(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let pushed_objects: Vec<&str> = std::iter::once("--no-replace-objects").chain(args.iter().copied()).collect();
    platform_git::stdout(root, &pushed_objects, |output| {
        format!(
            "git {} failed (exit {}): {}",
            args.iter().find(|arg| !arg.starts_with('-')).unwrap_or(&""),
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stderr).trim()
        )
    })
}

pub fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    git_bytes(root, args).map(|stdout| String::from_utf8_lossy(&stdout).into_owned())
}

fn commit(root: &Path, revision: &str) -> Result<String, String> {
    git(root, &["rev-parse", "--verify", "--end-of-options", &format!("{revision}^{{commit}}")])
        .map(|oid| oid.trim().to_owned())
}

fn commits(root: &Path, base: &str, head: &str) -> Result<Vec<String>, String> {
    let range = if base.is_empty() { head.to_owned() } else { format!("{base}..{head}") };
    Ok(git(root, &["rev-list", &range, "--"])?.lines().map(str::to_owned).collect())
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).replace('\0', "\n")
}

fn commit_segments(root: &Path, commits: &[String]) -> Result<Vec<Segment>, String> {
    commits
        .iter()
        .map(|oid| {
            let object = git_bytes(root, &["cat-file", "commit", oid])?;
            Ok(Segment { label: format!("commit {}", &oid[..oid.len().min(12)]), text: text(&object) })
        })
        .collect()
}

fn published_entries(raw: &[u8]) -> Vec<Blob> {
    let mut fields = raw.split(|byte| *byte == 0).filter(|field| !field.is_empty());
    let mut blobs = Vec::new();
    while let (Some(meta), Some(path)) = (fields.next(), fields.next()) {
        let meta = String::from_utf8_lossy(meta);
        let [_, new_mode, _, new_oid, status] = meta.split_whitespace().collect::<Vec<_>>()[..] else {
            continue;
        };
        if !status.starts_with('D') {
            let file = !NON_FILE_MODES.contains(&new_mode);
            blobs.push(Blob { path: path.to_vec(), oid: new_oid.to_owned(), file });
        }
    }
    blobs
}

fn published_blobs(root: &Path, commits: &[String]) -> Result<BTreeSet<Blob>, String> {
    let mut blobs = BTreeSet::new();
    for oid in commits {
        let raw = git_bytes(root, &["diff-tree", "-r", "-t", "-z", "-m", "--root", "--no-renames", "--no-commit-id", oid])?;
        blobs.extend(published_entries(&raw));
    }
    Ok(blobs)
}

fn nested(a: &[u8], b: &[u8]) -> bool {
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    long.starts_with(short) && (long.len() == short.len() || long[short.len()] == b'/')
}

fn layers(blobs: &BTreeSet<Blob>) -> Vec<Vec<&Blob>> {
    let mut layers: Vec<Vec<&Blob>> = Vec::new();
    for blob in blobs {
        match layers.iter_mut().find(|layer| layer.iter().all(|held| !nested(&held.path, &blob.path))) {
            Some(layer) => layer.push(blob),
            None => layers.push(vec![blob]),
        }
    }
    layers
}

fn archive_extension(content: &[u8]) -> Option<&'static str> {
    const SIGNATURES: [(&[u8], &str); 9] = [
        (b"PK\x03\x04", "zip"),
        (b"PK\x05\x06", "zip"),
        (b"PK\x07\x08", "zip"),
        (b"\x1f\x8b", "gz"),
        (b"BZh", "bz2"),
        (b"\xfd7zXZ\x00", "xz"),
        (b"\x28\xb5\x2f\xfd", "zst"),
        (b"7z\xbc\xaf\x27\x1c", "7z"),
        (b"Rar!\x1a\x07", "rar"),
    ];
    let tar = content.get(257..262) == Some(b"ustar".as_slice());
    SIGNATURES.iter().find(|(magic, _)| content.starts_with(magic)).map(|(_, ext)| *ext).or(tar.then_some("tar"))
}

fn is_text(content: &[u8]) -> bool {
    !content[..content.len().min(8000)].contains(&0)
}

fn neutral_name(path: &[u8], oid: &str) -> Vec<u8> {
    let short = &oid[..oid.len().min(12)];
    match path.iter().rposition(|byte| *byte == b'/') {
        Some(slash) => [&path[..=slash], format!("{short}.txt").as_bytes()].concat(),
        None => format!("{short}.txt").into_bytes(),
    }
}

fn put(file: &Path, bytes: &[u8], path: &[u8]) -> Result<(), String> {
    let written = file.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::write(file, bytes));
    written.map_err(|e| format!("cannot stage {} for the scan: {e}", String::from_utf8_lossy(path)))
}

fn staged_path(path: &[u8]) -> Result<&OsStr, String> {
    let unsafe_component = path.split(|byte| *byte == b'/').any(|part| matches!(part, b"" | b"." | b".."));
    if unsafe_component {
        return Err(format!("a tree entry has an unsafe path: {}", String::from_utf8_lossy(path)));
    }
    Ok(OsStr::from_bytes(path))
}

fn document(segments: &[Segment]) -> (String, Vec<usize>) {
    let (mut text, mut starts) = (String::new(), Vec::new());
    for segment in segments {
        starts.push(text.matches('\n').count() + 1);
        text.push_str(&segment.text);
        if !segment.text.ends_with('\n') {
            text.push('\n');
        }
    }
    (text, starts)
}

fn locate<'a>(segments: &'a [Segment], starts: &[usize], line: usize) -> Option<&'a str> {
    let index = starts.partition_point(|start| *start <= line).checked_sub(1)?;
    Some(segments[index].label.as_str())
}

impl Gitleaks<'_> {
    fn stage(&self, index: usize, staged: &mut Staged, path: &[u8], id: &str, depth: usize, content: &[u8]) -> Result<(), String> {
        let raw = self.scratch.join(format!("layer-{index}-{depth}-raw"));
        let text = self.scratch.join(format!("layer-{index}-{depth}-text"));
        put(&raw.join(staged_path(path)?), content, path)?;
        staged.entry(raw).or_default();
        if is_text(content) {
            let neutral = neutral_name(path, id);
            put(&text.join(staged_path(&neutral)?), &[b"\n".as_slice(), content].concat(), path)?;
            staged.entry(text).or_default().insert(String::from_utf8_lossy(&neutral).into_owned(), String::from_utf8_lossy(path).into_owned());
        }
        Ok(())
    }

    fn scan_layer(&self, index: usize, layer: &[&Blob], root: &Path) -> Result<Vec<String>, String> {
        let (mut staged, mut names) = (Staged::new(), Vec::new());
        for blob in layer.iter().filter(|blob| blob.path != IGNORE_FILE) {
            let content = git_bytes(root, &["cat-file", "blob", &blob.oid])?;
            let mut members = Vec::new();
            Unpacker::new(archive::DEPTH_CAP, archive::BYTE_CAP).members(&blob.path, &content, 0, &mut members)?;
            self.stage(index, &mut staged, &blob.path, &blob.oid, 0, &content)?;
            for member in members {
                let id = format!("{:012}", names.len());
                self.stage(index, &mut staged, &member.path, &id, member.depth, &member.content)?;
                names.push(String::from_utf8_lossy(&member.path).into_owned());
            }
        }
        let mut findings = Vec::new();
        if !names.is_empty() {
            let segment = Segment { label: "archive member names".into(), text: names.join("\n") };
            findings.extend(self.scan_text(&format!("layer-{index}-names"), &[segment])?);
        }
        for (tree, text_names) in &staged {
            for found in self.scan_dir(tree, false)? {
                let staged = found.file.trim_start_matches("./");
                findings.push(format!("{} in {}", found.rule, text_names.get(staged).map_or(staged, String::as_str)));
            }
        }
        Ok(findings)
    }

    fn scan_dir(&self, tree: &Path, strict: bool) -> Result<Vec<Finding>, String> {
        Scanner { config: self.config, ignore: &self.ignore }.dir(tree, strict)
    }

    fn scan_text(&self, name: &str, segments: &[Segment]) -> Result<Vec<String>, String> {
        let dir = self.scratch.join(name);
        let (text, starts) = document(segments);
        std::fs::create_dir_all(&dir)
            .and_then(|()| std::fs::write(dir.join("segments.txt"), format!("\n{text}")))
            .map_err(|e| format!("cannot stage the text for the scan: {e}"))?;
        self.scan_dir(&dir, true)?
            .into_iter()
            .map(|f| {
                let line = f.line.saturating_sub(1);
                let place = locate(segments, &starts, line).ok_or("a gitleaks finding is outside the scanned text")?;
                Ok(format!("{} in {place}", f.rule))
            })
            .collect()
    }
}

fn ref_segments(root: &Path, name: &str) -> Vec<Segment> {
    let mut segments = vec![Segment { label: "ref name".into(), text: name.to_owned() }];
    let mut object = format!("refs/tags/{}", name.trim_start_matches("refs/tags/"));
    while git(root, &["cat-file", "-t", &object]).is_ok_and(|kind| kind.trim() == "tag") {
        let Ok(annotation) = git_bytes(root, &["cat-file", "tag", &object]) else { break };
        let annotation = text(&annotation);
        let target = annotation.lines().next().and_then(|line| line.strip_prefix("object ")).map(str::to_owned);
        segments.push(Segment { label: "tag annotation".into(), text: annotation });
        let Some(target) = target else { break };
        object = target;
    }
    segments
}

fn text_segments(root: &Path, request: &Request, commits: &[String], blobs: &BTreeSet<Blob>) -> Result<Vec<Segment>, String> {
    let mut segments = commit_segments(root, commits)?;
    segments.extend(request.ref_name.iter().flat_map(|name| ref_segments(root, name)));
    let names: BTreeSet<String> = blobs.iter().map(|blob| String::from_utf8_lossy(&blob.path).into_owned()).collect();
    segments.push(Segment { label: "file names".into(), text: names.into_iter().collect::<Vec<_>>().join("\n") });
    for blob in blobs.iter().filter(|blob| blob.file && blob.path == IGNORE_FILE) {
        segments.push(Segment { label: ".gitleaksignore".into(), text: text(&git_bytes(root, &["cat-file", "blob", &blob.oid])?) });
    }
    Ok(segments)
}

pub fn south_denylist(south: &Path) -> Result<tempfile::NamedTempFile, String> {
    let main = south.join("main");
    git(&main, &["fetch", "-q", "origin", "main"])?;
    let rules = git_bytes(&main, &["show", SOUTH_DENYLIST])
        .map_err(|e| format!("the deny-list is missing from {} {SOUTH_DENYLIST}: {e}", main.display()))?;
    let mut file = tempfile::NamedTempFile::new().map_err(|e| format!("cannot create the deny-list copy: {e}"))?;
    std::io::Write::write_all(&mut file, &rules).map_err(|e| format!("cannot write the deny-list copy: {e}"))?;
    Ok(file)
}

pub fn scan_range(root: &Path, config: &Path, base: &str, head: &str, ref_name: Option<&str>) -> Result<BTreeSet<String>, String> {
    let request = Request {
        config: Source::Config(config.to_string_lossy().into_owned()),
        base: base.to_owned(),
        head: head.to_owned(),
        ref_name: ref_name.map(str::to_owned),
    };
    scan(root, &request)
}

fn scan(root: &Path, request: &Request) -> Result<BTreeSet<String>, String> {
    let held;
    let named = match &request.config {
        Source::Config(path) => PathBuf::from(path),
        Source::South(dir) => {
            held = south_denylist(Path::new(dir))?;
            held.path().to_path_buf()
        }
    };
    let config = std::fs::canonicalize(&named)
        .ok()
        .filter(|path| path.is_file())
        .ok_or_else(|| format!("the deny-list {} is missing", named.display()))?
        .to_string_lossy()
        .into_owned();
    let head = commit(root, &request.head)?;
    let base = if request.base.is_empty() { String::new() } else { commit(root, &request.base)? };
    let commits = commits(root, &base, &head)?;
    let blobs = published_blobs(root, &commits)?;
    let segments = text_segments(root, request, &commits, &blobs)?;
    let scratch = tempfile::tempdir().map_err(|e| format!("cannot create a scratch directory: {e}"))?;
    let work = scratch.path().join("work");
    let ignore = scratch.path().join("empty-ignore");
    std::fs::create_dir_all(&work)
        .and_then(|()| std::fs::write(&ignore, ""))
        .map_err(|e| format!("cannot prepare the scan: {e}"))?;
    let gitleaks = Gitleaks { config: &config, scratch: &work, ignore };
    let mut findings: BTreeSet<String> = gitleaks.scan_text("text", &segments)?.into_iter().collect();
    let files: BTreeSet<Blob> = blobs.into_iter().filter(|blob| blob.file).collect();
    for (index, layer) in layers(&files).iter().enumerate() {
        findings.extend(gitleaks.scan_layer(index, layer, root)?);
    }
    Ok(findings)
}

pub fn run(args: &[String]) -> u8 {
    if args.first().is_some_and(|command| command == "attest") {
        return super::denylist_attest::run(&args[1..]);
    }
    let Some(request) = parse(args) else {
        eprintln!("{USAGE}");
        return 2;
    };
    let outcome = std::env::current_dir()
        .map_err(|e| e.to_string())
        .and_then(|dir| git(&dir, &["rev-parse", "--show-toplevel"]))
        .map(|root| PathBuf::from(root.trim_end_matches('\n')))
        .and_then(|root| scan(&root, &request));
    match outcome {
        Ok(findings) if findings.is_empty() => 0,
        Ok(findings) => {
            for finding in findings {
                println!("denylist: {finding}");
            }
            1
        }
        Err(error) => {
            eprintln!("denylist: cannot scan: {error}");
            2
        }
    }
}

#[cfg(test)]
#[path = "denylist_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "denylist_publication_tests.rs"]
mod publication_tests;
