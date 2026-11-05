use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use north_landing::land::queue::{self, RedFix, Ticket};

mod cleanup;
mod sheriff;

mod baseline;
mod conflict;
mod credentials;
mod culprit_search;
mod isolation;
mod landing;
mod ledger;
mod precheck;
mod promote;
mod remote;
mod restart;
mod speculate;
mod spread;

#[path = "../support/land_serve_template.rs"]
mod template;

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
if [ -n "${NORTH_LAND_RECHECK:-}" ]; then
  printf '%s %s\n' "$(git rev-parse HEAD)" "$3" >>"$LAND_TRACE/rechecks"
  printf 'checking\nsafe-push: verdict green\n'
  exit 0
fi
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
[ ! -x "$LAND_TRACE/north-hook" ] || exec "$LAND_TRACE/north-hook" "$@"
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

fn layout(root: &Path) -> (PathBuf, PathBuf, PathBuf, PathBuf, PathBuf) {
    (root.join("origin.git"), root.join("proj/main"), root.join("state"), root.join("trace"), root.join("bin"))
}

fn build_template(root: &Path) -> String {
    let (origin, repo, state, trace, bin) = layout(root);
    for directory in [&state, &trace, &bin] {
        fs::create_dir_all(directory).unwrap();
    }
    fs::write(root.join("loadavg"), "0.50 0.50 0.50 1/100 1\n").unwrap();
    git(root, &["init", "-q", "--bare", "-b", "main", origin.to_str().unwrap()]);
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
    fs::write(root.join("threads.db"), b"").unwrap();
    executable(&trace.join("checker"), CHECKER);
    executable(&trace.join("north"), NORTH);
    executable(&trace.join("mint"), MINT);
    std::os::unix::fs::symlink(crate::support::push_trace_git(), bin.join("git")).unwrap();
    git(&repo, &["rev-parse", "HEAD"])
}

impl Fixture {
    fn new() -> Self {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path().to_owned();
        let template = template::Template::get(build_template);
        template.copy_into(&root);
        let (origin, repo, state, trace, bin) = layout(&root);
        Self { _scratch: scratch, root, origin, repo, state, trace, bin, base: template.base.clone(), queued: now_micros() }
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
            red_fix,
            ..Default::default()
        };
        (self.queue().join(queue::file_name(self.queued, &branch)), ticket)
    }

    fn command(&self) -> Command {
        self.command_with(["--repo", self.repo.to_str().unwrap()])
    }

    fn command_with<const N: usize>(&self, selection: [&str; N]) -> Command {
        let mut command = self.north();
        command.args(["land", "serve", "--once"]).args(selection).args(["--checker", self.trace.join("checker").to_str().unwrap(), "--app-token", self.trace.join("mint").to_str().unwrap()]);
        command
    }

    fn north(&self) -> Command {
        let path = format!("{}:{}", self.bin.display(), std::env::var("PATH").unwrap());
        let mut command = Command::new(env!("CARGO_BIN_EXE_north"));
        command
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("NORTH_RUNTIME_LINK")
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
            .env("AGENT_CAPACITY_LOADAVG", self.root.join("loadavg"))
            .env("AGENT_CAPACITY_CORES", "4")
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

fn tree(fixture: &Fixture, commit: &str) -> String {
    git(&fixture.repo, &["rev-parse", &format!("{commit}^{{tree}}")])
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

fn pushed_trees_were_checked(fixture: &Fixture) {
    let checked: Vec<String> = fixture.checks().iter().map(|(head, _)| tree(fixture, head)).collect();
    let pushed: Vec<String> = fixture.read("pushes").lines().filter_map(|line| line.split_whitespace().find_map(|word| word.strip_suffix(":refs/heads/main")).map(str::to_owned)).collect();
    assert!(pushed.last() == Some(&fixture.main()), "main is the last pushed commit: {pushed:?}");
    for commit in pushed {
        let pushed = tree(fixture, &commit);
        assert!(checked.contains(&pushed), "pushed {commit} carries tree {pushed}, which no check saw: {checked:?}");
    }
}

fn stamp(fixture: &Fixture, name: &str) -> f64 {
    fixture.read(name).trim().parse().unwrap_or_else(|_| panic!("{name} holds no time: {:?}", fixture.read(name)))
}
