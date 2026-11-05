use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

pub(super) fn git(repo: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git").arg("-C").arg(repo).args(args).stdin(Stdio::null()).output().map_err(|error| format!("git: {error}"))?;
    if !output.status.success() {
        return Err(format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub(super) fn git_ok(repo: &Path, args: &[&str]) -> Option<String> {
    git(repo, args).ok()
}

pub(super) fn line_count(text: &str) -> usize {
    let newlines = text.bytes().filter(|&byte| byte == b'\n').count();
    if text.is_empty() || text.ends_with('\n') { newlines } else { newlines + 1 }
}

pub(super) struct Snapshot {
    pub(super) repo: std::path::PathBuf,
    pub(super) rev: String,
    paths: Vec<String>,
    blobs: HashMap<String, String>,
    texts: HashMap<String, std::rc::Rc<str>>,
}

impl Snapshot {
    pub(super) fn new(repo: &Path, rev: &str) -> Result<Self, String> {
        let listing = git(repo, &["ls-tree", "-r", "-z", "--full-tree", rev])?;
        let mut paths = Vec::new();
        let mut blobs = HashMap::new();
        for entry in listing.split('\0').filter(|entry| !entry.is_empty()) {
            let Some((meta, path)) = entry.split_once('\t') else { continue };
            let mut fields = meta.split(' ');
            if let (Some(_), Some("blob"), Some(sha)) = (fields.next(), fields.next(), fields.next()) {
                paths.push(path.to_owned());
                blobs.insert(path.to_owned(), sha.to_owned());
            }
        }
        Ok(Self { repo: repo.to_path_buf(), rev: rev.to_owned(), paths, blobs, texts: HashMap::new() })
    }

    pub(super) fn paths(&self) -> &[String] {
        &self.paths
    }

    pub(super) fn has(&self, path: &str) -> bool {
        self.blobs.contains_key(path)
    }

    pub(super) fn text(&self, path: &str) -> Option<std::rc::Rc<str>> {
        if let Some(text) = self.texts.get(path) {
            return Some(text.clone());
        }
        let sha = self.blobs.get(path)?;
        let output = Command::new("git").arg("-C").arg(&self.repo).args(["cat-file", "blob", sha]).stdin(Stdio::null()).output().ok()?;
        output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into())
    }

    pub(super) fn load(&mut self, paths: &[String]) -> Result<(), String> {
        let want: Vec<&String> = paths.iter().filter(|path| self.has(path) && !self.texts.contains_key(*path)).collect();
        if want.is_empty() {
            return Ok(());
        }
        let mut child = Command::new("git").arg("-C").arg(&self.repo).args(["cat-file", "--batch"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|error| format!("git cat-file: {error}"))?;
        let input: String = want.iter().map(|path| format!("{}\n", self.blobs[*path])).collect();
        let mut stdin = child.stdin.take().ok_or("git cat-file: no stdin")?;
        let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
        let output = child.wait_with_output().map_err(|error| format!("git cat-file: {error}"))?;
        let _ = writer.join();
        if !output.status.success() {
            return Err(format!("git cat-file: {}", String::from_utf8_lossy(&output.stderr).trim()));
        }
        let buffer = output.stdout;
        let mut at = 0;
        for path in want {
            let newline = buffer[at..].iter().position(|&byte| byte == b'\n').map_or(buffer.len(), |offset| at + offset);
            let header = String::from_utf8_lossy(&buffer[at..newline]);
            let size: usize = header.split(' ').nth(2).and_then(|size| size.parse().ok()).unwrap_or(0);
            let end = (newline + 1 + size).min(buffer.len());
            self.texts.insert(path.clone(), String::from_utf8_lossy(&buffer[newline + 1..end]).into());
            at = end + 1;
        }
        Ok(())
    }
}
