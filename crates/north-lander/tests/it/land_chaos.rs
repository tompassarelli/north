use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::cell::Cell;

use north_base::platform::wait;
use super::serve::HANG;
use north_lander::land::candidate::{self, git};
use north_lander::land::lander::Lander;
use north_lander::land::serve::Serve;
use north_lander::land::watch::Watch;
use north_lander::land::queue::{self, Ticket};

const CHILD: &str = "NORTH_LAND_CHAOS_HOME";
const LANDER_ROLE: &str = "NORTH_LAND_CHAOS_LANDER";
const FIRST_TICKET: u128 = 1_790_000_000_000_000;

const CHECKER: &str = r#"dir="$1"; plan="$1/plan"; calls="$1/calls"
shift
mkdir -p "$calls"
n=1
while ! mkdir "$calls/$n" 2>/dev/null; do n=$((n + 1)); done
step=$(sed -n "${n}p" "$plan" 2>/dev/null)
case "$step" in
lost) echo "ssh: connect to host gex45 port 22: Connection reset by peer" >&2; exit 255 ;;
hold) while [ ! -e "$dir/release" ]; do sleep 0.05; done; echo "safe-push: verdict green" ;;
*) echo "safe-push: verdict green" ;;
esac
"#;

const AFTER_RUNNER: &str = r#"[ "$1" = land ] || { echo "$*" >> "$HOME/north-calls"; exit 0; }
eval outcome=\${$#}
while [ -e "$HOME/stall" ]; do echo $$ > "$HOME/stalled"; sleep 0.05; done
[ -e "$HOME/transient" ] && exit 75
case " $* " in *" --command "*) ;; *) git rev-parse HEAD >> "$HOME/after-runs" ;; esac
if [ -e bad.txt ]; then printf "event\tafter\tfailed\tbad.txt check\nfailed-step\tbad.txt check\t\n" > "$outcome"; exit 1; fi
"#;

fn isolated(name: &str, scenario: fn(&World)) {
    if let Some(home) = std::env::var_os(CHILD) {
        let home = Path::new(&home);
        scenario(&if std::env::var_os(LANDER_ROLE).is_some() { World::open(home) } else { World::new(home) });
        return;
    }
    let home = tempfile::tempdir().unwrap();
    let gitconfig = home.path().join("gitconfig");
    fs::write(&gitconfig, "[init]\n\tdefaultBranch = main\n[commit]\n\tgpgsign = false\n[user]\n\tname = chaos\n\temail = chaos@example.com\n").unwrap();
    let north = home.path().join("north");
    north_foundation::foundation::shim::write_script(&north, AFTER_RUNNER);
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([name, "--exact", "--nocapture", "--test-threads=1"])
        .env(CHILD, home.path())
        .env("HOME", home.path())
        .env("XDG_STATE_HOME", home.path().join("state"))
        .env("NORTH_TEST_ROOT", home.path())
        .env("NORTH_LAND_NORTH", &north)
        .env("NORTH_OFFLOAD_ROOT", home.path().join("offload"))
        .env("GIT_CONFIG_GLOBAL", &gitconfig)
        .env("GIT_CONFIG_NOSYSTEM", "1");
    let output = north_base::platform::process::try_run_group_traced_within(command, HANG)
        .unwrap()
        .unwrap_or_else(|processes| panic!("{name} ran past {HANG:?} in its isolated home: {processes}"));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success() && stdout.contains("1 passed"), "{name} failed in its isolated home:\n{stdout}\n{}", String::from_utf8_lossy(&output.stderr));
}

fn run(directory: &Path, arguments: &[&str]) -> String {
    git(directory, arguments).unwrap_or_else(|error| panic!("{error}"))
}

struct Lane {
    name: String,
    subject: String,
}

struct World {
    root: PathBuf,
    origin: PathBuf,
    main: PathBuf,
    plan: PathBuf,
    checker: Vec<String>,
    tickets: Cell<u128>,
}

impl World {
    fn open(root: &Path) -> Self {
        let plan = root.join("checker");
        let script = root.join("checker.sh");
        let checker = vec!["sh".to_owned(), script.display().to_string(), plan.display().to_string()];
        Self { root: root.to_owned(), origin: root.join("origin.git"), main: root.join("proj/main"), plan, checker, tickets: Cell::new(FIRST_TICKET) }
    }

    fn new(root: &Path) -> Self {
        let World { origin, main, plan, .. } = Self::open(root);
        fs::create_dir_all(&plan).unwrap();
        fs::create_dir_all(main.parent().unwrap()).unwrap();
        run(root, &["init", "-q", "--bare", "-b", "main", origin.to_str().unwrap()]);
        run(root, &["clone", "-q", origin.to_str().unwrap(), main.to_str().unwrap()]);
        fs::write(main.join("README"), "base\n").unwrap();
        fs::write(main.join(".safe-push"), "[landing]\n\tpush-app = lander\n").unwrap();
        run(&main, &["add", "-A"]);
        run(&main, &["commit", "-q", "-m", "base"]);
        run(&main, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
        fs::write(root.join("checker.sh"), CHECKER).unwrap();
        fs::write(root.join("token.sh"), "echo app-token\n").unwrap();
        Self::open(root)
    }

    fn lander(&self) -> Lander {
        let mut lander = Lander::new(&self.main, self.checker.clone()).unwrap();
        lander.app_token = vec!["sh".to_owned(), self.root.join("token.sh").display().to_string()];
        lander
    }

    fn lane_in(&self, checkout: &Path, root: &Path, branch: &str, file: &str) -> Ticket {
        run(checkout, &["fetch", "-q", "origin", "main"]);
        if checkout == root {
            run(root, &["checkout", "-q", "-B", branch, "FETCH_HEAD"]);
        } else {
            run(checkout, &["worktree", "add", "-q", "-b", branch, root.to_str().unwrap(), "FETCH_HEAD"]);
        }
        fs::write(root.join(file), format!("{branch}\n")).unwrap();
        run(root, &["add", "-A"]);
        run(root, &["commit", "-q", "-m", &format!("lane {branch} {file}")]);
        Ticket {
            origin: self.origin.display().to_string(),
            root: root.to_owned(),
            host: "laptop".into(),
            branch: branch.into(),
            head: run(root, &["rev-parse", "HEAD"]),
            alone_green: Some(run(root, &["rev-parse", "HEAD"])),
            alone_base: Some(run(root, &["rev-parse", "HEAD~"])),
            session: None,
            queued_at: "2026-11-05T12:00:00Z".into(),
            ..Default::default()
        }
    }

    fn lane(&self, branch: &str) -> Ticket {
        let root = self.root.join("proj/worktrees").join(queue::slug(branch));
        self.lane_in(&self.main, &root, branch, &format!("{}.txt", queue::slug(branch)))
    }

    fn enqueue(&self, lander: &Lander, ticket: &Ticket) -> Lane {
        self.tickets.set(self.tickets.get() + 1);
        let name = queue::file_name(self.tickets.get(), &ticket.branch);
        queue::write(&lander.queue.join(&name), ticket).unwrap();
        Lane { name, subject: run(&ticket.root, &["log", "-1", "--format=%s", &ticket.head]) }
    }

    fn landed_subjects(&self) -> Vec<String> {
        run(&self.origin, &["log", "--format=%s", "main"]).lines().map(str::to_owned).collect()
    }

    fn settled(&self, lander: &Lander, lanes: &[&Lane]) -> Vec<String> {
        let left: Vec<_> = queue::pending(&lander.queue).unwrap().into_iter().map(|(path, _)| path).collect();
        assert!(left.is_empty(), "lanes still queued: {left:?}");
        let subjects = self.landed_subjects();
        let main = run(&self.origin, &["rev-parse", "main"]);
        lanes
            .iter()
            .map(|lane| {
                let done = lander.queue.join("done").join(&lane.name);
                let verdict = queue::read(&done).unwrap_or_else(|error| panic!("{} was lost: {error}", lane.name)).verdict.unwrap_or_default();
                let copies = subjects.iter().filter(|subject| **subject == lane.subject).count();
                if let Some(sha) = verdict.strip_prefix("landed ") {
                    assert_eq!(copies, 1, "{} landed {copies} times: {subjects:?}", lane.name);
                    assert!(candidate::output(&self.origin, &["merge-base", "--is-ancestor", sha, &main]).unwrap().status.success(), "{} names {sha}, which is not on main", lane.name);
                    assert_eq!(run(&self.origin, &["log", "--format=%s", "-1", sha]).trim(), lane.subject, "on 2026-11-08 the lander told a lane of a stacked landing the top commit of the push, another lane's, so {} must name its own commit", lane.name);
                } else {
                    assert!(verdict.starts_with("failed ") || verdict.starts_with("conflict "), "{} ended neither landed nor refused: {verdict}", lane.name);
                    assert_eq!(copies, 0, "{} was refused yet reached main", lane.name);
                }
                verdict
            })
            .collect()
    }
}

fn drive(lander: &Lander, state: &mut Watch) -> Vec<String> {
    let mut rounds = Vec::new();
    let drained = wait::until(HANG, || {
        if queue::pending(&lander.queue).unwrap().is_empty() && state.in_flight() == 0 {
            return true;
        }
        rounds.push(format!("{:?}", Serve::new(lander).round(state)));
        false
    });
    assert!(drained, "the queue never drained: {:?}", &rounds[rounds.len().saturating_sub(5)..]);
    rounds
}

fn landed(verdicts: &[String]) {
    assert!(verdicts.iter().all(|verdict| verdict.starts_with("landed ")), "{verdicts:?}");
}


fn handed_in_with_nothing_committed(world: &World) {
    let lander = world.lander();
    let a = world.enqueue(&lander, &world.lane("a"));
    drive(&lander, &mut Watch::default());
    let old = run(&world.origin, &["rev-parse", "main~"]);
    let root = world.root.join("proj/worktrees/uncommitted");
    run(&world.main, &["worktree", "add", "-q", "-b", "uncommitted", root.to_str().unwrap(), &old]);
    fs::write(root.join("README"), "the change never committed\n").unwrap();
    fs::write(root.join("new.txt"), "untracked\n").unwrap();
    let ticket = Ticket { origin: world.origin.display().to_string(), root: root.clone(), host: "laptop".into(), branch: "uncommitted".into(), head: old, queued_at: "2026-11-12T02:07:00Z".into(), ..Default::default() };
    let uncommitted = world.enqueue(&lander, &ticket);
    drive(&lander, &mut Watch::default());
    landed(&world.settled(&lander, &[&a]));
    let verdict = queue::read(&lander.queue.join("done").join(&uncommitted.name)).unwrap().verdict.unwrap_or_default();
    assert!(verdict.starts_with("failed "), "on 2026-11-12 a ticket whose head was an old main commit was answered with main's head as its landing: {verdict}");
    assert_eq!((fs::read_to_string(root.join("README")).ok().as_deref(), root.join("new.txt").exists()), (Some("the change never committed\n"), true), "the lander removed a lane that still held uncommitted work");
}

#[test]
fn chaos_a_ticket_that_adds_no_commit_to_main_is_never_landed_and_its_dirty_lane_is_kept() {
    isolated("land_chaos::chaos_a_ticket_that_adds_no_commit_to_main_is_never_landed_and_its_dirty_lane_is_kept", handed_in_with_nothing_committed);
}
