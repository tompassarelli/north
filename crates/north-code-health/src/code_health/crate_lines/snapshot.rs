use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};

pub(super) struct Snapshot {
    files: BTreeMap<String, String>,
}

impl Snapshot {
    pub(super) fn at(repo: &Path, rev: &str) -> Result<Self, String> {
        let listing = git(repo, &["ls-tree", "-r", "-z", "--full-tree", rev])?;
        let entries: Vec<(String, String)> = listing
            .split('\0')
            .filter_map(|entry| {
                let (meta, path) = entry.split_once('\t')?;
                let mut fields = meta.split(' ');
                let (_, kind, object) = (fields.next()?, fields.next()?, fields.next()?);
                (kind == "blob" && wanted(path)).then(|| (path.to_owned(), object.to_owned()))
            })
            .collect();
        let objects: Vec<&str> = entries.iter().map(|(_, object)| object.as_str()).collect();
        let texts = blobs(repo, &objects)?;
        Ok(Self { files: entries.into_iter().zip(texts).map(|((path, _), text)| (path, text)).collect() })
    }

    pub(super) fn worktree(repo: &Path) -> Result<Self, String> {
        let listing = git(repo, &["ls-files", "-z", "-c", "-o", "--exclude-standard"])?;
        let files = listing
            .split('\0')
            .filter(|path| wanted(path))
            .filter_map(|path| std::fs::read_to_string(repo.join(path)).ok().map(|text| (path.to_owned(), text)))
            .collect();
        Ok(Self { files })
    }

    pub(super) fn text(&self, path: &str) -> Option<&str> {
        self.files.get(path).map(String::as_str)
    }

    pub(super) fn files(&self) -> impl Iterator<Item = (&str, &str)> {
        self.files.iter().map(|(path, text)| (path.as_str(), text.as_str()))
    }
}

fn wanted(path: &str) -> bool {
    path == "structure.toml" || path == "Cargo.toml" || path.ends_with("/Cargo.toml") || (path.ends_with(".rs") && !is_test_path(path))
}

pub(super) fn is_test_path(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    name == "tests.rs" || name.ends_with("_tests.rs") || path.split('/').any(|part| part == "tests")
}

fn git(repo: &Path, arguments: &[&str]) -> Result<String, String> {
    let output = Command::new("git").arg("-C").arg(repo).args(arguments).stdin(Stdio::null()).output().map_err(|error| format!("git: {error}"))?;
    if !output.status.success() {
        return Err(format!("git {}: {}", arguments.join(" "), String::from_utf8_lossy(&output.stderr).trim()));
    }
    String::from_utf8(output.stdout).map_err(|error| error.to_string())
}

fn blobs(repo: &Path, objects: &[&str]) -> Result<Vec<String>, String> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["cat-file", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|error| format!("git cat-file: {error}"))?;
    let request: String = objects.iter().map(|object| format!("{object}\n")).collect();
    let mut stdin = child.stdin.take().ok_or("git cat-file: no stdin")?;
    let writer = std::thread::spawn(move || stdin.write_all(request.as_bytes()));
    let mut reader = BufReader::new(child.stdout.take().ok_or("git cat-file: no stdout")?);
    let texts = objects.iter().map(|object| blob(&mut reader, object)).collect::<Result<Vec<_>, _>>();
    let written = writer.join().map_err(|_| "git cat-file: writer panicked".to_owned())?;
    let status = child.wait().map_err(|error| error.to_string())?;
    written.map_err(|error| format!("git cat-file: {error}"))?;
    if !status.success() {
        return Err(format!("git cat-file exited {status}"));
    }
    texts
}

fn blob(reader: &mut impl BufRead, object: &str) -> Result<String, String> {
    let mut header = String::new();
    reader.read_line(&mut header).map_err(|error| error.to_string())?;
    let size: usize = header.split(' ').nth(2).and_then(|size| size.trim().parse().ok()).ok_or_else(|| format!("git cat-file: no blob {object}: {}", header.trim()))?;
    let mut body = vec![0; size + 1];
    reader.read_exact(&mut body).map_err(|error| error.to_string())?;
    body.truncate(size);
    Ok(String::from_utf8_lossy(&body).into_owned())
}
