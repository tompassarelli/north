use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Eq, PartialEq)]
pub struct Refused(pub u8);

pub type Step<T = ()> = Result<T, Refused>;

pub fn refuse(message: impl std::fmt::Display) -> Refused {
    if let Err(cancel) = cancelled() {
        return cancel;
    }
    println!("{message}");
    Refused(1)
}

pub fn state_changed(what: &str) -> Refused {
    if let Err(cancel) = cancelled() {
        return cancel;
    }
    eprintln!("safe-push: repository state changed during secret scan ({what}) — refusing; NOT pushed.");
    Refused(1)
}

fn git(dir: &Path, args: &[&str], stderr: Stdio) -> Step<String> {
    let output = Command::new("git").current_dir(dir).args(args).stderr(stderr).output().map_err(|_| Refused(127))?;
    cancelled()?;
    if !output.status.success() {
        return Err(Refused(exit_code(output.status)));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim_end_matches('\n').to_owned())
}

pub fn git_quiet(dir: &Path, args: &[&str]) -> Option<String> {
    git(dir, args, Stdio::null()).ok()
}

pub fn git_read(dir: &Path, args: &[&str]) -> Step<String> {
    git(dir, args, Stdio::inherit())
}

pub fn git_status(dir: &Path, args: &[&str]) -> Step {
    let status = Command::new("git").current_dir(dir).args(args).status().map_err(|_| Refused(127))?;
    cancelled()?;
    if status.success() { Ok(()) } else { Err(Refused(exit_code(status))) }
}

pub fn exit_code(status: std::process::ExitStatus) -> u8 {
    use std::os::unix::process::ExitStatusExt;
    match (status.code(), status.signal()) {
        (Some(code), _) => u8::try_from(code).unwrap_or(1),
        (None, Some(signal)) => u8::try_from(128 + signal).unwrap_or(1),
        (None, None) => 1,
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct Identity {
    pub root: PathBuf,
    pub origin_url: String,
    git_dir: String,
    branch_ref: String,
    head_oid: String,
}

impl Identity {
    pub fn capture(dir: &Path) -> Step<Self> {
        let root = git_quiet(dir, &["rev-parse", "--show-toplevel"]).ok_or_else(|| refuse("safe-push: not in a git repo"))?;
        let git_dir = git_quiet(dir, &["rev-parse", "--absolute-git-dir"])
            .ok_or_else(|| refuse("safe-push: cannot identify this Git worktree"))?;
        let origin_url = git_quiet(dir, &["remote", "get-url", "--push", "origin"])
            .ok_or_else(|| refuse("safe-push: origin has no push URL"))?;
        let branch_ref = git_quiet(dir, &["symbolic-ref", "--quiet", "HEAD"])
            .ok_or_else(|| refuse("safe-push: detached HEAD — checkout a branch first"))?;
        if !branch_ref.starts_with("refs/heads/") {
            return Err(refuse("safe-push: HEAD does not name a local branch"));
        }
        let head_oid = git_quiet(dir, &["rev-parse", "--verify", &format!("{branch_ref}^{{commit}}")])
            .ok_or_else(|| refuse(format!("safe-push: cannot resolve {branch_ref} to a commit")))?;
        Ok(Self { root: PathBuf::from(root), origin_url, git_dir, branch_ref, head_oid })
    }

    pub fn revalidate(&self, dir: &Path) -> Step {
        let root = self.root.to_string_lossy();
        let head = format!("{}^{{commit}}", self.branch_ref);
        let checks: [(&[&str], &str, &str, &str); 5] = [
            (&["rev-parse", "--show-toplevel"], &root, "repository unavailable", "repository root/worktree changed"),
            (&["rev-parse", "--absolute-git-dir"], &self.git_dir, "Git worktree unavailable", "Git worktree changed"),
            (&["remote", "get-url", "--push", "origin"], &self.origin_url, "origin push URL unavailable", "origin push URL changed"),
            (&["symbolic-ref", "--quiet", "HEAD"], &self.branch_ref, "HEAD became detached", "current branch changed"),
            (&["rev-parse", "--verify", &head], &self.head_oid, "HEAD commit unavailable", "HEAD commit changed"),
        ];
        for (args, expected, unavailable, changed) in checks {
            match git_quiet(dir, args) {
                None => return Err(state_changed(unavailable)),
                Some(current) if current != expected => return Err(state_changed(changed)),
                Some(_) => {}
            }
        }
        Ok(())
    }
}

pub struct OriginRefs<'a> {
    dir: &'a Path,
    namespace: String,
}

impl<'a> OriginRefs<'a> {
    pub fn fetch(dir: &'a Path, origin_url: &str) -> Step<Self> {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.subsec_nanos());
        let refs = Self { dir, namespace: format!("refs/safe-push-origin/{}-{}", std::process::id(), nonce % 32768) };
        let refspec = format!("+refs/*:{}/*", refs.namespace);
        git_status(dir, &["fetch", "-q", "--no-tags", "--no-write-fetch-head", origin_url, &refspec])?;
        Ok(refs)
    }

    fn names(&self) -> Vec<String> {
        names(self.dir, &self.namespace)
    }

    pub fn snapshot(&self) -> Step<String> {
        let prefix = format!("{}/", self.namespace);
        let listing = git_read(self.dir, &["for-each-ref", "--format=%(objectname) %(refname)", &prefix])?;
        let lines = listing
            .lines()
            .filter_map(|line| line.split_once(' '))
            .map(|(oid, name)| format!("{oid}\trefs/{}", name.strip_prefix(&prefix).unwrap_or(name)));
        Ok(sorted(lines))
    }

    pub fn commits(&self) -> Step<Vec<String>> {
        let names = self.names();
        if names.is_empty() {
            return Ok(Vec::new());
        }
        let input: String = names.iter().map(|name| format!("{name}^{{commit}}\n")).collect();
        let peeled = feed(self.dir, &["cat-file", "--batch-check=%(objectname)"], &input).ok_or(Refused(1))?;
        let commits: std::collections::BTreeSet<&str> = peeled.lines().filter(|line| !line.contains(' ')).collect();
        Ok(commits.into_iter().map(str::to_owned).collect())
    }
}

impl Drop for OriginRefs<'_> {
    fn drop(&mut self) {
        delete_namespace(self.dir, &self.namespace);
    }
}

fn names(dir: &Path, namespace: &str) -> Vec<String> {
    feed(dir, &["for-each-ref", "--format=%(refname)", &format!("{namespace}/")], "")
        .map(|names| names.lines().map(str::to_owned).collect())
        .unwrap_or_default()
}

fn delete_namespace(dir: &Path, namespace: &str) {
    let input: String = names(dir, namespace).iter().map(|name| format!("delete {name}\n")).collect();
    let _ = feed(dir, &["update-ref", "--stdin"], &input);
}

static CANCELLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn cancelled() -> Step {
    if CANCELLED.load(std::sync::atomic::Ordering::SeqCst) { Err(Refused(130)) } else { Ok(()) }
}

pub fn watch_for_cancel() {
    std::thread::spawn(|| {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread().enable_all().build() else { return };
        runtime.block_on(async {
            use tokio::signal::unix::{SignalKind, signal};
            let (Ok(mut interrupt), Ok(mut terminate), Ok(mut hangup)) =
                (signal(SignalKind::interrupt()), signal(SignalKind::terminate()), signal(SignalKind::hangup()))
            else {
                return;
            };
            loop {
                tokio::select! {
                    _ = interrupt.recv() => {}
                    _ = terminate.recv() => {}
                    _ = hangup.recv() => {}
                }
                CANCELLED.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        });
    });
}

fn feed(dir: &Path, args: &[&str], input: &str) -> Option<String> {
    use std::io::Write;
    let mut child = Command::new("git")
        .current_dir(dir)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdin = child.stdin.take()?;
    let output = std::thread::scope(|scope| {
        let writer = scope.spawn(move || stdin.write_all(input.as_bytes()));
        let output = child.wait_with_output().ok()?;
        writer.join().ok()?.ok()?;
        Some(output)
    })?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

pub fn remote_snapshot(dir: &Path, origin_url: &str) -> Step<String> {
    Ok(sorted(git_read(dir, &["ls-remote", "--refs", origin_url])?.lines().map(str::to_owned)))
}

fn sorted(lines: impl Iterator<Item = String>) -> String {
    let mut lines: Vec<String> = lines.collect();
    lines.sort();
    lines.join("\n")
}
