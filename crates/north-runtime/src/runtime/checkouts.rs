//! Keeps every host's harness checkouts at origin/main, so the policy north reads there (south's `agents/orchestration.toml` first) matches the host that settles the fleet: each fleet tick there asks every host it ships runtimes to fast-forward `~/code/<project>/main`, since nothing else moves a build host's checkout between north landings, and a host five commits behind read no `[hosts.NAME]` table at all (north#872).
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use rustix::fs::{FlockOperation, flock};

use super::bounded;
use super::failure::{Failure, failed};
use super::follow::own_scope;
use super::layout::{self, Layout};
use super::remote::ssh;
use crate::land_offload::{self, ship::quote};

/// Seconds one host may take: a fetch of each checkout it keeps and a fast-forward, with room for a slow link.
const LIMIT: Duration = Duration::from_secs(25);
/// How stale the last sync must be before the tick starts another, so a second tick entry (or `north fleet waits` on this host) in the same tick starts none: the timer runs every 30 s.
const AGAIN: Duration = Duration::from_secs(25);
const LAST: &str = "host-checkouts.last";
const LOCK: &str = "host-checkouts.lock";

/// The projects whose `main` checkout north reads policy, goals or configuration from, in the order a host syncs them: `south` first, since its `agents/orchestration.toml` places every host and its `agents` is the policy every session reads.
const CHECKOUTS: [&str; 5] = ["south", "north", "nixos-config", "wisp", "makgora"];

/// Each project in `CHECKOUTS` with the origin its main checkout under `home` fetches, empty when that checkout is missing here.
fn origins(home: &Path) -> Vec<(&'static str, String)> {
    CHECKOUTS
        .iter()
        .map(|project| {
            let output = Command::new("git").arg("-C").arg(home.join("code").join(project).join("main")).args(["remote", "get-url", "origin"]).stdin(Stdio::null()).stderr(Stdio::null()).output();
            (*project, output.ok().filter(|output| output.status.success()).map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned()).unwrap_or_default())
        })
        .collect()
}

/// The shell a host runs: a checkout it lacks is cloned in the background from the origin `sources` names for it, so a host that enters offload holding only north and south gets every product repository's checkout from the same sync (on 2026-11-08 at 7:24 AM wisp's after-landing run on cpx62, which had no ~/code/wisp/main, exited 75 three times); each checkout it keeps fetches origin/main and fast-forwards onto it when it is behind and git can, printing each move; a checkout behind that cannot fast-forward is named without touching it, and the script exits 0 so one host's refusal never stops the others.
pub fn script(sources: &[(&str, String)]) -> String {
    let pairs = sources.iter().map(|(project, origin)| format!(" {} {}", quote(project), quote(origin))).collect::<String>();
    format!(
        r#"set -u
set --{pairs}
while [ $# -ge 2 ]; do
  p=$1 u=$2
  shift 2
  d="$HOME/code/$p/main"
  if [ ! -e "$d/.git" ]; then
    [ -n "$u" ] || continue
    if [ -e "$d" ]; then printf '%s: ~/code/%s/main holds no .git\n' "$p" "$p"; continue; fi
    mkdir -p "$HOME/code/$p" || {{ printf '%s: cannot make ~/code/%s\n' "$p" "$p"; continue; }}
    flock -n "$HOME/code/$p/.main-clone.lock" true 2>/dev/null || continue
    setsid -f sh -c 'exec 9>"$1/.main-clone.lock"; flock -n 9 || exit 0; [ -e "$1/main" ] && exit 0; t=$(mktemp -d "$1/.main-clone.XXXXXX") || exit 1; if git clone -q "$2" "$t"; then mv -T "$t" "$1/main"; else rmdir "$t"; fi' sh "$HOME/code/$p" "$u" </dev/null >/dev/null 2>&1
    printf '%s: cloning %s\n' "$p" "$u"
    continue
  fi
  if ! git -C "$d" fetch -q --no-auto-maintenance origin main 2>/dev/null; then printf '%s: fetch failed\n' "$p"; continue; fi
  if git -C "$d" rev-parse -q --verify HEAD >/dev/null 2>&1; then unborn=0 range=HEAD..origin/main; else unborn=1 range=origin/main; fi
  behind=$(git -C "$d" rev-list --count "$range" 2>/dev/null) || {{ printf '%s: cannot count commits behind origin\n' "$p"; continue; }}
  [ "$behind" = 0 ] && continue
  branch=$(git -C "$d" symbolic-ref -q --short HEAD 2>/dev/null) || branch=
  if [ "$branch" != main ]; then
    if [ -n "$branch" ]; then printf '%s: on %s, not main\n' "$p" "$branch"; continue; fi
    if git -C "$d" rev-parse -q --verify refs/heads/main >/dev/null 2>&1 && ! git -C "$d" merge-base --is-ancestor refs/heads/main origin/main 2>/dev/null; then printf '%s: detached and local main is not an ancestor of origin/main\n' "$p"; continue; fi
    if ! git -C "$d" switch -q main 2>/dev/null; then printf '%s: detached with no local main to switch to\n' "$p"; continue; fi
  fi
  if [ -n "$(git -C "$d" status --porcelain --untracked-files=no 2>/dev/null)" ]; then printf '%s: uncommitted changes\n' "$p"; continue; fi
  if git -C "$d" merge -q --ff-only origin/main 2>/dev/null; then
    if [ "$unborn" = 1 ]; then git -C "$d" branch -q --set-upstream-to=origin/main main 2>/dev/null; fi
    printf '%s: fast-forwarded %s commits\n' "$p" "$behind"
  else printf '%s: %s commits behind and not fast-forwardable\n' "$p" "$behind"; fi
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
    let script = script(&origins(&layout::home()));
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
        assert_eq!(run(&super::script(&[("south", String::new())]), &home), "south: fast-forwarded 1 commits\n");
        assert_eq!(git(&home.join("code/south/main"), &["rev-parse", "HEAD"]), ahead);
        assert_eq!(run(&super::script(&[("south", String::new())]), &home), "", "a checkout already at origin/main says nothing");
    }

    #[test]
    fn an_uncommitted_change_or_a_branch_other_than_main_is_named_and_left_alone() {
        let scratch = tempfile::tempdir().expect("scratch");
        let (origin, home) = host(scratch.path());
        let checkout = home.join("code/south/main");
        std::fs::write(checkout.join("one"), "edited").expect("edit");
        commit(&origin, "two");
        let before = git(&checkout, &["rev-parse", "HEAD"]);
        assert_eq!(run(&super::script(&[("south", String::new())]), &home), "south: uncommitted changes\n");
        assert_eq!(git(&checkout, &["rev-parse", "HEAD"]), before, "a dirty checkout is never moved");
        git(&checkout, &["checkout", "-q", "--", "one"]);
        git(&checkout, &["switch", "-q", "-c", "work"]);
        assert_eq!(run(&super::script(&[("south", String::new())]), &home), "south: on work, not main\n");
    }

    #[test]
    fn a_detached_checkout_at_an_ancestor_returns_to_main_and_fast_forwards() {
        let scratch = tempfile::tempdir().expect("scratch");
        let (origin, home) = host(scratch.path());
        let checkout = home.join("code/south/main");
        let ahead = commit(&origin, "two");
        git(&checkout, &["checkout", "-q", "--detach"]);
        assert_eq!(run(&super::script(&[("south", String::new())]), &home), "south: fast-forwarded 1 commits\n");
        assert_eq!(git(&checkout, &["symbolic-ref", "-q", "--short", "HEAD"]), "main", "a detached checkout at an ancestor returns to main");
        assert_eq!(git(&checkout, &["rev-parse", "HEAD"]), ahead);
    }

    #[test]
    fn lane_sync_fast_forwards_a_main_with_no_commits() {
        let scratch = tempfile::tempdir().expect("scratch");
        let (origin, home) = host(scratch.path());
        let checkout = home.join("code/south/main");
        std::fs::remove_dir_all(&checkout).expect("remove");
        std::fs::create_dir_all(&checkout).expect("checkout");
        git(&checkout, &["init", "-q", "-b", "main"]);
        git(&checkout, &["remote", "add", "origin", &origin.display().to_string()]);
        let tip = git(&origin, &["rev-parse", "main"]);
        assert_eq!(run(&super::script(&[("south", String::new())]), &home), "south: fast-forwarded 1 commits\n", "a main with no commits yet was kept as it was and `cannot count commits behind origin` printed");
        assert_eq!(git(&checkout, &["rev-parse", "HEAD"]), tip);
        assert_eq!(git(&checkout, &["rev-parse", "--abbrev-ref", "main@{upstream}"]), "origin/main");
    }

    #[test]
    fn a_host_lacking_a_checkout_clones_it_from_the_origin_the_settler_fetches() {
        let scratch = tempfile::tempdir().expect("scratch");
        let (origin, home) = host(scratch.path());
        let head = git(&origin, &["rev-parse", "HEAD"]);
        let sources = [("south", String::new()), ("wisp", origin.display().to_string()), ("makgora", String::new())];
        assert_eq!(run(&super::script(&sources), &home), format!("wisp: cloning {}\n", origin.display()), "on 2026-11-08 cpx62 entered offload holding north and south but no ~/code/wisp/main; a project the settler keeps no checkout of is skipped");
        let wisp = home.join("code/wisp/main");
        assert!(north_base::platform::wait::until(std::time::Duration::from_secs(20), || wisp.join(".git").exists()), "the background clone never moved into ~/code/wisp/main");
        assert_eq!(git(&wisp, &["rev-parse", "HEAD"]), head);
        assert_eq!(run(&super::script(&sources), &home), "", "a cloned checkout at origin/main says nothing");
    }
}
