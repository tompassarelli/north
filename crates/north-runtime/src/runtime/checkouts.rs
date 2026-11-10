//! Keeps every host's harness checkouts at origin/main, so the policy north reads there (south's `agents/orchestration.toml` first) matches the host that settles the fleet: each fleet tick there asks every host it ships runtimes to fast-forward `~/code/<project>/main`, since nothing else moves a build host's checkout between north landings, and a host five commits behind read no `[hosts.NAME]` table at all (north#872).
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::os::unix::process::CommandExt;
use std::process::Stdio;
use std::time::Duration;

use rustix::fs::{FlockOperation, flock};

use super::bounded;
use super::failure::{Failure, failed};
use super::follow::own_scope;
use super::layout::Layout;
use super::remote::ssh;
use crate::land_offload;

/// Seconds one host may take: a fetch of each checkout it keeps and a fast-forward, with room for a slow link.
const LIMIT: Duration = Duration::from_secs(25);
/// How stale the last sync must be before the tick starts another, so a second tick entry (or `north fleet waits` on this host) in the same tick starts none: the timer runs every 30 s.
const AGAIN: Duration = Duration::from_secs(25);
const LAST: &str = "host-checkouts.last";
const LOCK: &str = "host-checkouts.lock";

/// The projects whose `main` checkout north reads policy, goals or configuration from, in the order a host syncs them: `south` first, since its `agents/orchestration.toml` places every host and its `agents` is the policy every session reads.
const CHECKOUTS: [&str; 5] = ["south", "north", "nixos-config", "wisp", "makgora"];

/// The shell a host runs: each checkout it keeps fetches origin/main and fast-forwards onto it when it is behind and git can, printing each move; a checkout behind that cannot fast-forward is named without touching it, a checkout this host lacks is skipped, and the script exits 0 so one host's refusal never stops the others.
pub fn script() -> String {
    let projects = CHECKOUTS.map(|project| format!(" {project}")).concat();
    format!(
        r#"set -u
for p in{projects}; do
  d="$HOME/code/$p/main"
  [ -e "$d/.git" ] || continue
  if ! git -C "$d" fetch -q --no-auto-maintenance origin main 2>/dev/null; then printf '%s: fetch failed\n' "$p"; continue; fi
  behind=$(git -C "$d" rev-list --count HEAD..origin/main 2>/dev/null) || {{ printf '%s: cannot count commits behind origin\n' "$p"; continue; }}
  [ "$behind" = 0 ] && continue
  branch=$(git -C "$d" symbolic-ref -q --short HEAD 2>/dev/null) || branch=
  if [ "$branch" != main ]; then
    if [ -n "$branch" ]; then printf '%s: on %s, not main\n' "$p" "$branch"; continue; fi
    if git -C "$d" rev-parse -q --verify refs/heads/main >/dev/null 2>&1 && ! git -C "$d" merge-base --is-ancestor refs/heads/main origin/main 2>/dev/null; then printf '%s: detached and local main is not an ancestor of origin/main\n' "$p"; continue; fi
    if ! git -C "$d" switch -q main 2>/dev/null; then printf '%s: detached with no local main to switch to\n' "$p"; continue; fi
  fi
  if [ -n "$(git -C "$d" status --porcelain --untracked-files=no 2>/dev/null)" ]; then printf '%s: uncommitted changes\n' "$p"; continue; fi
  if git -C "$d" merge -q --ff-only origin/main 2>/dev/null; then printf '%s: fast-forwarded %s commits\n' "$p" "$behind"; else printf '%s: %s commits behind and not fast-forwardable\n' "$p" "$behind"; fi
done
exit 0"#
    )
}

/// What the fleet tick prints: the moves and refusals the last sync found, which the tick before it started in a scope of its own, so no fetch delays the tick and a slow host costs the next tick nothing.
pub fn publish() -> Vec<String> {
    if !land_offload::settles_fleet_here() {
        return Vec::new();
    }
    let layout = Layout::from_env();
    let last = layout.state.join(LAST);
    let mut lines: Vec<String> = fs::read_to_string(&last).unwrap_or_default().lines().map(str::to_owned).collect();
    let recent = fs::metadata(&last).and_then(|metadata| metadata.modified()).ok().and_then(|at| at.elapsed().ok()).is_some_and(|age| age < AGAIN);
    if recent {
        return lines;
    }
    if let Err(error) = std::env::current_exe().and_then(|north| own_scope(north).args(["runtime", "checkouts"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).process_group(0).spawn()) {
        lines.push(format!("checkouts: not started: {error}"));
    }
    lines
}

pub(super) fn run() -> Result<(), Failure> {
    if !land_offload::settles_fleet_here() {
        return Err(failed(1, "this host does not settle the fleet, so it asks no host for its checkouts"));
    }
    let layout = Layout::from_env();
    let lock = File::create(layout.state.join(LOCK))?;
    if flock(&lock, FlockOperation::NonBlockingLockExclusive).is_err() {
        println!("north runtime checkouts: the last sync is still running");
        return Ok(());
    }
    let destinations = land_offload::other_destinations();
    let lines = keep_all(&destinations);
    fs::write(layout.state.join(LAST), lines.join("\n"))?;
    if lines.is_empty() {
        println!("north runtime checkouts: {} hosts already read origin/main", destinations.len());
    } else {
        println!("north runtime checkouts: {} hosts asked; {}", destinations.len(), lines.join("; "));
    }
    Ok(())
}

/// Each move a host made and each host that could not be asked, asked side by side so the slowest link sets the wait.
fn keep_all(destinations: &[String]) -> Vec<String> {
    let script = script();
    std::thread::scope(|scope| {
        let runs: Vec<_> = destinations.iter().map(|destination| (destination, scope.spawn(|| keep(destination, &script)))).collect();
        let mut kept = Vec::new();
        for (destination, run) in runs {
            match run.join() {
                Ok(Ok(lines)) => kept.extend(lines.into_iter().map(|line| format!("{destination} {line}"))),
                Ok(Err(why)) => kept.push(format!("not synced on {destination}: {why}")),
                Err(_) => kept.push(format!("not synced on {destination}: the sync panicked")),
            }
        }
        let unique: BTreeSet<String> = kept.into_iter().collect();
        unique.into_iter().collect()
    })
}

/// The lines `destination` printed while its checkouts caught up, or why it could not be asked.
fn keep(destination: &str, script: &str) -> Result<Vec<String>, String> {
    let mut command = ssh(destination);
    command.args(["sh", "-c", script]).stdin(Stdio::null());
    let (status, said) = bounded::output(&mut command, LIMIT).map_err(|error| error.to_string())?;
    let lines: Vec<String> = said.lines().map(str::trim).filter(|line| !line.is_empty()).map(str::to_owned).collect();
    if status.success() {
        Ok(lines)
    } else {
        Err(format!("exited {status}: {}", lines.join("; ")))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    fn git(directory: &Path, arguments: &[&str]) -> String {
        let output = Command::new("git").arg("-C").arg(directory).args(arguments).output().expect("git");
        assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    fn commit(directory: &Path, name: &str) -> String {
        std::fs::write(directory.join(name), name).expect("write");
        git(directory, &["add", name]);
        git(directory, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", name]);
        git(directory, &["rev-parse", "HEAD"])
    }

    /// An origin with one commit, and the `~/code/south/main` clone a host keeps of it with HOME under `root`.
    fn host(root: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
        let origin = root.join("origin");
        std::fs::create_dir_all(&origin).expect("origin");
        git(&origin, &["init", "-q", "-b", "main"]);
        commit(&origin, "one");
        let home = root.join("home");
        let checkout = home.join("code/south/main");
        std::fs::create_dir_all(checkout.parent().expect("parent")).expect("code");
        let status = Command::new("git").args(["clone", "-q"]).arg(&origin).arg(&checkout).status().expect("clone");
        assert!(status.success());
        (origin, home)
    }

    fn run(script: &str, home: &Path) -> String {
        let output = Command::new("sh").args(["-c", script]).env("HOME", home).output().expect("sh");
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    #[test]
    fn a_checkout_behind_origin_fast_forwards_and_one_already_current_is_quiet() {
        let scratch = tempfile::tempdir().expect("scratch");
        let (origin, home) = host(scratch.path());
        let ahead = commit(&origin, "two");
        assert_eq!(run(&super::script(), &home), "south: fast-forwarded 1 commits\n");
        assert_eq!(git(&home.join("code/south/main"), &["rev-parse", "HEAD"]), ahead);
        assert_eq!(run(&super::script(), &home), "", "a checkout already at origin/main says nothing");
    }

    #[test]
    fn an_uncommitted_change_or_a_branch_other_than_main_is_named_and_left_alone() {
        let scratch = tempfile::tempdir().expect("scratch");
        let (origin, home) = host(scratch.path());
        let checkout = home.join("code/south/main");
        std::fs::write(checkout.join("one"), "edited").expect("edit");
        commit(&origin, "two");
        let before = git(&checkout, &["rev-parse", "HEAD"]);
        assert_eq!(run(&super::script(), &home), "south: uncommitted changes\n");
        assert_eq!(git(&checkout, &["rev-parse", "HEAD"]), before, "a dirty checkout is never moved");
        git(&checkout, &["checkout", "-q", "--", "one"]);
        git(&checkout, &["switch", "-q", "-c", "work"]);
        assert_eq!(run(&super::script(), &home), "south: on work, not main\n");
    }

    #[test]
    fn a_detached_checkout_at_an_ancestor_returns_to_main_and_fast_forwards() {
        let scratch = tempfile::tempdir().expect("scratch");
        let (origin, home) = host(scratch.path());
        let checkout = home.join("code/south/main");
        let ahead = commit(&origin, "two");
        git(&checkout, &["checkout", "-q", "--detach"]);
        assert_eq!(run(&super::script(), &home), "south: fast-forwarded 1 commits\n");
        assert_eq!(git(&checkout, &["symbolic-ref", "-q", "--short", "HEAD"]), "main", "a detached checkout at an ancestor returns to main");
        assert_eq!(git(&checkout, &["rev-parse", "HEAD"]), ahead);
    }

    #[test]
    fn a_host_that_keeps_no_checkout_is_skipped() {
        let scratch = tempfile::tempdir().expect("scratch");
        let home = scratch.path().join("home");
        std::fs::create_dir_all(&home).expect("home");
        assert_eq!(run(&super::script(), &home), "");
    }
}
