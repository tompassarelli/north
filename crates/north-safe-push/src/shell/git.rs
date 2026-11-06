use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use north_base::foundation::step::{Refused, Step, cancelled};
use north_base::platform::git;
use north_base::platform::process::exit_code;

pub enum Annotation<'a> {
    File(&'a Path),
    Message(&'a str),
}

pub trait Git {
    fn default_branch(&self) -> String;
    fn symbolic_ref(&self, name: &str) -> Option<String>;
    fn current_branch(&self) -> Option<String>;
    fn toplevel(&self) -> Option<String>;
    fn common_dir(&self) -> Step<String>;
    fn valid_branch_name(&self, branch: &str) -> bool;
    fn valid_reference(&self, reference: &str) -> bool;
    fn has_commit(&self, oid: &str) -> bool;
    fn is_ancestor(&self, ancestor: &str, descendant: &str) -> bool;
    fn resolve(&self, reference: &str) -> Option<String>;
    fn rev_parse(&self, revision: &str) -> Step<String>;
    fn object_type(&self, oid: &str) -> Option<String>;
    fn object(&self, oid: &str) -> Step<Vec<u8>>;
    fn commit_of(&self, revision: &str) -> Step<String>;
    fn boundary(&self, revisions: &[String]) -> Step<String>;
    fn count(&self, revisions: &[String]) -> Step<String>;
    fn dirty(&self) -> Step<bool>;
    fn annotate(&self, tag: &str, annotation: &Annotation, commit: &str) -> Step;
    fn delete_ref(&self, reference: &str);
    fn mirror_holds(&self, mirror: &Path, commit: &str, patterns: &[&str]) -> bool;
    fn executable_hook(&self, name: &str) -> Option<PathBuf>;
    fn setting(&self, remote_ref: &str, key: &str) -> String;
    fn push_url(&self) -> Option<String>;
}

pub struct Live {
    directory: PathBuf,
}

impl Live {
    pub fn new(directory: &Path) -> Self {
        Self { directory: directory.to_path_buf() }
    }

    fn read(&self, arguments: &[&str]) -> Option<String> {
        quiet(&self.directory, arguments)
    }

    fn loud(&self, arguments: &[&str]) -> Step<String> {
        read(&self.directory, arguments)
    }

    fn revisions(&self, flag: &str, revisions: &[String]) -> Step<String> {
        let mut arguments = vec!["rev-list", flag];
        arguments.extend(revisions.iter().map(String::as_str));
        self.loud(&arguments)
    }
}

impl Git for Live {
    fn default_branch(&self) -> String {
        default_branch(&self.directory)
    }

    fn symbolic_ref(&self, name: &str) -> Option<String> {
        symbolic_ref(&self.directory, name)
    }

    fn current_branch(&self) -> Option<String> {
        self.read(&["symbolic-ref", "--quiet", "--short", "HEAD"])
    }

    fn toplevel(&self) -> Option<String> {
        self.read(&["rev-parse", "--show-toplevel"])
    }

    fn common_dir(&self) -> Step<String> {
        self.loud(&["rev-parse", "--path-format=absolute", "--git-common-dir"])
    }

    fn valid_branch_name(&self, branch: &str) -> bool {
        self.valid_reference(&format!("refs/heads/{branch}"))
    }

    fn valid_reference(&self, reference: &str) -> bool {
        self.read(&["check-ref-format", reference]).is_some()
    }

    fn has_commit(&self, oid: &str) -> bool {
        self.read(&["cat-file", "-e", &format!("{oid}^{{commit}}")]).is_some()
    }

    fn is_ancestor(&self, ancestor: &str, descendant: &str) -> bool {
        self.read(&["merge-base", "--is-ancestor", ancestor, descendant]).is_some()
    }

    fn resolve(&self, reference: &str) -> Option<String> {
        self.read(&["rev-parse", "-q", "--verify", reference])
    }

    fn rev_parse(&self, revision: &str) -> Step<String> {
        self.loud(&["rev-parse", revision])
    }

    fn object_type(&self, oid: &str) -> Option<String> {
        self.read(&["cat-file", "-t", oid])
    }

    fn object(&self, oid: &str) -> Step<Vec<u8>> {
        git::run(&self.directory, &["cat-file", "-p", oid]).map(|output| output.stdout).map_err(|_| Refused(127))
    }

    fn commit_of(&self, revision: &str) -> Step<String> {
        self.loud(&["rev-list", "-n1", revision])
    }

    fn boundary(&self, revisions: &[String]) -> Step<String> {
        self.revisions("--boundary", revisions)
    }

    fn count(&self, revisions: &[String]) -> Step<String> {
        self.revisions("--count", revisions)
    }

    fn dirty(&self) -> Step<bool> {
        self.loud(&["status", "--porcelain", "--untracked-files=normal"]).map(|status| !status.is_empty())
    }

    fn annotate(&self, tag: &str, annotation: &Annotation, commit: &str) -> Step {
        let file;
        let (flag, value) = match annotation {
            Annotation::File(path) => {
                file = path.to_string_lossy();
                ("-F", file.as_ref())
            }
            Annotation::Message(message) => ("-m", *message),
        };
        self.loud(&["tag", "-a", tag, flag, value, commit]).map(drop)
    }

    fn delete_ref(&self, reference: &str) {
        let _ = self.read(&["update-ref", "-d", reference]);
    }

    fn mirror_holds(&self, mirror: &Path, commit: &str, patterns: &[&str]) -> bool {
        let git_dir = mirror.to_string_lossy();
        let mut arguments = vec!["--git-dir", git_dir.as_ref(), "for-each-ref", "--count=1", "--contains", commit, "--format=%(refname)"];
        arguments.extend(patterns);
        self.read(&arguments).is_some_and(|refs| !refs.is_empty())
    }

    fn executable_hook(&self, name: &str) -> Option<PathBuf> {
        let path = PathBuf::from(self.read(&["rev-parse", "--path-format=absolute", "--git-path", &format!("hooks/{name}")])?);
        std::fs::metadata(&path).is_ok_and(|meta| meta.is_file() && std::os::unix::fs::PermissionsExt::mode(&meta.permissions()) & 0o111 != 0).then_some(path)
    }

    fn setting(&self, remote_ref: &str, key: &str) -> String {
        git::safe_push_setting(&self.directory, remote_ref, key)
    }

    fn push_url(&self) -> Option<String> {
        self.read(&["remote", "get-url", "--push", "origin"])
    }
}

pub fn quiet(directory: &Path, arguments: &[&str]) -> Option<String> {
    let output = git::run(directory, arguments).ok()?;
    cancelled().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim_end_matches('\n').to_owned())
}

pub fn default_branch(repository: &Path) -> String {
    let origin_head = symbolic_ref(repository, "refs/remotes/origin/HEAD").unwrap_or_default();
    let default = origin_head.strip_prefix("refs/remotes/origin/").unwrap_or(&origin_head);
    if default.is_empty() { "main".to_owned() } else { default.to_owned() }
}

pub fn symbolic_ref(repository: &Path, name: &str) -> Option<String> {
    let output = git::run(repository, &["symbolic-ref", "--quiet", name]).ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim_end_matches('\n').to_owned())
}

pub fn read(directory: &Path, arguments: &[&str]) -> Step<String> {
    let output = Command::new("git").current_dir(directory).args(arguments).stderr(Stdio::inherit()).output().map_err(|_| Refused(127))?;
    cancelled()?;
    if !output.status.success() {
        return Err(Refused(exit_code(output.status)));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim_end_matches('\n').to_owned())
}

pub fn status(directory: &Path, arguments: &[&str]) -> Step {
    let status = Command::new("git").current_dir(directory).args(arguments).status().map_err(|_| Refused(127))?;
    cancelled()?;
    if status.success() { Ok(()) } else { Err(Refused(exit_code(status))) }
}

pub fn feed(directory: &Path, arguments: &[&str], input: &str) -> Option<String> {
    let mut child = Command::new("git")
        .current_dir(directory)
        .args(arguments)
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
