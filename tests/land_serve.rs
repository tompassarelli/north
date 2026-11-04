use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use north_landing::land::queue::{self, RedFix, Ticket};

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
n=$(( $(cat "$LAND_TRACE/checks" 2>/dev/null | wc -l) + 1 ))
printf '%s %s %s\n' "$(git rev-parse HEAD)" "$3" "$*" >>"$LAND_TRACE/checks"
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

const NORTH: &str = r#"#!/usr/bin/env bash
printf '%s\n' "$*" >>"$LAND_TRACE/north-calls"
if [ "$1 $2" = "runtime follow" ] && [ -n "${NORTH_RUNTIME_LINK:-}" ]; then ln -sfn gen-2 "$NORTH_RUNTIME_LINK"; fi
if [ "$1" = fleet ] && [ "$2" = send ] && [ "${4%% *}" = landed ]; then
  branch=$(printf '%s' "$4" | awk '{print $2}')
  main=$(git -C "$LAND_ORIGIN" rev-parse main)
  printf '%s %s\n' "$3" "$(git -C "$LAND_REPO" cherry "$main" "$branch" | tr '\n' ' ')" >>"$LAND_TRACE/send-cherry"
  printf '%s %s\n' "$3" "$4" >>"$LAND_TRACE/sent"
fi
"#;

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
        executable(&trace.join("checker"), CHECKER);
        executable(&trace.join("north"), NORTH);
        let real_git = String::from_utf8(Command::new("bash").args(["-c", "command -v git"]).output().unwrap().stdout).unwrap().trim().to_owned();
        executable(
            &bin.join("git"),
            &format!("#!/usr/bin/env bash\npushing=0\nfor a in \"$@\"; do if [ \"$a\" = push ]; then pushing=1; printf '%s token=%s\\n' \"$*\" \"${{GH_TOKEN:-none}}\" >>\"$LAND_TRACE/pushes\"; break; fi; done\nif [ $pushing = 1 ] && [ -e \"$LAND_TRACE/kill-after-push\" ]; then\n  {real_git} \"$@\"; rc=$?\n  rm -f \"$LAND_TRACE/kill-after-push\"\n  kill -9 $PPID\n  exit $rc\nfi\nexec {real_git} \"$@\"\n"),
        );
        Self { _scratch: scratch, root, origin, repo, state, trace, bin, base, queued: 1_000 }
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
            red_fix,
            verdict: None,
        };
        let path = self.queue().join(queue::file_name(self.queued, &branch));
        queue::write(&path, &ticket).unwrap();
        path
    }

    fn serve(&self) -> Output {
        let path = format!("{}:{}", self.bin.display(), std::env::var("PATH").unwrap());
        let output = Command::new(env!("CARGO_BIN_EXE_north"))
            .args(["land", "serve", "--once", "--repo", self.repo.to_str().unwrap(), "--checker", self.trace.join("checker").to_str().unwrap()])
            .env("XDG_STATE_HOME", &self.state)
            .env("PATH", path)
            .env("LAND_TRACE", &self.trace)
            .env("NORTH_LAND_NORTH", self.trace.join("north"))
            .env("LAND_ORIGIN", &self.origin)
            .env("LAND_REPO", &self.repo)
            .env("GH_TOKEN", "push-token")
            .env("GITHUB_TOKEN", "push-token")
            .env("SOPS_AGE_KEY_FILE", self.root.join("age.key"))
            .output()
            .unwrap();
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
        self.read("north-calls").lines().filter(|line| line.starts_with(prefix)).map(str::to_owned).collect()
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
    assert_eq!(checks.len(), 3, "the refused candidate of a, b and d halves to a and b, then d alone");
    assert!(checks.iter().any(|(checked, _)| *checked == main), "main is a commit the checks passed");
    let fleet = fixture.fleet();
    assert!(fleet.iter().any(|line| line.starts_with("fleet send session-lane-d ") && line.contains("did not land") && line.contains("https://github.com/o/r/actions/runs/4242")), "{fleet:?}");
    assert!(fleet.iter().any(|line| line.starts_with("fleet send session-lane-c ") && line.contains("did not land") && line.contains("shared.txt")), "{fleet:?}");
    for (lane, sha) in [("a", &main), ("b", &main), ("e", &e_landed)] {
        assert!(fleet.contains(&format!("fleet send session-lane-{lane} landed lane-{lane} as {sha}")), "{fleet:?}");
    }
    assert_eq!(fleet.len(), 5, "each lane is told once: {fleet:?}");
    let after: Vec<_> = lanes.iter().map(|lane| worktree_state(lane)).collect();
    assert_eq!(after, before, "the lander rebases only its own worktree");
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
    assert_eq!(captured.lines().count(), 2, "the refused pair halves to the Python lane alone");
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
    assert_eq!(fixture.checks().len(), 2);
    assert!(tickets.iter().all(|ticket| ticket.exists()), "an exit-75 check decided a lane");
    assert_eq!(fixture.main(), fixture.base);
    assert!(fixture.fleet().is_empty(), "{:?}", fixture.fleet());

    let racer = fixture.root.join("racer");
    git(&fixture.root, &["clone", "-q", fixture.origin.to_str().unwrap(), racer.to_str().unwrap()]);
    git(&racer, &["config", "user.name", "Racer"]);
    git(&racer, &["config", "user.email", "racer@example.invalid"]);
    fs::write(fixture.trace.join("verdict-3"), "refused https://github.com/o/r/actions/runs/3").unwrap();
    executable(
        &fixture.trace.join("hook"),
        &format!("#!/usr/bin/env bash\n[ \"$1\" = 3 ] || exit 0\ncd {}\necho race >race.txt\ncommand git add race.txt\ncommand git commit -qm 'main moves during the check'\ncommand git push -q origin main\n", racer.display()),
    );
    assert!(fixture.serve().status.success());
    let checks = fixture.checks();
    assert_eq!(checks.len(), 4, "a refusal on a moved base is rebuilt, not halved");
    assert_eq!(checks[3].1, git(&racer, &["rev-parse", "HEAD"]));
    assert_eq!(fixture.subjects(), ["main moves during the check", "change from a", "change from b", "change from c"]);
    let main = fixture.main();
    for ticket in &tickets {
        assert_eq!(fixture.verdict(ticket), Some(format!("landed {main}")));
    }
    let mut fleet = fixture.fleet();
    fleet.sort();
    assert_eq!(fleet, ["a", "b", "c"].map(|name| format!("fleet send session-lane-{name} landed lane-{name} as {main}")), "only landed messages go out");
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
    assert_eq!(checks.len(), 4, "the third {class} halves the candidate and checks its first half");
    assert_eq!(tree(&fixture, &checks[0].0), tree(&fixture, &checks[2].0));
    assert_ne!(tree(&fixture, &checks[3].0), tree(&fixture, &checks[2].0));
    assert!(tickets.iter().all(|ticket| ticket.exists()), "halving decided a lane");
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
    assert!(fleet.iter().any(|call| call.starts_with("fleet send session-lane-b ") && call.contains(&format!("did not land: {failure}"))), "{fleet:?}");
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
    assert_eq!(checks.len(), 4);
    assert_eq!((tree(&fixture, &checks[3].0), &checks[3].1), (tree(&fixture, &checks[0].0), &checks[0].1), "the deferred candidate is retried as it was");
    let main = fixture.main();
    assert_eq!(main, checks[3].0);
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
    assert!(told.len() == 1 && told[0].starts_with("fleet send session-lane-a north land: lane-a ") && told[0].contains("did not land"), "nothing landed, so the lane is told only its refusal: {told:?}");
    assert!(followed(&refused).is_empty(), "nothing landed, so nothing is followed");
    assert!(refused.pushes_tsv().is_empty());
}

fn followed(fixture: &Fixture) -> Vec<String> {
    fixture.read("north-calls").lines().filter(|line| !line.starts_with("fleet")).map(str::to_owned).collect()
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
