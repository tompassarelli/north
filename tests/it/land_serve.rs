use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use north_landing::land::queue::{self, RedFix, Ticket};

#[path = "land_cleanup.rs"]
mod cleanup;
#[path = "land_sheriff.rs"]
mod sheriff;

#[path = "land_serve/precheck.rs"]
mod precheck;

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").arg("-C").arg(directory).args(arguments).output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn executable(path: &Path, body: &str) {
    north_core::foundation::shim::write_executable(path, body);
}

const CHECKER: &str = r#"#!/usr/bin/env bash
set -eu
exec 8>>"$LAND_TRACE/checks.lock"
flock 8
n=$(( $(cat "$LAND_TRACE/checks" 2>/dev/null | wc -l) + 1 ))
printf '%s %s %s\n' "$(git rev-parse HEAD)" "$3" "$*" >>"$LAND_TRACE/checks"
exec 8>&-
env >"$LAND_TRACE/env-$n"
[ ! -x "$LAND_TRACE/hook" ] || "$LAND_TRACE/hook" "$n"
printf 'land-check ref push\ndispatch\nref delete\n' >>"$LAND_TRACE/github"
verdict=green
[ ! -e "$LAND_TRACE/verdict" ] || verdict=$(cat "$LAND_TRACE/verdict")
[ ! -e "$LAND_TRACE/verdict-$n" ] || verdict=$(cat "$LAND_TRACE/verdict-$n")
status=0
if [ -e "$LAND_TRACE/poison" ]; then
  { read -r poisoned; read -r poison_verdict; read -r poison_status; } <"$LAND_TRACE/poison"
  [ ! -e "$poisoned" ] || { verdict=$poison_verdict; status=$poison_status; }
fi
printf 'checking\nsafe-push: verdict %s\n' "$verdict"
[ ! -e "$LAND_TRACE/exit-$n" ] || status=$(cat "$LAND_TRACE/exit-$n")
exit "$status"
"#;

const MINT: &str = r#"#!/usr/bin/env bash
set -eu
[ "$*" = "--app lander" ] || { echo "unexpected arguments: $*" >&2; exit 2; }
[ ! -e "$LAND_TRACE/mint-fail" ] || { echo "token exchange failed: HTTP 502" >&2; exit 1; }
printf x >>"$LAND_TRACE/mints"
echo app-token
"#;

const NORTH: &str = r#"#!/usr/bin/env bash
printf '%s\n' "${*//$'\n'/\\n}" >>"$LAND_TRACE/north-calls"
if { [ "$1" = land-train ] || [ "$1" = incident ]; } && [ -n "${LAND_LEDGER:-}" ]; then THREADS_DB="$LAND_LEDGER" THREADS_SCHEMA_READY=1 exec "$LAND_REAL_NORTH" "$@"; fi
if [ "$1 $2" = "runtime follow" ] && [ -n "${NORTH_RUNTIME_LINK:-}" ]; then ln -sfn gen-2 "$NORTH_RUNTIME_LINK"; fi
if [ "$1" = fleet ] && [ "$2" = send ] && [ "${6%% *}" = landed ]; then
  branch=$(printf '%s' "$6" | awk '{print $2}')
  main=$(git -C "$LAND_ORIGIN" rev-parse main)
  printf '%s %s\n' "$5" "$(git -C "$LAND_REPO" cherry "$main" "$branch" | tr '\n' ' ')" >>"$LAND_TRACE/send-cherry"
  printf '%s %s\n' "$5" "$6" >>"$LAND_TRACE/sent"
fi
"#;

fn now_micros() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_micros()
}

struct Fixture {
    _scratch: tempfile::TempDir,
    root: PathBuf,
    origin: PathBuf,
    repo: PathBuf,
    state: PathBuf,
    trace: PathBuf,
    bin: PathBuf,
    base: String,
    queued: u128,
}

impl Fixture {
    fn new() -> Self {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path().to_owned();
        let (origin, repo, state, trace, bin) = (root.join("origin.git"), root.join("proj/main"), root.join("state"), root.join("trace"), root.join("bin"));
        for directory in [&state, &trace, &bin] {
            fs::create_dir_all(directory).unwrap();
        }
        git(&root, &["init", "-q", "--bare", "-b", "main", origin.to_str().unwrap()]);
        fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.name", "Lander scenario"]);
        git(&repo, &["config", "user.email", "lander@example.invalid"]);
        git(&repo, &["config", "commit.gpgsign", "false"]);
        git(&repo, &["remote", "add", "origin", origin.to_str().unwrap()]);
        fs::write(repo.join("app.txt"), "alpha\nbeta\ngamma\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "base"]);
        git(&repo, &["push", "-q", "origin", "main"]);
        let base = git(&repo, &["rev-parse", "HEAD"]);
        fs::write(root.join("threads.db"), b"").unwrap();
        executable(&trace.join("checker"), CHECKER);
        executable(&trace.join("north"), NORTH);
        executable(&trace.join("mint"), MINT);
        let real_git = String::from_utf8(Command::new("bash").args(["-c", "command -v git"]).output().unwrap().stdout).unwrap().trim().to_owned();
        executable(
            &bin.join("git"),
            &format!("#!/usr/bin/env bash\npushing=0\nfor a in \"$@\"; do if [ \"$a\" = push ]; then pushing=1; printf '%s token=%s app=%s\\n' \"$*\" \"${{GH_TOKEN:-none}}\" \"${{NORTH_LAND_PUSH_TOKEN:-none}}\" >>\"$LAND_TRACE/pushes\"; break; fi; done\nif [ $pushing = 1 ] && [ -e \"$LAND_TRACE/kill-after-push\" ]; then\n  {real_git} \"$@\"; rc=$?\n  rm -f \"$LAND_TRACE/kill-after-push\"\n  kill -9 $PPID\n  exit $rc\nfi\nexec {real_git} \"$@\"\n"),
        );
        Self { _scratch: scratch, root, origin, repo, state, trace, bin, base, queued: now_micros() }
    }

    fn lane(&self, name: &str, file: &str, text: &str) -> PathBuf {
        let root = self.root.join("proj/worktrees").join(name);
        git(&self.repo, &["fetch", "-q", "origin"]);
        git(&self.repo, &["worktree", "add", "-q", "-b", &format!("lane-{name}"), root.to_str().unwrap(), "origin/main"]);
        self.commit(&root, file, text, &format!("change from {name}"))
    }

    fn commit(&self, root: &Path, file: &str, text: &str, message: &str) -> PathBuf {
        fs::write(root.join(file), text).unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-qm", message]);
        root.to_owned()
    }

    fn queue(&self) -> PathBuf {
        queue::directory(&self.state, self.origin.to_str().unwrap())
    }

    fn enqueue(&mut self, root: &Path, red_fix: Option<RedFix>) -> PathBuf {
        let (path, ticket) = self.ticket(root, red_fix);
        queue::write(&path, &ticket).unwrap();
        path
    }

    fn ticket(&mut self, root: &Path, red_fix: Option<RedFix>) -> (PathBuf, Ticket) {
        self.queued += 1;
        let branch = git(root, &["symbolic-ref", "--short", "HEAD"]);
        let ticket = Ticket {
            origin: self.origin.to_str().unwrap().to_owned(),
            root: root.to_owned(),
            host: "laptop".to_owned(),
            branch: branch.clone(),
            head: git(root, &["rev-parse", "HEAD"]),
            session: Some(format!("session-{branch}")),
            queued_at: "2026-11-04T10:00:00+07:00".to_owned(),
            first_commit_secs: None,
            several_commits: None,
            red_fix,
            verdict: None,
            keep_lane: false,
        };
        (self.queue().join(queue::file_name(self.queued, &branch)), ticket)
    }

    fn command(&self) -> Command {
        self.command_with(["--repo", self.repo.to_str().unwrap()])
    }

    fn command_with<const N: usize>(&self, selection: [&str; N]) -> Command {
        let path = format!("{}:{}", self.bin.display(), std::env::var("PATH").unwrap());
        let mut command = Command::new(env!("CARGO_BIN_EXE_north"));
        command
            .args(["land", "serve", "--once"]).args(selection).args(["--checker", self.trace.join("checker").to_str().unwrap(), "--app-token", self.trace.join("mint").to_str().unwrap()])
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("XDG_STATE_HOME", &self.state)
            .env("PATH", path)
            .env("LAND_TRACE", &self.trace)
            .env("NORTH_LAND_NORTH", self.trace.join("north"))
            .env("LAND_ORIGIN", &self.origin)
            .env("LAND_LEDGER", self.root.join("threads.db"))
            .env("LAND_REAL_NORTH", env!("CARGO_BIN_EXE_north"))
            .env("LAND_REPO", &self.repo)
            .env("FAKE_REMOTE_HOMES", self.root.join("remote-homes"))
            .env("GH_TOKEN", "push-token")
            .env("GITHUB_TOKEN", "push-token")
            .env("SOPS_AGE_KEY_FILE", self.root.join("age.key"));
        command
    }

    fn serve(&self) -> Output {
        let output = self.command().output().unwrap();
        eprintln!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        output
    }

    fn main(&self) -> String {
        git(&self.origin, &["rev-parse", "refs/heads/main"])
    }

    fn subjects(&self) -> Vec<String> {
        git(&self.origin, &["log", "--reverse", "--format=%s", &format!("{}..main", self.base)]).lines().map(str::to_owned).collect()
    }

    fn read(&self, name: &str) -> String {
        fs::read_to_string(self.trace.join(name)).unwrap_or_default()
    }

    fn checks(&self) -> Vec<(String, String)> {
        self.read("checks").lines().map(|line| {
            let mut words = line.split(' ');
            (words.next().unwrap().to_owned(), words.next().unwrap().to_owned())
        }).collect()
    }

    fn pushes_tsv(&self) -> Vec<Vec<String>> {
        fs::read_to_string(self.state.join("safe-push/pushes.tsv")).unwrap_or_default().lines().map(|line| line.split('\t').map(str::to_owned).collect()).collect()
    }

    fn calls(&self, prefix: &str) -> Vec<String> {
        self.read("north-calls").lines().filter(|line| line.starts_with(prefix) && !line.starts_with("capacity offload reap")).map(str::to_owned).collect()
    }

    fn fleet(&self) -> Vec<String> {
        self.calls("fleet send")
    }

    fn verdict(&self, ticket: &Path) -> Option<String> {
        queue::read(&ticket.parent().unwrap().join("done").join(ticket.file_name().unwrap())).ok().and_then(|ticket| ticket.verdict)
    }
}

#[test]
fn serve_train() {
    let mut fixture = Fixture::new();
    let lanes: Vec<PathBuf> = ["a", "b", "c"].iter().map(|name| fixture.lane(name, &format!("{name}.txt"), name)).collect();
    let tickets: Vec<PathBuf> = lanes.iter().map(|lane| fixture.enqueue(lane, None)).collect();
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.subjects(), ["change from a", "change from b", "change from c"]);
    assert_eq!(fixture.checks().len(), 1, "the candidate is checked once");
    let main = fixture.main();
    for ticket in &tickets {
        assert_eq!(fixture.verdict(ticket), Some(format!("landed {main}")));
        assert!(!ticket.exists());
    }
}

#[test]
fn serve_several_commits_lands_each_commit_after_one_check() {
    let mut fixture = Fixture::new();
    let split = fixture.lane("split", "one.txt", "one");
    fixture.commit(&split, "two.txt", "two", "second change from split");
    fixture.commit(&split, "three.txt", "three", "third change from split");
    let (path, mut ticket) = fixture.ticket(&split, None);
    ticket.several_commits = Some("each commit reverts on its own".to_owned());
    queue::write(&path, &ticket).unwrap();
    let after = fixture.lane("after", "after.txt", "after");
    fixture.enqueue(&after, None);
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.subjects(), ["change from split", "second change from split", "third change from split", "change from after"]);
    assert_eq!(fixture.checks().len(), 1, "the candidate is checked once");
    let done = queue::read(&path.parent().unwrap().join("done").join(path.file_name().unwrap())).unwrap();
    assert_eq!(done.verdict, Some(format!("landed {}", fixture.main())));
    assert_eq!(done.several_commits.as_deref(), Some("each commit reverts on its own"));
}

#[test]
fn serve_red_fix_first() {
    let mut fixture = Fixture::new();
    let a = fixture.lane("a", "a.txt", "a");
    let fix = fixture.lane("fix", "fix.txt", "fix");
    fixture.enqueue(&a, None);
    fixture.enqueue(&fix, Some(RedFix::Marker));
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.subjects(), ["change from fix", "change from a"]);
}

#[test]
fn serve_reenqueue_replaces() {
    let mut fixture = Fixture::new();
    let a = fixture.lane("a", "a.txt", "first");
    let older = fixture.enqueue(&a, None);
    git(&a, &["reset", "-q", "--hard", "HEAD~1"]);
    fixture.commit(&a, "a.txt", "second", "change from a, again");
    let newer = fixture.enqueue(&a, None);
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.subjects(), ["change from a, again"]);
    assert_eq!(git(&fixture.origin, &["show", "main:a.txt"]), "second");
    assert!(fixture.verdict(&older).unwrap().starts_with("replaced"));
    assert_eq!(fixture.verdict(&newer), Some(format!("landed {}", fixture.main())));
}

#[test]
fn serve_cas_lost() {
    let mut fixture = Fixture::new();
    let a = fixture.lane("a", "a.txt", "a");
    let ticket = fixture.enqueue(&a, None);
    let racer = fixture.root.join("racer");
    git(&fixture.root, &["clone", "-q", fixture.origin.to_str().unwrap(), racer.to_str().unwrap()]);
    git(&racer, &["config", "user.name", "Racer"]);
    git(&racer, &["config", "user.email", "racer@example.invalid"]);
    executable(
        &fixture.trace.join("hook"),
        &format!("#!/usr/bin/env bash\n[ \"$1\" = 1 ] || exit 0\ncd {}\necho race >race.txt\ncommand git add race.txt\ncommand git commit -qm 'per-lane landing under the lander'\ncommand git push -q origin main\n", racer.display()),
    );
    assert!(fixture.serve().status.success());
    let checks = fixture.checks();
    assert_eq!(checks.len(), 2, "a lost lease rebuilds and checks again");
    let race = git(&racer, &["rev-parse", "HEAD"]);
    assert_eq!(checks[1].1, race, "the rebuilt candidate is on the new main");
    assert_eq!(fixture.main(), checks[1].0);
    assert_eq!(fixture.subjects(), ["per-lane landing under the lander", "change from a"]);
    assert_eq!(fixture.verdict(&ticket), Some(format!("landed {}", fixture.main())));
}

#[test]
fn serve_pushes_checked_tree() {
    let mut fixture = Fixture::new();
    for name in ["a", "b"] {
        let lane = fixture.lane(name, &format!("{name}.txt"), name);
        fixture.enqueue(&lane, None);
    }
    assert!(fixture.serve().status.success());
    let checks = fixture.checks();
    assert_eq!(checks.len(), 1);
    assert_eq!(fixture.main(), checks[0].0, "main is the very commit that was checked");
    assert_eq!(checks[0].1, fixture.base);
}

#[test]
fn serve_candidate_head_unchanged() {
    let mut fixture = Fixture::new();
    let a = fixture.lane("a", "a.txt", "a");
    fs::write(a.join("scratch.txt"), "untracked\n").unwrap();
    fs::write(a.join("app.txt"), "edited but not committed\n").unwrap();
    let before = (git(&a, &["rev-parse", "HEAD"]), git(&a, &["symbolic-ref", "HEAD"]), git(&a, &["status", "--porcelain"]));
    fixture.enqueue(&a, None);
    assert!(fixture.serve().status.success());
    assert_eq!((git(&a, &["rev-parse", "HEAD"]), git(&a, &["symbolic-ref", "HEAD"]), git(&a, &["status", "--porcelain"])), before);
    assert_eq!(fixture.subjects(), ["change from a"]);

    let b = fixture.lane("b", "b.txt", "b");
    let ticket = fixture.enqueue(&b, None);
    let main = fixture.main();
    let lander = fixture.root.join("proj/worktrees/lander");
    executable(&fixture.trace.join("hook"), &format!("#!/usr/bin/env bash\ncommand git -C {} commit -q --allow-empty -m moved\n", lander.display()));
    let output = fixture.serve();
    assert_eq!(output.status.code(), Some(75));
    assert!(String::from_utf8_lossy(&output.stdout).contains("moved while candidate"));
    assert_eq!(fixture.main(), main, "nothing is pushed when the lander worktree moved during the check");
    assert!(ticket.exists(), "the lane stays queued");
}

#[test]
fn serve_gate_refusal() {
    let mut fixture = Fixture::new();
    let a = fixture.lane("a", "a.txt", "a");
    let ticket = fixture.enqueue(&a, None);
    fs::write(fixture.trace.join("verdict"), "refused https://github.com/o/r/actions/runs/7").unwrap();
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.main(), fixture.base);
    assert_eq!(fixture.verdict(&ticket).as_deref(), Some("failed https://github.com/o/r/actions/runs/7"));
    assert_eq!(fixture.read("pushes"), "", "a refused candidate is not pushed");
}

fn refuse_pushes(fixture: &Fixture, message: &str) {
    executable(&fixture.origin.join("hooks/pre-receive"), &format!("#!/usr/bin/env bash\necho '{message}' >&2\nexit 1\n"));
}

#[test]
fn serve_archived_origin_refusal_is_terminal() {
    let mut fixture = Fixture::new();
    let a = fixture.lane("a", "a.txt", "a");
    let ticket = fixture.enqueue(&a, None);
    refuse_pushes(&fixture, "This repository was archived so it is read-only.");
    let output = fixture.serve();
    assert!(output.status.success(), "a terminal refusal decides the ticket");
    assert_eq!(fixture.main(), fixture.base);
    assert_eq!(fixture.read("pushes").lines().count(), 1, "the push is attempted once");
    let verdict = fixture.verdict(&ticket).unwrap();
    assert!(verdict.starts_with("failed origin refused the push of ") && verdict.ends_with(": remote: This repository was archived so it is read-only."), "{verdict}");
    let fleet = fixture.fleet();
    assert!(fleet.len() == 1 && fleet[0].starts_with("fleet send --from service:north-land session-lane-a north land: lane-a ") && fleet[0].contains("This repository was archived so it is read-only."), "{fleet:?}");
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.checks().len(), 1, "no further candidate is checked for the refused ticket");
    assert_eq!(fixture.read("pushes").lines().count(), 1);
}

#[test]
fn serve_other_origin_refusal_waits_and_keeps_the_ticket() {
    let mut fixture = Fixture::new();
    let a = fixture.lane("a", "a.txt", "a");
    let ticket = fixture.enqueue(&a, None);
    refuse_pushes(&fixture, "the server is having a moment");
    let output = fixture.serve();
    assert_eq!(output.status.code(), Some(75));
    assert!(String::from_utf8_lossy(&output.stdout).contains("waiting: origin refused the push of "), "{}", String::from_utf8_lossy(&output.stdout));
    assert!(ticket.exists(), "a refusal of unknown cause keeps the lane queued");
    assert_eq!(fixture.read("pushes").lines().count(), 1);
    assert!(fixture.fleet().is_empty(), "{:?}", fixture.fleet());
}
#[test]
fn serve_infrastructure_verdicts_keep_tickets() {
    for class in ["red-main abc", "transient fetch", "deferred DEFER_QUEUED"] {
        let mut fixture = Fixture::new();
        let a = fixture.lane("a", "a.txt", "a");
        let ticket = fixture.enqueue(&a, None);
        fs::write(fixture.trace.join("verdict"), class).unwrap();
        assert_eq!(fixture.serve().status.code(), Some(75));
        assert!(ticket.exists(), "{class} decided the lane");
        assert_eq!(fixture.main(), fixture.base);
    }
}

#[test]
fn serve_stale_revert() {
    let mut fixture = Fixture::new();
    fixture.commit(&fixture.repo, "app.txt", "alpha\nbeta\ngamma\ndelta\nepsilon\nzeta\n", "x");
    fixture.commit(&fixture.repo, "app.txt", "alpha changed by y\nbeta\ngamma\ndelta\nepsilon\nzeta\n", "y");
    git(&fixture.repo, &["push", "-q", "origin", "main"]);
    let main = fixture.main();
    let a = fixture.lane("a", "notes.txt", "lane work\n");
    git(&a, &["checkout", &fixture.base, "--", "app.txt"]);
    git(&a, &["commit", "-q", "--amend", "-am", "lane copies app.txt from an older ref"]);
    let ticket = fixture.enqueue(&a, None);
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.main(), main);
    let verdict = fixture.verdict(&ticket).unwrap();
    let log = verdict.strip_prefix("failed ").unwrap();
    assert!(fs::read_to_string(log).unwrap().contains("undoes recent commits"));
    assert!(fixture.checks().is_empty());
}

#[test]
fn serve_already_on_main() {
    let mut fixture = Fixture::new();
    let a = fixture.lane("a", "a.txt", "a");
    let ticket = fixture.enqueue(&a, None);
    git(&fixture.repo, &["cherry-pick", &git(&a, &["rev-parse", "HEAD"])]);
    git(&fixture.repo, &["push", "-q", "origin", "main"]);
    let main = fixture.main();
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.main(), main);
    assert_eq!(fixture.verdict(&ticket), Some(format!("landed {main}")));
    assert!(fixture.checks().is_empty());
    assert_eq!(fixture.read("pushes"), "");
}

#[test]
fn serve_token_scoped() {
    let mut fixture = Fixture::new();
    let a = fixture.lane("a", "a.txt", "a");
    fixture.enqueue(&a, None);
    assert!(fixture.serve().status.success());
    let env = fixture.read("env-1");
    assert!(env.contains("LAND_TRACE="));
    for name in ["GH_TOKEN=", "GITHUB_TOKEN=", "SOPS_AGE_KEY_FILE="] {
        assert!(!env.lines().any(|line| line.starts_with(name)), "the check saw {name}");
    }
    assert!(fixture.read("pushes").contains("token=push-token"), "the push step keeps its token");
}

#[test]
fn serve_all_serves_every_repository_without_sharing() {
    let north = Fixture::new();
    let mut south = Fixture::new();
    south.state = north.state.clone();
    let north_lane = north.lane("a", "a.txt", "a");
    let south_lane = south.lane("b", "b.txt", "b");
    for (fixture, lane) in [(&north, &north_lane), (&south, &south_lane)] {
        assert!(fixture.land_enqueue(lane, &fixture.state, "session").status.success());
    }
    assert_ne!(north.queue(), south.queue(), "each origin has its own queue directory");
    assert_eq!(queue::pending(&north.queue()).unwrap().len(), 1);
    assert_eq!(queue::pending(&south.queue()).unwrap().len(), 1);
    let output = north.command_with(["--all"]).output().unwrap();
    eprintln!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success());
    assert_eq!(north.subjects(), ["change from a"]);
    assert_eq!(south.subjects(), ["change from b"]);
    for fixture in [&north, &south] {
        assert!(queue::pending(&fixture.queue()).unwrap().is_empty());
        let landed = fs::read_dir(fixture.queue().join("done")).unwrap().filter_map(Result::ok).filter_map(|entry| queue::read(&entry.path()).ok()).filter(|ticket| ticket.verdict.as_deref() == Some(&format!("landed {}", fixture.main()))).count();
        assert_eq!(landed, 1, "the repository's own ticket is done in its own queue");
        assert!(fixture.root.join("proj/worktrees/lander/.git").exists());
        assert!(!fixture.queue().join("lock").exists());
    }
    assert_ne!(north.root.join("proj/worktrees/lander"), south.root.join("proj/worktrees/lander"));
    let checked: Vec<String> = north.checks().into_iter().map(|(head, _)| head).collect();
    assert_eq!(checked.len(), 2, "each repository's candidate is checked once: {checked:?}");
    assert!(checked.contains(&north.main()) && checked.contains(&south.main()), "each candidate was checked in its own checkout and is what landed: {checked:?}");
}

#[test]
fn serve_reaps_a_previous_landers_remote_offload_runs_before_it_serves() {
    let mut fixture = Fixture::new();
    let a = fixture.lane("a", "a.txt", "a");
    fixture.enqueue(&a, None);
    assert!(fixture.serve().status.success());
    let calls = fixture.read("north-calls");
    let reap = calls.lines().position(|line| line == "capacity offload reap --owner north-land").expect(&calls);
    let first_check = fixture.read("checks");
    assert!(!first_check.is_empty());
    assert_eq!(reap, 0, "the reap is the first thing the lander asks of north: {calls}");
}

#[test]
fn serve_marks_its_checks_with_the_offload_owner_the_reaper_matches() {
    let mut fixture = Fixture::new();
    let a = fixture.lane("a", "a.txt", "a");
    fixture.enqueue(&a, None);
    assert!(fixture.serve().status.success());
    assert!(fixture.read("env-1").lines().any(|line| line == "NORTH_OFFLOAD_OWNER=north-land"));
}

#[test]
fn serve_github_writes() {
    let mut fixture = Fixture::new();
    for name in ["a", "b", "c"] {
        let lane = fixture.lane(name, &format!("{name}.txt"), name);
        fixture.enqueue(&lane, None);
    }
    assert!(fixture.serve().status.success());
    let pushes = fixture.read("pushes");
    assert_eq!(pushes.lines().count(), 1, "{pushes}");
    assert!(pushes.contains(":refs/heads/main") && pushes.contains("--force-with-lease=refs/heads/main:"));
    let github = fixture.read("github");
    assert_eq!(github.lines().filter(|line| *line == "land-check ref push").count(), 1);
    assert_eq!(github.lines().filter(|line| *line == "dispatch").count(), 1);
    assert_eq!(github.lines().filter(|line| *line == "ref delete").count(), 1);
}

fn worktree_state(root: &Path) -> (String, String) {
    (git(root, &["rev-parse", "HEAD"]), git(root, &["status", "--porcelain"]))
}

fn tree(fixture: &Fixture, commit: &str) -> String {
    git(&fixture.repo, &["rev-parse", &format!("{commit}^{{tree}}")])
}

#[test]
fn serve_tells_the_culprit_its_own_refusal_reason() {
    let mut fixture = Fixture::new();
    let tickets: Vec<PathBuf> = ["a", "b"].iter().map(|name| {
        let lane = fixture.lane(name, &format!("{name}.txt"), name);
        fixture.enqueue(&lane, None)
    }).collect();
    executable(
        &fixture.trace.join("hook"),
        r#"#!/usr/bin/env bash
command git cat-file -e HEAD:b.txt 2>/dev/null || exit 0
echo refused >"$LAND_TRACE/verdict-$1"
for k in $(seq 300); do echo "   Compiling crate-$k v0.1.0"; done
echo 'dep-gate: crates/x/Cargo.toml: proptest: added dependency needs a reason'
printf 'error[E0425]: cannot find value `lane_b` in this scope\n --> src/lib.rs:2:5\n\n'
echo 'running 2 tests'
printf -- '---- lane_b::adds stdout ----\nthread %s panicked at src/lib.rs:4:9:\nassertion `left == right` failed in check %s\n  left: 1\n right: 2\nnote: run with `RUST_BACKTRACE=1`\n' "'lane_b::adds'" "$1"
echo 'safe-push: land check cargo test exited 101 after 12s'
echo "safe-push: ✗ the pre-push checks failed in check $1; NOT pushed."
echo "safe-push: ✗ the pre-push checks failed in check $1; NOT pushed."
"#,
    );
    assert!(fixture.serve().status.success());
    let checks = fixture.checks();
    assert_eq!(checks.len(), 4, "the refused candidate of a and b, each lane alone, then a on its own");
    let n = 1 + (1..3).find(|index| Command::new("git").arg("-C").arg(&fixture.repo).args(["cat-file", "-e", &format!("{}:b.txt", checks[*index].0)]).output().unwrap().status.success()).unwrap();
    assert_eq!(fixture.verdict(&tickets[0]), Some(format!("landed {}", fixture.main())));
    let failure = fixture.verdict(&tickets[1]).unwrap();
    let fleet = fixture.fleet();
    let told: Vec<&String> = fleet.iter().filter(|call| call.starts_with("fleet send --from service:north-land session-lane-b ")).collect();
    assert_eq!(told.len(), 1, "{fleet:?}");
    let lines: Vec<&str> = told[0].trim_start_matches("fleet send --from service:north-land session-lane-b ").split("\\n").collect();
    let log = lines.last().unwrap().strip_prefix("full log: ").unwrap();
    assert!(log.ends_with("-candidate.log") && fs::read_to_string(log).unwrap().contains(&format!("check {n}")), "the full log is the culprit's own check: {log}");
    assert_eq!(
        lines[..9],
        [
            format!("north land: lane-b ({}) did not land: {failure}", git(&fixture.root.join("proj/worktrees/b"), &["rev-parse", "HEAD"])).as_str(),
            "dep-gate: crates/x/Cargo.toml: proptest: added dependency needs a reason",
            "error[E0425]: cannot find value `lane_b` in this scope",
            " --> src/lib.rs:2:5",
            "---- lane_b::adds stdout ----",
            "thread 'lane_b::adds' panicked at src/lib.rs:4:9:",
            format!("assertion `left == right` failed in check {n}").as_str(),
            "safe-push: land check cargo test exited 101 after 12s",
            format!("safe-push: ✗ the pre-push checks failed in check {n}; NOT pushed.").as_str(),
        ]
    );
    let tail = lines.iter().position(|line| *line == "last lines of the log:").expect("the log tail follows the errors");
    assert_eq!(lines[tail - 1], "safe-push: ✗ the pre-push checks failed in check 3; NOT pushed.");
    let tail_lines = &lines[tail + 1..lines.len() - 1];
    assert!(tail_lines.contains(&"running 2 tests") && tail_lines.contains(&"  left: 1") && tail_lines.len() <= north_landing::land::refusal::TAIL_LINES, "{tail_lines:?}");
    assert!(!tail_lines.iter().any(|line| line.starts_with("   Compiling crate-1 ")), "{tail_lines:?}");
    let reason: usize = lines[1..lines.len() - 1].iter().map(|line| line.len() + 1).sum();
    assert!(reason <= north_landing::land::refusal::BOUND + 1, "{reason}");
}

#[test]
fn serve_mixed() {
    let mut fixture = Fixture::new();
    let lanes: Vec<PathBuf> = [("a", "a.txt"), ("b", "shared.txt"), ("c", "shared.txt"), ("d", "fail.txt"), ("e", "e.txt")]
        .iter()
        .map(|(name, file)| fixture.lane(name, file, name))
        .collect();
    fs::write(lanes[2].join("scratch.txt"), "untracked\n").unwrap();
    let before: Vec<_> = lanes.iter().map(|lane| worktree_state(lane)).collect();
    let tickets: Vec<PathBuf> = lanes.iter().map(|lane| fixture.enqueue(lane, None)).collect();
    git(&fixture.repo, &["cherry-pick", &git(&lanes[4], &["rev-parse", "HEAD"])]);
    git(&fixture.repo, &["push", "-q", "origin", "main"]);
    let e_landed = fixture.main();
    executable(
        &fixture.trace.join("hook"),
        "#!/usr/bin/env bash\n! command git cat-file -e HEAD:fail.txt 2>/dev/null || echo 'refused https://github.com/o/r/actions/runs/4242' >\"$LAND_TRACE/verdict-$1\"\n",
    );
    assert!(fixture.serve().status.success());
    let main = fixture.main();
    assert_eq!(fixture.verdict(&tickets[4]), Some(format!("landed {e_landed}")), "a lane already on main counts as landed");
    assert_eq!(fixture.verdict(&tickets[0]), Some(format!("landed {main}")));
    assert_eq!(fixture.verdict(&tickets[1]), Some(format!("landed {main}")));
    assert_eq!(fixture.verdict(&tickets[2]).as_deref(), Some("conflict shared.txt"));
    assert_eq!(fixture.verdict(&tickets[3]).as_deref(), Some("failed https://github.com/o/r/actions/runs/4242"));
    assert_eq!(git(&fixture.origin, &["show", "main:shared.txt"]), "b");
    git(&fixture.origin, &["cat-file", "-e", "main:a.txt"]);
    assert!(git(&fixture.origin, &["ls-tree", "--name-only", "main"]).lines().all(|file| file != "fail.txt"), "the failing lane landed");
    let checks = fixture.checks();
    assert_eq!(checks.len(), 5, "the refused candidate of a, b and d, each lane alone, then a and b together");
    assert!(checks.iter().any(|(checked, _)| *checked == main), "main is a commit the checks passed");
    let fleet = fixture.fleet();
    assert!(fleet.iter().any(|line| line.starts_with("fleet send --from service:north-land session-lane-d ") && line.contains("did not land") && line.contains("https://github.com/o/r/actions/runs/4242")), "{fleet:?}");
    assert!(fleet.iter().any(|line| line.starts_with("fleet send --from service:north-land session-lane-c ") && line.contains("did not land") && line.contains("shared.txt")), "{fleet:?}");
    for (lane, sha) in [("a", &main), ("b", &main), ("e", &e_landed)] {
        assert!(fleet.contains(&format!("fleet send --from service:north-land session-lane-{lane} landed lane-{lane} as {sha}")), "{fleet:?}");
    }
    assert_eq!(fleet.len(), 5, "each lane is told once: {fleet:?}");
    for landed in [0, 1, 4] {
        assert!(!lanes[landed].exists(), "a landed clean lane is removed");
    }
    for kept in [2, 3] {
        assert_eq!(worktree_state(&lanes[kept]), before[kept], "the lander touches only its own worktree and the lanes it landed");
    }
}

#[test]
fn serve_lands_a_lane_whose_only_conflict_was_a_refused_lane_ahead_of_it() {
    let mut fixture = Fixture::new();
    let a = fixture.lane("a", "app.txt", "from a\n");
    fixture.commit(&a, "fail.txt", "fail\n", "fail marker from a");
    let b = fixture.lane("b", "app.txt", "from b\n");
    let tickets = [fixture.enqueue(&a, None), fixture.enqueue(&b, None)];
    executable(
        &fixture.trace.join("hook"),
        "#!/usr/bin/env bash\n! command git cat-file -e HEAD:fail.txt 2>/dev/null || echo 'refused https://github.com/o/r/actions/runs/4343' >\"$LAND_TRACE/verdict-$1\"\n",
    );
    let output = fixture.serve();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("lane-b conflicts only with lanes ahead of it in this candidate (app.txt)"));
    assert_eq!(fixture.verdict(&tickets[0]).as_deref(), Some("failed https://github.com/o/r/actions/runs/4343"));
    assert_eq!(fixture.verdict(&tickets[1]), Some(format!("landed {}", fixture.main())));
    assert_eq!(git(&fixture.origin, &["show", "main:app.txt"]), "from b");
}

#[test]
fn serve_captured_candidate() {
    let mut fixture = Fixture::new();
    let a = fixture.lane("a", "new.py", "print('a')\n");
    let b = fixture.lane("b", "clean.txt", "clean\n");
    let bad = fixture.enqueue(&a, None);
    let good = fixture.enqueue(&b, None);
    executable(
        &fixture.trace.join("hook"),
        &format!(
            "#!/usr/bin/env bash\ncommand git cat-file -e HEAD:new.py 2>/dev/null || exit 0\ncommand git rev-parse HEAD >>\"$LAND_TRACE/captured\"\ncommand git reset -q --hard {}\necho refused >\"$LAND_TRACE/verdict-$1\"\n",
            fixture.base
        ),
    );
    assert!(fixture.serve().status.success());
    let captured = fixture.read("captured");
    assert_eq!(captured.lines().count(), 2, "the refused pair, then the Python lane alone");
    for candidate in captured.lines() {
        git(&fixture.repo, &["cat-file", "-e", &format!("{candidate}:new.py")]);
    }
    assert!(fixture.verdict(&bad).unwrap().starts_with("failed "), "the lane whose candidate moved HEAD is refused");
    assert_eq!(fixture.verdict(&good), Some(format!("landed {}", fixture.main())));
    assert_eq!(fixture.subjects(), ["change from b"]);
    assert!(git(&fixture.origin, &["ls-tree", "--name-only", "main"]).lines().all(|file| file != "new.py"), "the captured Python candidate was published");
    assert_eq!(fixture.read("pushes").lines().count(), 1);
}

#[test]
fn serve_transient_not_culprit() {
    let mut fixture = Fixture::new();
    let tickets: Vec<PathBuf> = ["a", "b", "c"].iter().map(|name| {
        let lane = fixture.lane(name, &format!("{name}.txt"), name);
        fixture.enqueue(&lane, None)
    }).collect();
    fs::write(fixture.trace.join("verdict-1"), "refused https://github.com/o/r/actions/runs/1").unwrap();
    fs::write(fixture.trace.join("verdict-2"), "refused https://github.com/o/r/actions/runs/2").unwrap();
    fs::write(fixture.trace.join("exit-2"), "75").unwrap();
    assert_eq!(fixture.serve().status.code(), Some(75));
    assert_eq!(fixture.checks().len(), 4, "the refused candidate, then each lane alone");
    assert!(tickets.iter().all(|ticket| ticket.exists()), "an exit-75 check decided a lane");
    assert_eq!(fixture.main(), fixture.base);
    assert!(fixture.fleet().is_empty(), "{:?}", fixture.fleet());

    let racer = fixture.root.join("racer");
    git(&fixture.root, &["clone", "-q", fixture.origin.to_str().unwrap(), racer.to_str().unwrap()]);
    git(&racer, &["config", "user.name", "Racer"]);
    git(&racer, &["config", "user.email", "racer@example.invalid"]);
    fs::write(fixture.trace.join("verdict-5"), "refused https://github.com/o/r/actions/runs/5").unwrap();
    executable(
        &fixture.trace.join("hook"),
        &format!("#!/usr/bin/env bash\n[ \"$1\" = 5 ] || exit 0\ncd {}\necho race >race.txt\ncommand git add race.txt\ncommand git commit -qm 'main moves during the check'\ncommand git push -q origin main\n", racer.display()),
    );
    assert!(fixture.serve().status.success());
    let checks = fixture.checks();
    assert_eq!(checks.len(), 6, "a refusal on a moved base is rebuilt, not searched");
    assert_eq!(checks[5].1, git(&racer, &["rev-parse", "HEAD"]));
    assert_eq!(fixture.subjects(), ["main moves during the check", "change from a", "change from b", "change from c"]);
    let main = fixture.main();
    for ticket in &tickets {
        assert_eq!(fixture.verdict(ticket), Some(format!("landed {main}")));
    }
    let mut fleet = fixture.fleet();
    fleet.sort();
    assert_eq!(fleet, ["a", "b", "c"].map(|name| format!("fleet send --from service:north-land session-lane-{name} landed lane-{name} as {main}")), "only landed messages go out");
}

fn persistent_isolates(verdict: &str, exit: &str, class: &str) {
    let mut fixture = Fixture::new();
    let tickets: Vec<PathBuf> = ["a", "b", "c"].iter().map(|name| {
        let lane = fixture.lane(name, &format!("{name}.txt"), name);
        fixture.enqueue(&lane, None)
    }).collect();
    fs::write(fixture.trace.join("poison"), format!("b.txt\n{verdict}\n{exit}\n")).unwrap();
    for _ in 0..2 {
        assert_eq!(fixture.serve().status.code(), Some(75));
    }
    assert_eq!(fixture.checks().len(), 2);
    assert!(tickets.iter().all(|ticket| ticket.exists()), "a {class} decided a lane before the third try");
    assert_eq!(fixture.serve().status.code(), Some(75));
    let checks = fixture.checks();
    assert_eq!(checks.len(), 6, "the third {class} searches each lane alone");
    assert_eq!(tree(&fixture, &checks[0].0), tree(&fixture, &checks[2].0));
    for alone in &checks[3..] {
        assert_eq!(git(&fixture.repo, &["rev-list", "--count", &format!("{}..{}", fixture.base, alone.0)]), "1", "each lane is checked on its own");
    }
    assert!(tickets.iter().all(|ticket| ticket.exists()), "the search decided a lane that only ended without a verdict");
    assert_eq!(fixture.main(), fixture.base);
    let mut rounds = 0;
    while fixture.serve().status.code() == Some(75) {
        rounds += 1;
        assert!(rounds < 12, "the queue never settled");
    }
    let failure = fixture.verdict(&tickets[1]).unwrap();
    assert!(failure.starts_with(&format!("failed persistent {class}: ")) && failure.ends_with("-candidate.log"), "{failure}");
    let main = fixture.main();
    for ticket in [&tickets[0], &tickets[2]] {
        assert!(fixture.verdict(ticket).is_some_and(|verdict| verdict.starts_with("landed ")));
    }
    assert_eq!(fixture.verdict(&tickets[2]), Some(format!("landed {main}")));
    assert_eq!(fixture.subjects(), ["change from a", "change from c"]);
    let fleet = fixture.fleet();
    assert!(fleet.iter().any(|call| call.starts_with("fleet send --from service:north-land session-lane-b ") && call.contains(&format!("did not land: {failure}"))), "{fleet:?}");
    assert!(fleet.iter().filter(|call| call.contains("did not land")).count() == 1, "{fleet:?}");
}

#[test]
fn serve_persistent_transient_isolates() {
    persistent_isolates("green", "75", "transient");
}

#[test]
fn serve_persistent_deferred_isolates() {
    persistent_isolates("deferred DEFER_QUEUED", "0", "deferred");
}

#[test]
fn serve_defer() {
    let mut fixture = Fixture::new();
    let tickets: Vec<PathBuf> = ["a", "b", "c"].iter().map(|name| {
        let lane = fixture.lane(name, &format!("{name}.txt"), name);
        fixture.enqueue(&lane, None)
    }).collect();
    fs::write(fixture.trace.join("verdict-1"), "deferred DEFER_QUEUED").unwrap();
    assert_eq!(fixture.serve().status.code(), Some(75));
    assert!(tickets.iter().all(|ticket| ticket.exists()));
    fs::write(fixture.trace.join("verdict-2"), "refused https://github.com/o/r/actions/runs/2").unwrap();
    fs::write(fixture.trace.join("verdict-3"), "deferred DEFER_QUEUED").unwrap();
    assert_eq!(fixture.serve().status.code(), Some(75));
    assert!(tickets.iter().all(|ticket| ticket.exists()), "a DEFER during the halving ejected a lane");
    assert_eq!(fixture.main(), fixture.base);
    assert_eq!(fixture.read("pushes"), "");
    assert!(fixture.serve().status.success());
    let checks = fixture.checks();
    assert_eq!(checks.len(), 6, "the deferred candidate, the refused candidate, three lanes alone, then the candidate again");
    assert_eq!((tree(&fixture, &checks[5].0), &checks[5].1), (tree(&fixture, &checks[0].0), &checks[0].1), "the deferred candidate is retried as it was");
    let main = fixture.main();
    assert_eq!(main, checks[5].0);
    for ticket in &tickets {
        assert_eq!(fixture.verdict(ticket), Some(format!("landed {main}")));
    }
}

#[test]
fn serve_red_main_lands_fixes_first() {
    let mut fixture = Fixture::new();
    let a = fixture.lane("a", "a.txt", "a");
    let fix = fixture.lane("fix", "fix.txt", "fix");
    let held = fixture.enqueue(&a, None);
    let fixed = fixture.enqueue(&fix, Some(RedFix::Marker));
    fs::write(fixture.trace.join("verdict-1"), "red-main abc").unwrap();
    fs::write(fixture.trace.join("verdict-3"), "red-main abc").unwrap();
    assert_eq!(fixture.serve().status.code(), Some(75));
    assert_eq!(fixture.subjects(), ["change from fix"]);
    assert_eq!(fixture.verdict(&fixed), Some(format!("landed {}", fixture.main())));
    assert!(held.exists(), "red main decided the held lane");
    let checks = fixture.checks();
    assert_eq!(checks.len(), 3);
    assert_eq!(git(&fixture.repo, &["ls-tree", "--name-only", &checks[1].0]), "app.txt\nfix.txt", "the red-fix lane is checked alone");
}

#[test]
fn serve_dropped_commit() {
    let mut fixture = Fixture::new();
    let a = fixture.lane("a", "a.txt", "a");
    let dropped = git(&a, &["rev-parse", "HEAD"]);
    fixture.commit(&a, "b.txt", "second", "second change from a");
    let ticket = fixture.enqueue(&a, None);
    fs::write(fixture.repo.join("a.txt"), "a").unwrap();
    fixture.commit(&fixture.repo, "other.txt", "other", "a.txt and more from elsewhere");
    git(&fixture.repo, &["push", "-q", "origin", "main"]);
    let main = fixture.main();
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.main(), main, "the refused lane moved main");
    let verdict = fixture.verdict(&ticket).unwrap();
    let log = verdict.strip_prefix("failed ").unwrap_or_else(|| panic!("{verdict}"));
    let text = fs::read_to_string(log).unwrap();
    assert!(text.contains("lane commits missing") && text.contains(&dropped), "{text}");
    assert!(fixture.checks().is_empty());
    let fleet = fixture.fleet();
    assert!(fleet.len() == 1 && fleet[0].contains(log), "{fleet:?}");
}

#[test]
fn serve_context_only() {
    let mut fixture = Fixture::new();
    let twenty: String = (1..=20).map(|line| format!("{line}\n")).collect();
    fixture.commit(&fixture.repo, "shared.txt", &twenty, "twenty shared lines");
    git(&fixture.repo, &["push", "-q", "origin", "main"]);
    let a = fixture.lane("a", "a.txt", "a");
    let edit = |text: &str, lines: &[(usize, &str)]| -> String {
        text.lines().enumerate().map(|(index, line)| format!("{}\n", lines.iter().find(|(at, _)| *at == index + 1).map_or(line, |(_, new)| *new))).collect()
    };
    fixture.commit(&a, "shared.txt", &edit(&twenty, &[(2, "two"), (18, "eighteen")]), "a edits two shared lines");
    let ticket = fixture.enqueue(&a, None);
    fs::write(fixture.repo.join("other.txt"), "other\n").unwrap();
    fixture.commit(&fixture.repo, "shared.txt", &edit(&twenty, &[(2, "two")]), "main edits the first of them");
    git(&fixture.repo, &["push", "-q", "origin", "main"]);
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.verdict(&ticket), Some(format!("landed {}", fixture.main())), "a lane whose diff changed only in context was refused");
    assert!(git(&fixture.origin, &["show", "main:shared.txt"]).lines().any(|line| line == "eighteen"));
    git(&fixture.origin, &["cat-file", "-e", "main:a.txt"]);
}

fn kill_lander_hook(fixture: &Fixture) {
    executable(&fixture.trace.join("hook"), "#!/usr/bin/env bash\n[ \"$1\" = 1 ] || exit 0\nkill -9 \"$(awk '{print $4}' /proc/$PPID/stat)\"\nkill -9 $PPID\n");
}

fn three_lanes(fixture: &mut Fixture) -> Vec<PathBuf> {
    ["a", "b", "c"]
        .iter()
        .map(|name| {
            let lane = fixture.lane(name, &format!("{name}.txt"), name);
            fixture.enqueue(&lane, None)
        })
        .collect()
}

#[test]
fn serve_landed_only_on_main() {
    let mut fixture = Fixture::new();
    three_lanes(&mut fixture);
    assert!(fixture.serve().status.success());
    let main = fixture.main();
    let mut sent: Vec<String> = fixture.read("sent").lines().map(str::to_owned).collect();
    sent.sort();
    let expected: Vec<String> = ["a", "b", "c"].iter().map(|name| format!("session-lane-{name} landed lane-{name} as {main}")).collect();
    assert_eq!(sent, expected);
    let cherries = fixture.read("send-cherry");
    assert_eq!(cherries.lines().count(), 3);
    for line in cherries.lines() {
        assert!(!line.contains('+'), "a landed message went out while the lane commit was not on main: {line}");
        assert!(line.contains('-'), "{line}");
    }
    let rows = fixture.pushes_tsv();
    assert_eq!(rows.len(), 3);
    for row in &rows {
        assert_eq!(row.len(), 6, "{row:?}");
        assert!(row[0].parse::<u64>().is_ok());
        assert_eq!(row[1], "origin");
        assert_eq!(row[4], main);
        assert_eq!(row[5], format!("session-{}", row[2]));
    }
    let stages = fs::read_to_string(fixture.state.join("safe-push/stages.tsv")).unwrap();
    assert_eq!(stages.lines().filter(|line| line.split('\t').nth(3) == Some("check")).count(), 3);
    assert_eq!(stages.lines().filter(|line| line.split('\t').nth(3) == Some("push")).count(), 3);

    let mut refused = Fixture::new();
    let lane = refused.lane("a", "a.txt", "a");
    refused.enqueue(&lane, None);
    fs::write(refused.trace.join("verdict"), "refused https://github.com/o/r/actions/runs/7").unwrap();
    assert!(refused.serve().status.success());
    let told = refused.fleet();
    assert!(told.len() == 1 && told[0].starts_with("fleet send --from service:north-land session-lane-a north land: lane-a ") && told[0].contains("did not land"), "nothing landed, so the lane is told only its refusal: {told:?}");
    assert!(followed(&refused).is_empty(), "nothing landed, so nothing is followed");
    assert!(refused.pushes_tsv().is_empty());
}

fn followed(fixture: &Fixture) -> Vec<String> {
    fixture.read("north-calls").lines().filter(|line| !line.starts_with("fleet") && !line.starts_with("land-train") && !line.starts_with("capacity offload reap") && !line.starts_with("incident")).map(str::to_owned).collect()
}

#[test]
fn serve_after_landing() {
    let mut fixture = Fixture::new();
    three_lanes(&mut fixture);
    assert!(fixture.serve().status.success());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while followed(&fixture).len() < 3 && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let calls = followed(&fixture);
    let main = fixture.main();
    assert_eq!(calls.len(), 3, "{calls:?}");
    assert!(calls[0].starts_with("after-landing --git-dir ") && calls[0].contains(&format!("--base {} --head {main}", fixture.base)), "{calls:?}");
    assert!(calls[1].starts_with("follows-release --repo-dir "), "{calls:?}");
    assert!(calls[2].starts_with("runtime follow --repo-dir "), "{calls:?}");
    assert_eq!(fixture.fleet().len(), 3);
    assert!(fixture.serve().status.success());
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert_eq!(followed(&fixture).len(), 3, "an idle round follows nothing again");
}

#[test]
fn serve_restart() {
    let mut fixture = Fixture::new();
    let tickets = three_lanes(&mut fixture);
    kill_lander_hook(&fixture);
    let killed = fixture.serve();
    assert_eq!(killed.status.code(), None, "the lander was killed mid-check");
    assert_eq!(fixture.main(), fixture.base);
    assert_eq!(fixture.read("pushes"), "");
    assert!(tickets.iter().all(|ticket| ticket.exists()));
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.subjects(), ["change from a", "change from b", "change from c"]);
    assert_eq!(fixture.read("pushes").lines().count(), 1, "nothing is pushed twice");
    let main = fixture.main();
    for ticket in &tickets {
        assert_eq!(fixture.verdict(ticket), Some(format!("landed {main}")));
    }
    assert_eq!(fixture.pushes_tsv().len(), 3);
    assert_eq!(fixture.fleet().len(), 3);
}

#[test]
fn serve_restart_after_push() {
    let mut fixture = Fixture::new();
    let tickets = three_lanes(&mut fixture);
    fs::write(fixture.trace.join("kill-after-push"), "").unwrap();
    let killed = fixture.serve();
    assert_eq!(killed.status.code(), None, "the lander was killed right after the push");
    let main = fixture.main();
    assert_ne!(main, fixture.base);
    assert!(tickets.iter().all(|ticket| ticket.exists()), "the lander died before it decided the tickets");
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.main(), main);
    assert_eq!(fixture.subjects(), ["change from a", "change from b", "change from c"]);
    assert_eq!(fixture.read("pushes").lines().count(), 1, "nothing is pushed twice");
    assert_eq!(fixture.checks().len(), 1, "the restarted lander does not check the landed lanes again");
    for ticket in &tickets {
        assert_eq!(fixture.verdict(ticket), Some(format!("landed {main}")));
    }
    assert_eq!(fixture.pushes_tsv().len(), 3);
    assert_eq!(fixture.fleet().len(), 3);
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.pushes_tsv().len(), 3);
    assert_eq!(fixture.fleet().len(), 3);
}

#[test]
fn serve_reentry() {
    let mut fixture = Fixture::new();
    let a = fixture.lane("a", "a.txt", "a");
    let ticket = fixture.enqueue(&a, None);
    executable(
        &fixture.trace.join("hook"),
        &format!(
            "#!/usr/bin/env bash\n[ \"$1\" = 1 ] || exit 0\ntimeout 20 {} land enqueue >\"$LAND_TRACE/reentry-enqueue\" 2>&1\necho $? >\"$LAND_TRACE/reentry-status\"\ncommand git push -q origin HEAD:refs/heads/scratch/x\n",
            env!("CARGO_BIN_EXE_north")
        ),
    );
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.read("reentry-status").trim(), "0", "{}", fixture.read("reentry-enqueue"));
    assert!(fixture.read("reentry-enqueue").contains("not queued"), "{}", fixture.read("reentry-enqueue"));
    assert!(queue::pending(&fixture.queue()).unwrap().is_empty(), "the nested enqueue left a ticket");
    assert_eq!(git(&fixture.origin, &["rev-parse", "refs/heads/scratch/x"]), git(&fixture.origin, &["rev-parse", "refs/heads/main"]));
    assert_eq!(fixture.subjects(), ["change from a"]);
    assert_eq!(fixture.verdict(&ticket), Some(format!("landed {}", fixture.main())));
}

struct Serving(std::process::Child);

impl Drop for Serving {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_for_exit(serving: &mut Serving, seconds: u64) -> Option<std::process::ExitStatus> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(seconds);
    while std::time::Instant::now() < deadline {
        if let Some(status) = serving.0.try_wait().unwrap() {
            return Some(status);
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    None
}

#[test]
fn serve_loop_exits_after_runtime_follow() {
    let mut fixture = Fixture::new();
    let link = fixture.root.join("runtime-current");
    std::os::unix::fs::symlink("gen-1", &link).unwrap();
    let path = format!("{}:{}", fixture.bin.display(), std::env::var("PATH").unwrap());
    let child = Command::new(env!("CARGO_BIN_EXE_north"))
        .args(["land", "serve", "--repo", fixture.repo.to_str().unwrap(), "--checker", fixture.trace.join("checker").to_str().unwrap()])
        .env("XDG_STATE_HOME", &fixture.state)
        .env("NORTH_RUNTIME_LINK", &link)
        .env("PATH", path)
        .env("LAND_TRACE", &fixture.trace)
        .env("NORTH_LAND_NORTH", fixture.trace.join("north"))
        .env("LAND_ORIGIN", &fixture.origin)
        .env("LAND_REPO", &fixture.repo)
        .env("SOPS_AGE_KEY_FILE", fixture.root.join("age.key"))
        .stdin(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let mut serving = Serving(child);
    assert!(wait_for_exit(&mut serving, 5).is_none(), "an empty queue keeps the lander serving");
    let lane = fixture.lane("a", "a.txt", "a");
    fixture.enqueue(&lane, None);
    let status = wait_for_exit(&mut serving, 120).expect("the lander exits once the follow step changed the runtime");
    assert_eq!(status.code(), Some(0));
    assert_eq!(fixture.subjects(), ["change from a"]);
    assert_eq!(fs::read_link(&link).unwrap(), Path::new("gen-2"));
    assert!(followed(&fixture).iter().any(|call| call.starts_with("runtime follow")));
}

#[test]
fn serve_worktree_not_reaped() {
    let fixture = Fixture::new();
    let lanes = fixture.root.join("proj/worktrees");
    let spend = |name: &str| {
        let path = lanes.join(name);
        git(&fixture.repo, &["worktree", "add", "-q", "--detach", path.to_str().unwrap(), "origin/main"]);
        fs::File::options().write(true).open(path.join(".git")).unwrap().set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3 * 3600)).unwrap();
    };
    for name in ["lander", "lander-2", "spent-reap"] {
        spend(name);
    }

    let reap = Command::new(env!("CARGO_BIN_EXE_north")).arg("wt-reap").current_dir(&fixture.repo).output().unwrap();
    assert!(reap.status.success(), "{}", String::from_utf8_lossy(&reap.stderr));
    assert!(!lanes.join("spent-reap").exists(), "the control lane is reaped: {}", String::from_utf8_lossy(&reap.stdout));
    assert!(lanes.join("lander/.git").exists() && lanes.join("lander-2/.git").exists());

    spend("spent-gc");
    let gc = Command::new(env!("CARGO_BIN_EXE_north"))
        .args(["lane-gc", "--root", fixture.root.to_str().unwrap(), "--idle-hours", "0", "--state", fixture.root.join("unlanded.txt").to_str().unwrap()])
        .env("HOME", &fixture.root)
        .output()
        .unwrap();
    let report = format!("{}{}", String::from_utf8_lossy(&gc.stdout), String::from_utf8_lossy(&gc.stderr));
    assert!(!lanes.join("spent-gc").exists(), "the control lane is collected: {report}");
    assert!(lanes.join("lander/.git").exists() && lanes.join("lander-2/.git").exists(), "{report}");
}

const FAKE_SSH: &str = r#"#!/usr/bin/env bash
printf '%s\n' "$*" >>"$LAND_TRACE/ssh"
while [ $# -gt 0 ]; do
  case "$1" in
    -o) shift 2 ;;
    -T) shift ;;
    *) destination=${1#*@}; shift; break ;;
  esac
done
[ "${1:-}" != -- ] || shift
[ -d "$FAKE_REMOTE_HOMES/$destination" ] || { echo "ssh: connect to host $destination port 22: Connection timed out" >&2; exit 255; }
command=$*
case "$command" in git-upload-pack*) command="git upload-pack${command#git-upload-pack}" ;; esac
unset XDG_STATE_HOME
HOME=$FAKE_REMOTE_HOMES/$destination exec sh -c "$command"
"#;

impl Fixture {
    fn landing_config(&mut self, config: &str) {
        executable(&self.bin.join("ssh"), FAKE_SSH);
        self.commit(&self.repo, ".safe-push", &format!("[landing]\n{config}"), "configure landing");
        git(&self.repo, &["push", "-q", "origin", "main"]);
        self.base = self.main();
    }

    fn land_enqueue(&self, lane: &Path, state: &Path, session: &str) -> Output {
        let output = Command::new(env!("CARGO_BIN_EXE_north")).args(["land", "enqueue"]).current_dir(lane).env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1").env("XDG_STATE_HOME", state).env("SAFE_PUSH_SOUTH", state.join("no-south")).env_remove("SAFE_PUSH_DENYLIST").env("NORTH_FLEET_SESSION", session).env_remove("NORTH_LAND_SERVING").output().unwrap();
        eprintln!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        output
    }
}

fn json_names(directory: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(directory).map(|entries| entries.filter_map(Result::ok).map(|entry| entry.file_name().to_string_lossy().into_owned()).filter(|name| name.ends_with(".json")).collect()).unwrap_or_default();
    names.sort();
    names
}

#[test]
fn remote_outbox_lands() {
    let mut fixture = Fixture::new();
    fixture.landing_config("\tlander-host = lander.invalid\n\tremote-hosts = gex45\n");
    let home = fixture.root.join("remote-homes/gex45");
    let clone = fixture.root.join("gex45/proj/main");
    fs::create_dir_all(&home).unwrap();
    git(&fixture.root, &["clone", "-q", fixture.origin.to_str().unwrap(), clone.to_str().unwrap()]);
    git(&clone, &["config", "user.name", "Remote scenario"]);
    git(&clone, &["config", "user.email", "remote@example.invalid"]);
    let lane = fixture.root.join("gex45/proj/worktrees/r");
    git(&clone, &["worktree", "add", "-q", "-b", "lane-r", lane.to_str().unwrap(), "origin/main"]);
    fixture.commit(&lane, "r.txt", "r", "change from r");
    let head = git(&lane, &["rev-parse", "HEAD"]);
    let top = git(&lane, &["rev-parse", "--show-toplevel"]);
    let queued = fixture.land_enqueue(&lane, &home.join(".local/state"), "session-remote");
    assert!(queued.status.success());
    assert!(String::from_utf8_lossy(&queued.stdout).contains(&format!("Done: queued (outbox) {top} lane-r {head}")));
    let outbox = queue::directory(&home.join(".local/state"), fixture.origin.to_str().unwrap()).join(queue::OUTBOX);
    let names = json_names(&outbox);
    assert_eq!(names.len(), 1, "{names:?}");

    assert!(fixture.serve().status.success());
    assert_eq!(fixture.subjects(), ["change from r"]);
    assert!(json_names(&outbox).is_empty(), "the remote ticket is still in the outbox");
    assert_eq!(json_names(&outbox.join("taken")), names);
    let main = fixture.main();
    let local = fixture.queue().join(&names[0]);
    assert_eq!(fixture.verdict(&local), Some(format!("landed {main}")));
    let remote = queue::read(&outbox.join("taken").join(&names[0])).unwrap();
    let done = queue::read(&fixture.queue().join("done").join(&names[0])).unwrap();
    assert_eq!((done.host.as_str(), done.root.as_path()), (remote.host.as_str(), remote.root.as_path()), "the pulled ticket keeps its host and root");
    assert!(fixture.fleet().contains(&format!("fleet send --from service:north-land session-remote landed lane-r as {main}")), "{:?}", fixture.fleet());
    let ssh = fixture.read("ssh");
    assert!(ssh.lines().all(|line| line.contains("ConnectTimeout=5")), "{ssh}");
    assert!(ssh.lines().any(|line| line.contains("git-upload-pack") && line.contains(&top)), "the lane is fetched from ssh://gex45{top}: {ssh}");
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.subjects(), ["change from r"], "a taken ticket was pulled again");
}

#[test]
fn remote_outbox_unreachable() {
    let mut fixture = Fixture::new();
    fixture.landing_config("\tremote-hosts = nexus\n");
    let a = fixture.lane("a", "a.txt", "a");
    let ticket = fixture.enqueue(&a, None);
    let output = fixture.serve();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.lines().any(|line| line.starts_with("north land: waiting: nexus's outbox skipped this round")), "{stdout}");
    assert_eq!(fixture.verdict(&ticket), Some(format!("landed {}", fixture.main())), "an unreachable host blocked the local ticket");
    assert!(fixture.read("ssh").lines().all(|line| line.contains("ConnectTimeout=5")));
}

impl Fixture {
    fn staged_remote_ticket(&mut self, name: &str, before_local: bool) -> (PathBuf, PathBuf) {
        let home = self.root.join("remote-homes/gex45");
        let clone = self.root.join("gex45/proj/main");
        fs::create_dir_all(&home).unwrap();
        git(&self.root, &["clone", "-q", self.origin.to_str().unwrap(), clone.to_str().unwrap()]);
        git(&clone, &["config", "user.name", "Remote scenario"]);
        git(&clone, &["config", "user.email", "remote@example.invalid"]);
        let lane = self.root.join("gex45/proj/worktrees").join(name);
        git(&clone, &["worktree", "add", "-q", "-b", &format!("lane-{name}"), lane.to_str().unwrap(), "origin/main"]);
        self.commit(&lane, &format!("{name}.txt"), name, &format!("change from {name}"));
        assert!(self.land_enqueue(&lane, &home.join(".local/state"), &format!("session-{name}")).status.success());
        let outbox = queue::directory(&home.join(".local/state"), self.origin.to_str().unwrap()).join(queue::OUTBOX);
        let names = json_names(&outbox);
        assert_eq!(names.len(), 1, "{names:?}");
        let staged = self.root.join("staged");
        fs::create_dir_all(&staged).unwrap();
        let staged_name = if before_local { queue::file_name(self.queued - 1_000_000, &format!("lane-{name}")) } else { names[0].clone() };
        fs::rename(outbox.join(&names[0]), staged.join(&staged_name)).unwrap();
        (staged, outbox)
    }

    fn release_on_check(&self, check: usize, staged: &Path, outbox: &Path, extra: &str) {
        executable(
            &self.trace.join("hook"),
            &format!("#!/usr/bin/env bash\n[ \"$1\" = {check} ] || exit 0\nmv {}/*.json {}/\n{extra}", staged.display(), outbox.display()),
        );
    }
}

#[test]
fn remote_outbox_joins_the_rebuilt_candidate() {
    let mut fixture = Fixture::new();
    fixture.landing_config("\tlander-host = lander.invalid\n\tremote-hosts = gex45 nexus\n");
    let a = fixture.lane("a", "a.txt", "a");
    let ticket = fixture.enqueue(&a, None);
    let (staged, outbox) = fixture.staged_remote_ticket("r", false);
    let racer = fixture.root.join("racer");
    git(&fixture.root, &["clone", "-q", fixture.origin.to_str().unwrap(), racer.to_str().unwrap()]);
    git(&racer, &["config", "user.name", "Racer"]);
    git(&racer, &["config", "user.email", "racer@example.invalid"]);
    fixture.release_on_check(1, &staged, &outbox, &format!("cd {}\necho race >race.txt\ncommand git add race.txt\ncommand git commit -qm 'main moves during the check'\ncommand git push -q origin main\n", racer.display()));
    let output = fixture.serve();
    assert!(output.status.success());
    let checks = fixture.checks();
    assert_eq!(checks.len(), 2, "the rebuilt candidate carried the ticket pulled after the first check: {checks:?}");
    assert_eq!(fixture.subjects(), ["main moves during the check", "change from a", "change from r"]);
    assert_eq!(fixture.verdict(&ticket), Some(format!("landed {}", fixture.main())));
    assert!(json_names(&outbox).is_empty(), "the remote ticket is still in the outbox");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.lines().filter(|line| line.starts_with("north land: waiting: nexus's outbox skipped this round")).count(), 2, "once per round, and the second round finds the queue empty: {stdout}");
    assert_eq!(fixture.read("ssh").lines().filter(|line| line.contains("nexus")).count(), 3, "the outbox is pulled before each full candidate");
}

#[test]
fn remote_outbox_waits_out_the_bisect() {
    let mut fixture = Fixture::new();
    fixture.landing_config("\tlander-host = lander.invalid\n\tremote-hosts = gex45\n");
    let tickets: Vec<PathBuf> = ["a", "b"].iter().map(|name| {
        let lane = fixture.lane(name, &format!("{name}.txt"), name);
        fixture.enqueue(&lane, None)
    }).collect();
    let (staged, outbox) = fixture.staged_remote_ticket("r", true);
    fs::write(fixture.trace.join("poison"), "a.txt\nrefused https://github.com/o/r/actions/runs/1\n1\n").unwrap();
    fixture.release_on_check(1, &staged, &outbox, "");
    assert!(fixture.serve().status.success(), "the bisect did not converge");
    assert!(fixture.verdict(&tickets[0]).is_some_and(|verdict| verdict.starts_with("failed ")), "{:?}", fixture.verdict(&tickets[0]));
    let main = fixture.main();
    assert_eq!(fixture.verdict(&tickets[1]), Some(format!("landed {main}")));
    let taken = json_names(&outbox.join("taken"));
    assert!(json_names(&outbox).is_empty() && taken.len() == 1, "the pulled ticket was lost: {taken:?}");
    assert_eq!(fixture.verdict(&fixture.queue().join(&taken[0])), Some(format!("landed {main}")));
    let mut subjects = fixture.subjects();
    subjects.sort();
    assert_eq!(subjects, ["change from b", "change from r"], "each ticket lands once");
    let checks = fixture.checks();
    let carrying_r: Vec<_> = checks.iter().filter(|(head, _)| git(&fixture.repo, &["ls-tree", "--name-only", head]).lines().any(|file| file == "r.txt")).collect();
    assert_eq!(carrying_r.len(), 1, "the pulled ticket was checked more than once: {checks:?}");
    assert_eq!(carrying_r[0].0, main, "the pulled ticket landed in a candidate other than the one checked");
    assert_eq!(git(&fixture.repo, &["rev-list", "--count", &format!("{}..{main}", carrying_r[0].1)]), "2", "the landed candidate sits directly on the main it was checked against");
    pushed_trees_were_checked(&fixture);
}

#[test]
fn remote_enqueue_off_lander_host() {
    let mut fixture = Fixture::new();
    fixture.landing_config("\tlander-host = lander.invalid\n");
    let a = fixture.lane("a", "a.txt", "a");
    let head = git(&a, &["rev-parse", "HEAD"]);
    let top = git(&a, &["rev-parse", "--show-toplevel"]);
    let output = fixture.land_enqueue(&a, &fixture.state, "session-a");
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).lines().any(|line| line == format!("Done: queued (outbox) {top} lane-a {head}")));
    assert_eq!(json_names(&fixture.queue().join(queue::OUTBOX)).len(), 1);
    assert!(queue::pending(&fixture.queue()).unwrap().is_empty(), "an off-lander-host ticket went to the lander queue");

    let host = fs::read_to_string("/proc/sys/kernel/hostname").unwrap().trim().to_owned();
    fixture.landing_config(&format!("\tlander-host = {host}\n"));
    let output = fixture.land_enqueue(&a, &fixture.state, "session-a");
    assert!(String::from_utf8_lossy(&output.stdout).lines().any(|line| line == format!("Done: queued {top} lane-a {head}")));
    assert_eq!(queue::pending(&fixture.queue()).unwrap().len(), 1, "the lander host queues locally");
}

fn pushed_trees_were_checked(fixture: &Fixture) {
    let checked: Vec<String> = fixture.checks().iter().map(|(head, _)| tree(fixture, head)).collect();
    let pushed: Vec<String> = fixture.read("pushes").lines().filter_map(|line| line.split_whitespace().find_map(|word| word.strip_suffix(":refs/heads/main")).map(str::to_owned)).collect();
    assert!(pushed.last() == Some(&fixture.main()), "main is the last pushed commit: {pushed:?}");
    for commit in pushed {
        let pushed = tree(fixture, &commit);
        assert!(checked.contains(&pushed), "pushed {commit} carries tree {pushed}, which no check saw: {checked:?}");
    }
}

const AWAIT: &str = "await() { for ((i = 0; i < 400; i++)); do [ -e \"$LAND_TRACE/$1\" ] && return 0; sleep 0.05; done; return 1; }\n";
const LANDED: &str = "main() { command git -C \"$LAND_ORIGIN\" rev-parse main; }\nlanded() { for ((i = 0; i < 400; i++)); do [ \"$(main)\" != \"$1\" ] && return 0; sleep 0.05; done; return 1; }\n";

fn speculate_serve(fixture: &mut Fixture, later: &[PathBuf]) -> (Output, std::time::Duration) {
    let tickets: Vec<(PathBuf, Ticket)> = later.iter().map(|lane| fixture.ticket(lane, None)).collect();
    let started = std::time::Instant::now();
    let child = fixture.command().stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
    while fixture.checks().is_empty() {
        assert!(started.elapsed() < std::time::Duration::from_secs(30), "the first batch's check never started");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    for (path, ticket) in &tickets {
        queue::write(path, ticket).unwrap();
    }
    let output = child.wait_with_output().unwrap();
    let elapsed = started.elapsed();
    eprintln!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    (output, elapsed)
}

#[test]
fn speculate_two_batches() {
    let mut fixture = Fixture::new();
    let first: Vec<PathBuf> = ["a", "b"].iter().map(|name| fixture.lane(name, &format!("{name}.txt"), name)).collect();
    let second: Vec<PathBuf> = ["c", "d"].iter().map(|name| fixture.lane(name, &format!("{name}.txt"), name)).collect();
    for lane in &first {
        fixture.enqueue(lane, None);
    }
    executable(&fixture.trace.join("hook"), &format!("#!/usr/bin/env bash\n{AWAIT}{LANDED}case $1 in\n1) await check-2 && touch \"$LAND_TRACE/overlapped\" ;;\n2) start=$(main); touch \"$LAND_TRACE/check-2\"; landed \"$start\" ;;\nesac\n"));
    let (output, elapsed) = speculate_serve(&mut fixture, &second);
    assert!(output.status.success());
    assert!(fixture.trace.join("overlapped").exists(), "the second batch's check starts while the first batch's check runs");
    assert!(elapsed < std::time::Duration::from_secs(12), "two overlapping batches took {elapsed:?}");
    assert_eq!(fixture.subjects(), ["change from a", "change from b", "change from c", "change from d"]);
    let checks = fixture.checks();
    assert_eq!(checks.len(), 2, "each batch is checked once");
    assert_eq!(checks[0].1, fixture.base);
    assert_eq!(checks[1].1, checks[0].0, "the second batch is checked on the first batch's candidate");
    assert_eq!(fixture.main(), checks[1].0);
    assert_eq!(fixture.read("pushes").lines().count(), 2);
    pushed_trees_were_checked(&fixture);
}

#[test]
fn speculate_discard_on_red() {
    let mut fixture = Fixture::new();
    let bad = fixture.lane("a", "fail.txt", "a");
    let good = fixture.lane("b", "b.txt", "b");
    let culprit = fixture.enqueue(&bad, None);
    executable(
        &fixture.trace.join("hook"),
        &format!("#!/usr/bin/env bash\n{AWAIT}case $1 in 1) await check-2 ;; 2) touch \"$LAND_TRACE/check-2\"; sleep 30 ;; esac\n! command git cat-file -e HEAD:fail.txt 2>/dev/null || echo refused >\"$LAND_TRACE/verdict-$1\"\n"),
    );
    let (output, elapsed) = speculate_serve(&mut fixture, std::slice::from_ref(&good));
    assert!(output.status.success());
    assert!(elapsed < std::time::Duration::from_secs(20), "the speculative check of a red candidate is stopped, not waited for: {elapsed:?}");
    let checks = fixture.checks();
    assert_eq!(checks.len(), 3, "the red candidate, its discarded successor, then the successor rebuilt: {checks:?}");
    assert_eq!(checks[1].1, checks[0].0, "the successor was speculated on the red candidate");
    assert_eq!(checks[2].1, fixture.base, "the successor is rebuilt on main without the culprit");
    assert!(fixture.verdict(&culprit).unwrap().starts_with("failed "));
    assert_eq!(fixture.subjects(), ["change from b"]);
    assert_eq!(fixture.main(), checks[2].0);
    assert!(git(&fixture.origin, &["ls-tree", "--name-only", "main"]).lines().all(|file| file != "fail.txt"));
    pushed_trees_were_checked(&fixture);
}

#[test]
fn speculate_real_gates() {
    let mut fixture = Fixture::new();
    let a = fixture.lane("a", "a.txt", "a");
    let b = fixture.lane("b", "b.txt", "b");
    fixture.enqueue(&a, None);
    executable(
        &fixture.trace.join("hook"),
        &format!("#!/usr/bin/env bash\n{AWAIT}{LANDED}case $1 in\n1) await main-at-start ;;\n2) start=$(main); printf '%s\\n' \"$start\" >\"$LAND_TRACE/main-at-start.tmp\"; mv \"$LAND_TRACE/main-at-start.tmp\" \"$LAND_TRACE/main-at-start\"; landed \"$start\"; main >\"$LAND_TRACE/main-at-end\" ;;\nesac\n"),
    );
    let (output, _) = speculate_serve(&mut fixture, std::slice::from_ref(&b));
    assert!(output.status.success());
    let checks = fixture.checks();
    assert_eq!(checks.len(), 2, "the speculative candidate lands without re-running");
    let first = &checks[0].0;
    assert_eq!(fixture.read("main-at-start").trim(), fixture.base, "the speculative check starts before the first candidate lands");
    assert_eq!(fixture.read("main-at-end").trim(), first, "the first candidate lands while the speculative check is in flight");
    assert!(fixture.read("checks").lines().nth(1).unwrap().ends_with(&format!("--check-candidate --base {first}")), "{}", fixture.read("checks"));
    assert_eq!(fixture.main(), checks[1].0);
    assert_eq!(fixture.subjects(), ["change from a", "change from b"]);
    let pushes = fixture.read("pushes");
    assert_eq!(pushes.lines().count(), 2);
    assert!(pushes.lines().nth(1).unwrap().contains(&format!("--force-with-lease=refs/heads/main:{first}")), "{pushes}");
    pushed_trees_were_checked(&fixture);
}

fn wait_for(path: &Path) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while !path.exists() {
        assert!(std::time::Instant::now() < deadline, "{} never appeared", path.display());
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

fn stacked_serve(fixture: &mut Fixture, depth: &str, arrivals: &[(usize, &[PathBuf])]) -> Output {
    let batches: Vec<(usize, Vec<(PathBuf, Ticket)>)> = arrivals.iter().map(|(after, lanes)| (*after, lanes.iter().map(|lane| fixture.ticket(lane, None)).collect())).collect();
    let child = fixture.command().args(["--depth", depth]).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
    for (after, tickets) in &batches {
        wait_for(&fixture.trace.join(format!("started-{after}")));
        for (path, ticket) in tickets {
            queue::write(path, ticket).unwrap();
        }
        fs::write(fixture.trace.join(format!("arrived-{after}")), "").unwrap();
    }
    let output = child.wait_with_output().unwrap();
    eprintln!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    output
}

fn stacked(output: &Output) -> usize {
    String::from_utf8_lossy(&output.stdout).lines().filter(|line| line.starts_with("north land: speculative candidate ") && line.contains(" carries ")).count()
}

fn stamp(fixture: &Fixture, name: &str) -> f64 {
    fixture.read(name).trim().parse().unwrap_or_else(|_| panic!("{name} holds no time: {:?}", fixture.read(name)))
}

const STACKED_HOOK: &str = "#!/usr/bin/env bash\nawait() { for ((i = 0; i < 1200; i++)); do [ -e \"$LAND_TRACE/$1\" ] && return 0; sleep 0.05; done; return 1; }\ndate +%s.%N >\"$LAND_TRACE/start-$1\"\ntouch \"$LAND_TRACE/started-$1\"\n";

#[test]
fn speculate_depth_three() {
    let mut fixture = Fixture::new();
    let lanes: Vec<PathBuf> = ["a", "b", "c", "d"].iter().map(|name| fixture.lane(name, &format!("{name}.txt"), name)).collect();
    fixture.enqueue(&lanes[0], None);
    executable(&fixture.trace.join("hook"), &format!("{STACKED_HOOK}[ \"$1\" -gt 3 ] || await started-3\nsleep 5\ndate +%s.%N >\"$LAND_TRACE/end-$1\"\n"));
    let output = stacked_serve(&mut fixture, "3", &[(1, &lanes[1..2]), (2, &lanes[2..])]);
    assert!(output.status.success());
    assert_eq!(fixture.subjects(), ["change from a", "change from b", "change from c", "change from d"]);
    let checks = fixture.checks();
    assert_eq!(checks.len(), 3, "each candidate is checked once: {checks:?}");
    assert_eq!(checks[0].1, fixture.base);
    assert_eq!(checks[1].1, checks[0].0, "the second candidate is stacked on the first");
    assert_eq!(checks[2].1, checks[1].0, "the third candidate is stacked on the second");
    assert_eq!(stacked(&output), 2, "two candidates were stacked beside the first");
    let (starts, ends): (Vec<f64>, Vec<f64>) = (1..=3).map(|n| (stamp(&fixture, &format!("start-{n}")), stamp(&fixture, &format!("end-{n}")))).unzip();
    let length = starts.iter().zip(&ends).map(|(start, end)| end - start).fold(0.0, f64::max);
    let span = ends.iter().copied().fold(0.0, f64::max) - starts[0];
    assert!(starts.iter().copied().fold(0.0, f64::max) < ends.iter().copied().fold(f64::MAX, f64::min), "all three checks were in flight at once: {starts:?} {ends:?}");
    assert!(span <= 1.2 * length, "three stacked checks took {span:.2} s from the first start, over 1.2 check lengths of {length:.2} s");
    assert_eq!(fixture.main(), checks[2].0);
    assert_eq!(fixture.read("pushes").lines().count(), 3);
    pushed_trees_were_checked(&fixture);
}

#[test]
fn speculate_depth_three_red_middle() {
    let mut fixture = Fixture::new();
    let lanes: Vec<PathBuf> = [("a", "a.txt"), ("b", "fail.txt"), ("c", "c.txt")].iter().map(|(name, file)| fixture.lane(name, file, name)).collect();
    fixture.enqueue(&lanes[0], None);
    executable(
        &fixture.trace.join("hook"),
        &format!("{STACKED_HOOK}case $1 in\n1) ! await stopped || touch \"$LAND_TRACE/stopped-in-flight\" ;;\n2) await started-3 ;;\n3) echo $$ >\"$LAND_TRACE/stacked-pid\"; sleep 300 ;;\nesac\n! command git cat-file -e HEAD:fail.txt 2>/dev/null || echo refused >\"$LAND_TRACE/verdict-$1\"\n"),
    );
    let watcher = {
        let trace = fixture.trace.clone();
        std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
            while std::time::Instant::now() < deadline {
                if let Ok(pid) = fs::read_to_string(trace.join("stacked-pid"))
                    && !Path::new(&format!("/proc/{}", pid.trim())).exists()
                {
                    fs::write(trace.join("stopped"), "").unwrap();
                    return true;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            false
        })
    };
    let output = stacked_serve(&mut fixture, "3", &[(1, &lanes[1..2]), (2, &lanes[2..])]);
    assert!(output.status.success());
    assert!(watcher.join().unwrap(), "the check stacked on the red candidate was stopped");
    assert!(fixture.trace.join("stopped-in-flight").exists(), "the stacked check was stopped while the first candidate was still checked, not when its turn to land came");
    let checks = fixture.checks();
    assert!(checks.len() >= 4, "{checks:?}");
    assert_eq!(checks[1].1, checks[0].0);
    assert_eq!(checks[2].1, checks[1].0, "c was stacked on the red b");
    assert!(checks[3..].iter().all(|(_, base)| *base == checks[0].0), "after the red middle, its lanes are rebuilt on the new main: {checks:?}");
    let culprit = json_names(&fixture.queue().join("done")).into_iter().find(|name| name.ends_with("-lane-b.json")).unwrap();
    assert!(fixture.verdict(&fixture.queue().join(culprit)).unwrap().starts_with("failed "));
    assert_eq!(fixture.subjects(), ["change from a", "change from c"]);
    assert!(git(&fixture.origin, &["ls-tree", "--name-only", "main"]).lines().all(|file| file != "fail.txt"));
    pushed_trees_were_checked(&fixture);
}

#[test]
fn speculate_depth_one_waits() {
    let mut fixture = Fixture::new();
    let lanes: Vec<PathBuf> = ["a", "b"].iter().map(|name| fixture.lane(name, &format!("{name}.txt"), name)).collect();
    fixture.enqueue(&lanes[0], None);
    executable(&fixture.trace.join("hook"), &format!("{STACKED_HOOK}[ \"$1\" != 1 ] || {{ await arrived-1; sleep 1; }}\n"));
    let output = stacked_serve(&mut fixture, "1", &[(1, &lanes[1..])]);
    assert!(output.status.success());
    assert_eq!(stacked(&output), 0, "depth 1 never speculates");
    assert!(!fixture.root.join("proj/worktrees/lander-2").exists());
    let checks = fixture.checks();
    assert_eq!(checks.len(), 2);
    assert_eq!(checks[1].1, checks[0].0, "b is checked on main after a landed");
    assert_eq!(fixture.subjects(), ["change from a", "change from b"]);
    pushed_trees_were_checked(&fixture);
}

#[test]
fn speculate_depth_two_stacks_once() {
    let mut fixture = Fixture::new();
    let lanes: Vec<PathBuf> = ["a", "b", "c"].iter().map(|name| fixture.lane(name, &format!("{name}.txt"), name)).collect();
    fixture.enqueue(&lanes[0], None);
    executable(&fixture.trace.join("hook"), &format!("{STACKED_HOOK}[ \"$1\" -gt 2 ] || {{ await arrived-2; sleep 1; }}\n"));
    let output = stacked_serve(&mut fixture, "2", &[(1, &lanes[1..2]), (2, &lanes[2..])]);
    assert!(output.status.success());
    assert!(!fixture.root.join("proj/worktrees/lander-3").exists(), "depth 2 keeps one spare worktree");
    assert_eq!(stacked(&output), 1, "c waits for the next round instead of a second stacked candidate");
    let checks = fixture.checks();
    assert_eq!(checks.len(), 3, "{checks:?}");
    assert_eq!(checks[1].1, checks[0].0);
    assert_eq!(checks[2].1, checks[1].0);
    assert_eq!(fixture.subjects(), ["change from a", "change from b", "change from c"]);
    pushed_trees_were_checked(&fixture);
}

type Row = (String, String, i64, i64, i64, String, Option<String>, Option<String>, String, Option<String>, Option<String>, String);

fn ledger_rows(fixture: &Fixture, expected: usize) -> Vec<Row> {
    let ledger = fixture.root.join("threads.db");
    for _ in 0..200 {
        if let Ok(db) = rusqlite::Connection::open(&ledger)
            && let Ok(rows) = db
                .prepare("SELECT lane, verdict, batch_size, bisect_depth, attempts, candidate, landed_sha, first_commit_at, queued_at, check_started_at, check_finished_at, coalesce(landed_at, '') FROM landings ORDER BY lane")
                .and_then(|mut statement| statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?, row.get(9)?, row.get(10)?, row.get(11)?)))?.collect::<Result<Vec<Row>, _>>())
            && rows.len() >= expected
        {
            return rows;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    panic!("the ledger never held {expected} landings rows");
}

fn iso(seconds: f64) -> String {
    north_core::foundation::clock::Timestamp::from_unix_seconds_f64(seconds).iso()
}

#[test]
fn serve_records_the_landed_lane_with_its_first_commit_queue_check_and_landing_times() {
    let mut fixture = Fixture::new();
    let lane = fixture.lane("a", "a.txt", "a");
    let ticket = fixture.enqueue(&lane, None);
    let first_commit = (now_micros() / 1_000_000) as i64 - 1_800;
    let mut queued = queue::read(&ticket).unwrap();
    queued.first_commit_secs = Some(first_commit);
    queue::write(&ticket, &queued).unwrap();
    assert!(fixture.serve().status.success());
    let main = fixture.main();
    let rows = ledger_rows(&fixture, 1);
    assert_eq!(rows.len(), 1);
    let (lane, verdict, batch, depth, attempts, candidate, landed_sha, first, queued, started, finished, landed) = rows[0].clone();
    assert_eq!((lane.as_str(), verdict.as_str(), batch, depth, attempts), ("a", "landed", 1, 0, 1));
    assert_eq!((candidate, landed_sha.as_deref()), (main.clone(), Some(main.as_str())));
    assert_eq!(first.as_deref(), Some(iso(first_commit as f64).as_str()));
    let (queued, started, finished) = (queued, started.unwrap(), finished.unwrap());
    assert!(first.unwrap() < queued && queued <= started && started <= finished && finished <= landed, "first commit, queued, check start, check end and landed are in order: {queued} {started} {finished} {landed}");
}

#[test]
fn serve_records_a_conflict_and_a_failed_check_as_refusals_beside_the_landings() {
    let mut fixture = Fixture::new();
    let lanes: Vec<PathBuf> = [("a", "a.txt"), ("b", "shared.txt"), ("c", "shared.txt"), ("d", "fail.txt")].iter().map(|(name, file)| fixture.lane(name, file, name)).collect();
    for lane in &lanes {
        fixture.enqueue(lane, None);
    }
    executable(&fixture.trace.join("hook"), "#!/usr/bin/env bash\n! command git cat-file -e HEAD:fail.txt 2>/dev/null || echo 'refused https://github.com/o/r/actions/runs/4242' >\"$LAND_TRACE/verdict-$1\"\n");
    assert!(fixture.serve().status.success());
    let rows = ledger_rows(&fixture, 4);
    let verdicts: Vec<(&str, &str)> = rows.iter().map(|row| (row.0.as_str(), row.1.as_str())).collect();
    assert_eq!(verdicts, [("a", "landed"), ("b", "landed"), ("c", "conflict"), ("d", "failed")]);
    let (a, d) = (&rows[0], &rows[3]);
    assert!(a.3 >= 1, "a and b landed after the refused candidate was halved, so their bisect depth is above zero: {a:?}");
    assert!(d.6.is_none() && d.9.is_some() && d.10.is_some(), "the failed lane keeps its check window and has no landed sha: {d:?}");
    let incidents = incident_rows(&fixture, 2);
    assert_eq!(incidents, [("lander-conflict".to_owned(), "lander".to_owned(), "lane-c".to_owned()), ("lander-refused".to_owned(), "lander".to_owned(), "lane-d".to_owned())]);
}

fn incident_rows(fixture: &Fixture, expected: usize) -> Vec<(String, String, String)> {
    let ledger = fixture.root.join("threads.db");
    for _ in 0..200 {
        if let Ok(db) = rusqlite::Connection::open(&ledger)
            && let Ok(rows) = db
                .prepare("SELECT class, noticed_by, subject FROM incidents WHERE started_at IS NOT NULL AND started_at <= detected_at ORDER BY class")
                .and_then(|mut statement| statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?.collect::<Result<Vec<_>, _>>())
            && rows.len() >= expected
        {
            return rows;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    panic!("the ledger never held {expected} incidents rows");
}

impl Fixture {
    fn opt_into_app(&mut self) {
        fs::write(self.repo.join(".safe-push"), "[landing]\n\tpush-app = lander\n").unwrap();
        git(&self.repo, &["add", "."]);
        git(&self.repo, &["commit", "-qm", "opt into the lander App"]);
        git(&self.repo, &["push", "-q", "origin", "main"]);
        self.base = git(&self.repo, &["rev-parse", "HEAD"]);
    }
}

#[test]
fn serve_push_actor_app() {
    let mut fixture = Fixture::new();
    fixture.opt_into_app();
    let a = fixture.lane("a", "a.txt", "a");
    let ticket = fixture.enqueue(&a, None);
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.verdict(&ticket), Some(format!("landed {}", fixture.main())));
    let pushes = fixture.read("pushes");
    assert_eq!(pushes.lines().count(), 1);
    assert!(pushes.contains("token=none app=app-token"), "the push carries the App token and no ambient token: {pushes}");
    assert!(!pushes.contains("push-token"), "the ambient token reached the push: {pushes}");
    assert!(pushes.contains("credential.helper="), "the push must reset the inherited credential helpers: {pushes}");
    assert!(!pushes.contains("app-token "), "the token must not be on the command line: {pushes}");
    assert_eq!(fixture.read("mints"), "x");
    let env = fixture.read("env-1");
    assert!(env.contains("LAND_TRACE="));
    for name in ["GH_TOKEN=", "GITHUB_TOKEN=", "SOPS_AGE_KEY_FILE=", "NORTH_LAND_PUSH_TOKEN="] {
        assert!(!env.lines().any(|line| line.starts_with(name)), "the check saw {name}");
    }
}

#[test]
fn serve_app_mint_failure_keeps_the_ticket() {
    let mut fixture = Fixture::new();
    fixture.opt_into_app();
    let a = fixture.lane("a", "a.txt", "a");
    let ticket = fixture.enqueue(&a, None);
    fs::write(fixture.trace.join("mint-fail"), "").unwrap();
    let output = fixture.serve();
    assert_eq!(output.status.code(), Some(75));
    assert!(String::from_utf8_lossy(&output.stdout).contains("transient: no push credential"), "{}", String::from_utf8_lossy(&output.stdout));
    assert!(ticket.exists(), "a mint failure decided the lane");
    assert_eq!(fixture.verdict(&ticket), None);
    assert_eq!(fixture.main(), fixture.base);
    assert_eq!(fixture.read("pushes"), "", "a mint failure fell back to another credential");
    fs::remove_file(fixture.trace.join("mint-fail")).unwrap();
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.verdict(&ticket), Some(format!("landed {}", fixture.main())));
    assert!(fixture.read("pushes").contains("app=app-token"));
}

#[test]
fn serve_unconfigured_repository_keeps_its_credential() {
    let mut fixture = Fixture::new();
    let a = fixture.lane("a", "a.txt", "a");
    fixture.enqueue(&a, None);
    assert!(fixture.serve().status.success());
    assert!(fixture.read("pushes").contains("token=push-token app=none"));
    assert_eq!(fixture.read("mints"), "");
}

const REAL_CHECKER: &str = r#"#!/usr/bin/env bash
set -u
log=$(mktemp) outcome=$(mktemp) status=0
NORTH_LAND_NORTH="$LAND_REAL_NORTH" "$LAND_REAL_NORTH" land checks --base "$3" --head HEAD --outcome "$outcome" >"$log" 2>&1 || status=$?
cat "$log"
if [ "$status" -eq 0 ]; then printf 'safe-push: verdict green %s\n' "$(git rev-parse HEAD)"
elif [ "$status" -eq 75 ]; then printf 'safe-push: verdict transient %s\n' "$(awk -F'\t' '$1 == "skip" { line = $2 } END { print line }' "$outcome")"
else printf 'safe-push: verdict refused %s\n' "$(grep -m1 'safe-push: land check ' "$log" | grep -v ': land check: ')"
fi
"#;

#[test]
fn serve_hung_check_judged_against_base() {
    let mut fixture = Fixture::new();
    let step = fixture.trace.join("step.sh");
    git(&fixture.repo, &["config", "--file", ".safe-push", "land.check", &format!("* :: bash {}", step.display())]);
    git(&fixture.repo, &["add", ".safe-push"]);
    git(&fixture.repo, &["commit", "-qm", "check every change"]);
    git(&fixture.repo, &["push", "-q", "origin", "main"]);
    executable(&fixture.trace.join("checker"), REAL_CHECKER);
    let serve = |fixture: &Fixture| {
        let output = fixture.command().env("SAFE_PUSH_LAND_CHECK_TIMEOUT", "2").output().unwrap();
        eprintln!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        output
    };

    let a = fixture.lane("a", "a.txt", "a");
    let first = fixture.enqueue(&a, None);
    fs::write(&step, "exit 0\n").unwrap();
    assert!(serve(&fixture).status.success());
    assert!(fixture.verdict(&first).is_some_and(|verdict| verdict.starts_with("landed ")), "{:?}", fixture.verdict(&first));

    let b = fixture.lane("b", "b.txt", "b");
    let second = fixture.enqueue(&b, None);
    fs::write(&step, "sleep 600\n").unwrap();
    let main = fixture.main();
    assert!(serve(&fixture).status.success());
    let verdict = fixture.verdict(&second).unwrap_or_default();
    assert!(verdict.starts_with("failed ") && verdict.contains(&format!("land check bash {} hung: it ran past its 2 s deadline on this lane, while base tree ", step.display())), "{verdict}");
    assert_eq!(fixture.main(), main, "a hung candidate is not pushed");

    fs::remove_dir_all(fixture.state.join("safe-push/check-timings")).unwrap();
    let c = fixture.lane("c", "c.txt", "c");
    let third = fixture.enqueue(&c, None);
    assert_eq!(serve(&fixture).status.code(), Some(75), "a hang on a base tree never timed is infrastructure");
    assert!(third.exists(), "a hang the base tree was never timed for decided the lane");
    assert_eq!(fixture.main(), main);
}

const ALONE_WAIT: &str = r#"#!/usr/bin/env bash
! command git cat-file -e HEAD:fail.txt 2>/dev/null || echo refused >"$LAND_TRACE/verdict-$1"
[ "$1" -ge 2 ] && [ "$1" -le 5 ] || exit 0
date +%s%N >"$LAND_TRACE/start-$1"
for k in $(seq 400); do [ "$(ls "$LAND_TRACE"/start-* | wc -l)" = 4 ] && break; sleep 0.05; done
date +%s%N >"$LAND_TRACE/end-$1"
echo "safe-push: ✗ the pre-push checks failed in check $1; NOT pushed."
"#;

fn stamps(fixture: &Fixture, prefix: &str) -> Vec<u128> {
    (2..=5).map(|n| fixture.read(&format!("{prefix}-{n}")).trim().parse().unwrap()).collect()
}

fn culprit_lanes(fixture: &mut Fixture) -> Vec<PathBuf> {
    ["a", "b", "c", "d"]
        .iter()
        .map(|name| {
            let file = if *name == "c" { "fail.txt".to_owned() } else { format!("{name}.txt") };
            let lane = fixture.lane(name, &file, name);
            fixture.enqueue(&lane, None)
        })
        .collect()
}

#[test]
fn culprit_search_names_the_culprit_in_one_round() {
    let mut fixture = Fixture::new();
    let tickets = culprit_lanes(&mut fixture);
    executable(&fixture.trace.join("hook"), ALONE_WAIT);
    let output = fixture.serve();
    assert!(output.status.success());
    let checks = fixture.checks();
    assert_eq!(checks.len(), 6, "the refused candidate, four lanes alone, then the three green lanes together");
    assert!(checks[1..5].iter().all(|(_, base)| *base == fixture.base), "each lane is checked alone on the unmoved base");
    let (starts, ends) = (stamps(&fixture, "start"), stamps(&fixture, "end"));
    assert!(starts.iter().max() < ends.iter().min(), "the four single-lane checks overlapped: starts {starts:?} ends {ends:?}");
    let culprit = (2..=5).find(|n| Command::new("git").arg("-C").arg(&fixture.repo).args(["cat-file", "-e", &format!("{}:fail.txt", checks[n - 1].0)]).output().unwrap().status.success()).unwrap();
    let failure = fixture.verdict(&tickets[2]).unwrap();
    assert!(failure.starts_with("failed ") && failure.ends_with("-candidate.log"), "{failure}");
    let log = failure.trim_start_matches("failed ");
    assert!(fs::read_to_string(log).unwrap().contains(&format!("failed in check {culprit}")), "the culprit's own log: {log}");
    let fleet = fixture.fleet();
    let told: Vec<&String> = fleet.iter().filter(|call| call.starts_with("fleet send --from service:north-land session-lane-c ")).collect();
    assert_eq!(told.len(), 1, "{fleet:?}");
    assert!(told[0].contains(&format!("safe-push: ✗ the pre-push checks failed in check {culprit}; NOT pushed.")) && told[0].contains(&format!("full log: {log}")), "{told:?}");
    let main = fixture.main();
    for ticket in [&tickets[0], &tickets[1], &tickets[3]] {
        assert_eq!(fixture.verdict(ticket), Some(format!("landed {main}")));
    }
    assert_eq!(fixture.subjects(), ["change from a", "change from b", "change from d"]);
    assert_eq!(checks[5].0, main, "the three green lanes landed as one candidate");
    assert_eq!(fixture.read("pushes").lines().count(), 1);
    assert_eq!(git(&fixture.origin, &["rev-list", "--count", &format!("{}..main", fixture.base)]), "3");
    assert_eq!(fleet.len(), 4, "each lane is told once: {fleet:?}");
}

#[test]
fn culprit_search_checks_in_waves_of_the_free_slots() {
    let mut fixture = Fixture::new();
    let tickets = culprit_lanes(&mut fixture);
    let offload = fixture.root.join("offload");
    fs::create_dir_all(&offload).unwrap();
    for lock in ["proj-main.lock", "proj-main-1.lock"] {
        fs::write(offload.join(lock), "").unwrap();
    }
    let mut holder = Command::new("flock").args(["-x", offload.join("proj-main.lock").to_str().unwrap(), "sleep", "120"]).spawn().unwrap();
    while Command::new("flock").args(["-n", offload.join("proj-main.lock").to_str().unwrap(), "true"]).status().unwrap().success() {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    executable(&fixture.trace.join("hook"), "#!/usr/bin/env bash\n! command git cat-file -e HEAD:fail.txt 2>/dev/null || echo refused >\"$LAND_TRACE/verdict-$1\"\n");
    let output = fixture.command().env("HOME", &fixture.root).env("NORTH_OFFLOAD_ROOT", &offload).output().unwrap();
    holder.kill().unwrap();
    holder.wait().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    eprintln!("{stdout}{}", String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success());
    assert!(stdout.contains("1 of 2 offload slots are free, so they are checked alone in waves of 1"), "{stdout}");
    assert!(fixture.verdict(&tickets[2]).unwrap().starts_with("failed "));
    assert_eq!(fixture.subjects(), ["change from a", "change from b", "change from d"]);
}

#[test]
fn culprit_search_falls_back_to_halving_when_every_lane_passes_alone() {
    let mut fixture = Fixture::new();
    let tickets: Vec<PathBuf> = ["a", "b"].iter().map(|name| {
        let lane = fixture.lane(name, &format!("{name}.txt"), name);
        fixture.enqueue(&lane, None)
    }).collect();
    fs::write(fixture.trace.join("verdict-1"), "refused https://github.com/o/r/actions/runs/1").unwrap();
    let output = fixture.serve();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.matches("every lane passed alone, so the refusal is an interaction; falling back to halving the candidate").count(), 1, "{stdout}");
    assert_eq!(fixture.checks().len(), 5, "the pair, each lane alone, then the first half and the second half");
    assert!(fixture.verdict(&tickets[0]).is_some_and(|verdict| verdict.starts_with("landed ")));
    assert_eq!(fixture.verdict(&tickets[1]), Some(format!("landed {}", fixture.main())));
    assert_eq!(fixture.subjects(), ["change from a", "change from b"]);
}

const RUNNER_CHECKER: &str = r#"#!/usr/bin/env bash
set -u
printf '%s %s %s\n' "$(git rev-parse HEAD)" "$3" "$*" >>"$LAND_TRACE/checks"
[ ! -x "$LAND_TRACE/hook" ] || "$LAND_TRACE/hook"
unset AGENT_CAPACITY_LEASE SAFE_PUSH_LAND_CAPACITY SAFE_PUSH_LAND_OFFLOAD_HOST SAFE_PUSH_LAND_RUNNER SAFE_PUSH_LAND_CHECK_TIMEOUT
export XDG_RUNTIME_DIR="$LAND_TRACE/runtime"
outcome=$(mktemp)
status=0
NORTH_LAND_NORTH="$LAND_TRACE/north-pass" "$LAND_REAL_NORTH" land checks --base "$3" --head "$(git rev-parse HEAD)" --dry 1 --outcome "$outcome" || status=$?
skip=$(awk -F'\t' '$1 == "skip" { found = $2 } END { print found }' "$outcome")
case $status in
  0) echo 'safe-push: verdict green' ;;
  75) echo "safe-push: verdict transient ${skip:-exit 75}" ;;
  *) echo 'safe-push: verdict refused' ;;
esac
[ "$status" -eq 0 ]
"#;

const PASSTHROUGH_NORTH: &str = "#!/usr/bin/env bash\n[ \"$1\" != land-cache ] || exit 1\nexec \"$LAND_REAL_NORTH\" \"$@\"\n";

const PLANTED_CHECK: &str = r#"printf '%s\n' "$(git rev-parse HEAD)" >>"$LAND_TRACE/check-runs"; [ ! -e "$LAND_TRACE/plant" ] || bash "$LAND_TRACE/plant"; if grep -qs broken ./*.txt; then echo "error: a lane wrote broken" >&2; exit 1; fi"#;

impl Fixture {
    fn with_runner_check(&mut self, check: &str) {
        executable(&self.trace.join("checker"), RUNNER_CHECKER);
        executable(&self.trace.join("north-pass"), PASSTHROUGH_NORTH);
        executable(&self.bin.join("supply-alert"), "#!/usr/bin/env bash\nprintf '%s|' \"$@\" >>\"$LAND_TRACE/alerts\"\necho >>\"$LAND_TRACE/alerts\"\n");
        fs::create_dir_all(self.trace.join("runtime")).unwrap();
        git(&self.repo, &["config", "--file", ".safe-push", "--add", "land.check", &format!("*.txt,Cargo.lock :: {check}")]);
        git(&self.repo, &["add", ".safe-push"]);
        git(&self.repo, &["commit", "-qm", "land check"]);
        git(&self.repo, &["push", "-q", "origin", "main"]);
        self.base = git(&self.repo, &["rev-parse", "HEAD"]);
    }

    fn alerts(&self) -> Vec<String> {
        self.read("alerts").lines().map(str::to_owned).collect()
    }

    fn pause(&self) -> PathBuf {
        self.queue().join(north_landing::land::pause::FILE)
    }

    fn retry_now(&self) {
        let mut pause: serde_json::Value = serde_json::from_str(&fs::read_to_string(self.pause()).unwrap()).unwrap();
        pause["retry_at"] = serde_json::json!(0);
        fs::write(self.pause(), pause.to_string()).unwrap();
    }

    fn base_runs(&self) -> usize {
        self.read("check-runs").lines().filter(|line| *line == self.base).count()
    }
}

#[test]
fn serve_pauses_while_main_fails_its_own_check() {
    let mut fixture = Fixture::new();
    fixture.with_runner_check(PLANTED_CHECK);
    let a = fixture.lane("a", "a.txt", "a");
    let b = fixture.lane("b", "b.txt", "b");
    let tickets = [fixture.enqueue(&a, None), fixture.enqueue(&b, None)];
    fs::write(fixture.trace.join("plant"), "echo 'bash: nix: command not found' >&2\nexit 127\n").unwrap();

    assert_eq!(fixture.serve().status.code(), Some(75));
    assert!(tickets.iter().all(|ticket| ticket.exists()), "a broken check environment decided a lane");
    assert_eq!(fixture.main(), fixture.base);
    let alerts = fixture.alerts();
    assert_eq!(alerts.len(), 1, "{alerts:?}");
    assert!(alerts[0].starts_with("normal|main's check environment is broken: ") && alerts[0].contains(": bash: nix: command not found|"), "{alerts:?}");
    assert_eq!(fixture.base_runs(), 1, "the base tree reran the failed check other than once");
    let checked = fixture.checks().len();

    let output = fixture.serve();
    assert_eq!(output.status.code(), Some(75));
    assert!(String::from_utf8_lossy(&output.stdout).contains("new candidates are paused and the base check is retried in "), "{}", String::from_utf8_lossy(&output.stdout));
    assert_eq!(fixture.checks().len(), checked, "a paused lander checked a new candidate");

    fixture.retry_now();
    assert_eq!(fixture.serve().status.code(), Some(75));
    assert_eq!(fixture.checks().len(), checked + 1, "the lander did not retry once the pause ran out");
    assert_eq!(fixture.base_runs(), 2, "the retry did not rerun the base check");
    assert_eq!(fixture.alerts().len(), 1, "a still-broken environment alerted again");
    assert!(tickets.iter().all(|ticket| ticket.exists()));

    fs::remove_file(fixture.trace.join("plant")).unwrap();
    fixture.retry_now();
    assert!(fixture.serve().status.success());
    assert!(!fixture.pause().exists(), "the lander stayed paused after main passed its check");
    assert_eq!(fixture.subjects(), ["change from a", "change from b"]);
    assert!(tickets.iter().all(|ticket| fixture.verdict(ticket).is_some_and(|verdict| verdict.starts_with("landed "))));
    assert_eq!(fixture.alerts().len(), 1);
}

#[test]
fn serve_refuses_a_lane_that_alone_fails_a_check_main_passes() {
    let mut fixture = Fixture::new();
    fixture.with_runner_check(PLANTED_CHECK);
    let a = fixture.lane("a", "a.txt", "broken");
    let ticket = fixture.enqueue(&a, None);
    assert!(fixture.serve().status.success());
    assert!(fixture.verdict(&ticket).is_some_and(|verdict| verdict.starts_with("failed ")), "{:?}", fixture.verdict(&ticket));
    assert_eq!(fixture.main(), fixture.base);
    assert_eq!(fixture.base_runs(), 1, "the refusal was not judged against the base tree");
    assert!(fixture.alerts().is_empty());
    assert!(!fixture.pause().exists());
}

const RUNTIME_PATH_CHECK: &str = r#"printf '%s\n' "$(git rev-parse HEAD)" >>"$LAND_TRACE/check-runs"; if git diff --quiet "$(cat "$LAND_TRACE/runtime-sha")" "$SAFE_PUSH_LAND_HEAD" -- Cargo.lock; then exit 0; fi; echo 'nix: command not found behind the north capacity gate' >&2; exit 127"#;

#[test]
fn serve_judges_a_failure_off_the_runtime_path_against_the_base_on_the_same_path() {
    let mut fixture = Fixture::new();
    fixture.with_runner_check(RUNTIME_PATH_CHECK);
    fs::write(fixture.trace.join("runtime-sha"), &fixture.base).unwrap();
    let a = fixture.lane("a", "Cargo.lock", "lockfile change");
    let ticket = fixture.enqueue(&a, None);
    assert_eq!(fixture.serve().status.code(), Some(75), "a lane forced off the installed runtime was refused for the environment");
    assert!(ticket.exists(), "the off-runtime failure decided the lane");
    assert_eq!(fixture.base_runs(), 1, "the base tree did not rerun the check");
    assert!(fixture.pause().exists());
    assert_eq!(fixture.alerts().len(), 1, "{:?}", fixture.alerts());
}
