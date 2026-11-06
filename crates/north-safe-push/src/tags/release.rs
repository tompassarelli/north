use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::shell::git::default_branch;
use crate::session::cancel::watch_for_cancel;
use north_landing::landing::step::{Refused, Step, git_quiet, git_read, refuse};
use super::publish::{self, Scanned, TagRequest};

const USAGE: &str = "usage: north release VERSION [--notes FILE]

In the main checkout of a repository, clean and at origin's default branch,
create the annotated tag vVERSION at HEAD (its annotation is FILE, or
\"Release vVERSION\"), push it through the safe-push tag scans, and create its
GitHub release with gh release create --verify-tag --notes-from-tag.
A rerun after a partial failure finishes the missing half; a version whose tag
and release both exist is refused. safe-push --tag refuses v* tags: they
publish only through north release.
The workflow .github/workflows/release-on-tag.yml gives a tag made any other way
its release; another repository calls it from a workflow on push tags v* with
uses: tompassarelli/north/.github/workflows/release-on-tag.yml@main and
permissions contents: write.
";

#[derive(Debug, Eq, PartialEq)]
pub struct Request {
    pub tag: String,
    pub notes: Option<PathBuf>,
}

#[derive(Debug, Eq, PartialEq)]
pub enum Parsed {
    Help,
    Release(Request),
}

pub fn parse(arguments: &[String]) -> Result<Parsed, String> {
    let mut version = None;
    let mut notes = None;
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "-h" | "--help" if arguments.len() == 1 => return Ok(Parsed::Help),
            "--notes" if notes.is_none() => match rest.next() {
                Some(file) if !file.is_empty() && !file.starts_with('-') => notes = Some(PathBuf::from(file)),
                _ => return Err("--notes needs a file".to_owned()),
            },
            other if version.is_none() && !other.starts_with('-') => version = Some(other.to_owned()),
            other => return Err(format!("unexpected argument '{other}'")),
        }
    }
    let version = version.ok_or("missing VERSION")?;
    let version = version.strip_prefix('v').unwrap_or(&version);
    if !plausible_version(version) {
        return Err(format!("invalid version '{version}': it starts with a digit and holds only letters, digits, '.', '-' and '+'"));
    }
    Ok(Parsed::Release(Request { tag: format!("v{version}"), notes }))
}

fn plausible_version(version: &str) -> bool {
    version.starts_with(|first: char| first.is_ascii_digit())
        && !version.contains("..")
        && !version.ends_with('.')
        && version.chars().all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '+'))
}

pub fn version_tag(tag: &str) -> bool {
    tag.starts_with('v')
}

pub fn run(arguments: &[String]) -> u8 {
    watch_for_cancel();
    let request = match parse(arguments) {
        Ok(Parsed::Help) => {
            print!("{USAGE}");
            return 0;
        }
        Ok(Parsed::Release(request)) => request,
        Err(reason) => {
            eprint!("north release: {reason}\n{USAGE}");
            return 2;
        }
    };
    match release(&request) {
        Ok(()) => {
            println!("north release: ✓ {} has its tag and its GitHub release", request.tag);
            0
        }
        Err(Refused(code)) => {
            println!("north release: refused; {} is not fully released", request.tag);
            if code == 0 { 1 } else { code }
        }
    }
}

fn release(request: &Request) -> Step {
    let dir = std::env::current_dir().map_err(|_| Refused(1))?;
    let tag = request.tag.as_str();
    let root = git_quiet(&dir, &["rev-parse", "--show-toplevel"]).ok_or_else(|| refuse("north release: not in a git repository"))?;
    let root = Path::new(&root);
    if !git_read(root, &["status", "--porcelain", "--untracked-files=normal"])?.is_empty() {
        return Err(refuse("north release: the working tree is dirty; commit or remove the changes first"));
    }
    let default = default_branch(root);
    let branch = git_quiet(root, &["symbolic-ref", "--quiet", "--short", "HEAD"]).unwrap_or_default();
    if branch != default {
        return Err(refuse(format!("north release: release from the {default} branch's checkout, not '{}'", if branch.is_empty() { "detached HEAD" } else { &branch })));
    }
    let tracking = format!("+refs/heads/{default}:refs/remotes/origin/{default}");
    git_read(root, &["fetch", "-q", "--no-tags", "origin", &tracking])?;
    let head = git_read(root, &["rev-parse", "HEAD"])?;
    if git_read(root, &["rev-parse", &format!("refs/remotes/origin/{default}")])? != head {
        return Err(refuse(format!("north release: HEAD is not origin/{default}; land or pull first")));
    }
    let remote_tag = !git_read(root, &["ls-remote", "origin", &format!("refs/tags/{tag}")])?.is_empty();
    let released = gh_quiet(root, &["release", "view", tag, "--json", "tagName"]);
    match (remote_tag, released) {
        (true, true) => return Err(refuse(format!("north release: {tag} already exists with its GitHub release"))),
        (false, true) => return Err(refuse(format!("north release: a GitHub release named {tag} exists without its tag; resolve it by hand"))),
        (false, false) => {
            create_tag(root, request, &head)?;
            push_tag(root, tag)?;
        }
        (true, false) => println!("north release: {tag} is on origin without its GitHub release; creating the release"),
    }
    create_release(root, tag)
}

fn create_tag(root: &Path, request: &Request, head: &str) -> Step {
    let tag = request.tag.as_str();
    let tag_ref = format!("refs/tags/{tag}");
    if let Some(existing) = git_quiet(root, &["rev-parse", "-q", "--verify", &tag_ref]) {
        let annotated = git_quiet(root, &["cat-file", "-t", &existing]).as_deref() == Some("tag");
        let target = git_quiet(root, &["rev-list", "-n1", &existing]).unwrap_or_default();
        if !annotated || target != head {
            return Err(refuse(format!("north release: local tag {tag} is not an annotated tag of HEAD; it was not made by north release")));
        }
        println!("north release: reusing the local annotated tag {tag}");
        return Ok(());
    }
    let message = format!("Release {tag}");
    let mut arguments = vec!["tag", "-a", tag];
    let notes = request.notes.as_ref().map(|notes| notes.to_string_lossy().into_owned());
    match &notes {
        Some(file) => arguments.extend(["-F", file.as_str()]),
        None => arguments.extend(["-m", message.as_str()]),
    }
    arguments.push(head);
    git_read(root, &arguments).map(drop)
}

fn push_tag(root: &Path, tag: &str) -> Step {
    std::env::set_current_dir(root).map_err(|_| Refused(1))?;
    match publish::publish_tag(&TagRequest { tag, dry_run: false })? {
        Scanned::Clean => Ok(()),
        Scanned::Refused(refused) => Err(refused),
    }
}

fn create_release(root: &Path, tag: &str) -> Step {
    let created = Command::new("gh")
        .current_dir(root)
        .args(["release", "create", tag, "--verify-tag", "--title", tag, "--notes-from-tag"])
        .status()
        .map_err(|_| refuse("north release: gh is not on PATH"))?;
    if created.success() || gh_quiet(root, &["release", "view", tag, "--json", "tagName"]) {
        return Ok(());
    }
    Err(refuse(format!("north release: gh release create {tag} failed; rerun north release to finish it")))
}

fn gh_quiet(root: &Path, arguments: &[&str]) -> bool {
    Command::new("gh")
        .current_dir(root)
        .args(arguments)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}
