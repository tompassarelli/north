use std::path::{Path, PathBuf};

use north_base::foundation::step::{Refused, Step, refuse};

use super::name::release_name;
use super::publish::{self, Scanned, TagRequest};
use crate::session::cancel::watch_for_cancel;
use crate::shell::Shell;
use crate::shell::git::Annotation;


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
    Ok(Parsed::Release(Request { tag: release_name(version), notes }))
}

fn plausible_version(version: &str) -> bool {
    version.starts_with(|first: char| first.is_ascii_digit())
        && !version.contains("..")
        && !version.ends_with('.')
        && version.chars().all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '+'))
}

pub fn run(arguments: &[String], shell: &Shell) -> u8 {
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
    match release(&request, shell) {
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

fn release(request: &Request, shell: &Shell) -> Step {
    let git = &*shell.git;
    let tag = request.tag.as_str();
    let root = git.toplevel().ok_or_else(|| refuse("north release: not in a git repository"))?;
    std::env::set_current_dir(Path::new(&root)).map_err(|_| Refused(1))?;
    if git.dirty()? {
        return Err(refuse("north release: the working tree is dirty; commit or remove the changes first"));
    }
    let default = git.default_branch();
    let branch = git.current_branch().unwrap_or_default();
    if branch != default {
        return Err(refuse(format!("north release: release from the {default} branch's checkout, not '{}'", if branch.is_empty() { "detached HEAD" } else { &branch })));
    }
    shell.origin.fetch(&format!("+refs/heads/{default}:refs/remotes/origin/{default}"))?;
    let head = git.rev_parse("HEAD")?;
    if git.rev_parse(&format!("refs/remotes/origin/{default}"))? != head {
        return Err(refuse(format!("north release: HEAD is not origin/{default}; land or pull first")));
    }
    let remote_tag = !shell.origin.listing("origin", &[&format!("refs/tags/{tag}")])?.is_empty();
    match (remote_tag, shell.github.release_exists(tag)) {
        (true, true) => return Err(refuse(format!("north release: {tag} already exists with its GitHub release"))),
        (false, true) => return Err(refuse(format!("north release: a GitHub release named {tag} exists without its tag; resolve it by hand"))),
        (false, false) => {
            create_tag(shell, request, &head)?;
            match publish::publish_tag(&TagRequest { tag, dry_run: false }, shell)? {
                Scanned::Clean => {}
                Scanned::Refused(refused) => return Err(refused),
            }
        }
        (true, false) => println!("north release: {tag} is on origin without its GitHub release; creating the release"),
    }
    create_release(shell, tag)
}

fn create_tag(shell: &Shell, request: &Request, head: &str) -> Step {
    let git = &*shell.git;
    let tag = request.tag.as_str();
    if let Some(existing) = git.resolve(&format!("refs/tags/{tag}")) {
        let annotated = git.object_type(&existing).as_deref() == Some("tag");
        let target = git.commit_of(&existing).unwrap_or_default();
        if !annotated || target != head {
            return Err(refuse(format!("north release: local tag {tag} is not an annotated tag of HEAD; it was not made by north release")));
        }
        println!("north release: reusing the local annotated tag {tag}");
        return Ok(());
    }
    let message = format!("Release {tag}");
    let annotation = request.notes.as_deref().map_or(Annotation::Message(&message), Annotation::File);
    git.annotate(tag, &annotation, head)
}

fn create_release(shell: &Shell, tag: &str) -> Step {
    let created = shell.github.create_release(tag).ok_or_else(|| refuse("north release: gh is not on PATH"))?;
    if created || shell.github.release_exists(tag) {
        return Ok(());
    }
    Err(refuse(format!("north release: gh release create {tag} failed; rerun north release to finish it")))
}
