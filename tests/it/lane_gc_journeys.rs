use north_foundation::foundation::shim;
use std::fs;
use std::path::{Path, PathBuf};
use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::sync::LazyLock;

use serde_json::{Value, json};

use crate::support::template::Template;

struct Rig {
    root: PathBuf,
    home: PathBuf,
    stubs: PathBuf,
    env: Vec<(String, String)>,
}

struct Holder(Child);

impl Drop for Holder {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn executable(path: &Path, text: &str) {
    shim::write_script(path, text);
}

impl Rig {
    fn new(root: &Path, home: &Path) -> Self {
        let stubs = root.join("stubs");
        fs::create_dir_all(&stubs).unwrap();
        fs::create_dir_all(home.join("code")).unwrap();
        let config = root.join("gitconfig");
        fs::write(&config, "[user]\n\tname = fixture\n\temail = fixture@example.invalid\n[init]\n\tdefaultBranch = main\n[advice]\n\tdetachedHead = false\n[gc]\n\tauto = 0\n[maintenance]\n\tauto = false\n[receive]\n\tautogc = false\n").unwrap();
        let path = format!("{}:{}", stubs.display(), std::env::var("PATH").unwrap());
        let env = [
            ("HOME", home.display().to_string()),
            ("GIT_CONFIG_GLOBAL", config.display().to_string()),
            ("GIT_CONFIG_NOSYSTEM", "1".into()),
            ("RESIDENT_CAP_HOST", "fixture-host".into()),
            ("LANE_GC_FIXTURE", root.display().to_string()),
            ("LANE_GC_REAL_GIT", real_git()),
            ("LANE_GC_STALE_FILE", root.join("stale.json").display().to_string()),
            ("LANE_GC_MIRRORS", root.join("mirrors").display().to_string()),
            ("FLEET_TEST_FILE", root.join("fleet.json").display().to_string()),
            ("FLEET_TEST_SENDS", root.join("fleet-sends").display().to_string()),
            ("PATH", path),
        ];
        let rig = Self { root: root.into(), home: home.into(), stubs, env: env.into_iter().map(|(k, v)| (k.into(), v)).collect() };
        rig.stub("ssh", "cat >/dev/null\necho '{\"ok\":true,\"branches\":{}}'\n");
        rig.stub("supply-alert", "[ ! -e \"$LANE_GC_FIXTURE/ping-fault\" ] || exit 1\nfor arg; do printf '%s|' \"$arg\"; done >>\"$LANE_GC_FIXTURE/alerts\"\necho >>\"$LANE_GC_FIXTURE/alerts\"\n");
        rig.stub(
            "north",
            "case \"$1 $2\" in\n\"fleet list\") cat \"$FLEET_TEST_FILE\" ;;\n\"threads \"*) printf 'item\\tholder\\n' ;;\n\"safe-push abandon\") printf '%s\\n' \"$*\" >>\"$LANE_GC_FIXTURE/abandons\"; git push -q origin \":refs/heads/$4\" ;;\n\"fleet send\") [ ! -e \"$LANE_GC_FIXTURE/ping-fault\" ] || exit 1; printf '%s\\n' \"$*\" >>\"$FLEET_TEST_SENDS\" ;;\n*) exit 1 ;;\nesac\n",
        );
        rig
    }

    fn stub(&self, name: &str, text: &str) {
        executable(&self.stubs.join(name), text);
    }

    fn git(&self, dir: &Path, args: &[&str]) -> String {
        let output = Command::new(real_git()).args(args).current_dir(dir).envs(self.env.iter().filter(|(k, _)| k != "PATH").cloned()).output().unwrap();
        assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn git_stdin(&self, dir: &Path, args: &[&str], input: &str) {
        let mut child = Command::new(real_git()).args(args).current_dir(dir).envs(self.env.iter().filter(|(k, _)| k != "PATH").cloned()).stdin(Stdio::piped()).spawn().unwrap();
        child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
        assert!(child.wait().unwrap().success(), "git {args:?}");
    }

    fn git_ok(&self, dir: &Path, args: &[&str]) -> bool {
        Command::new(real_git()).args(args).current_dir(dir).envs(self.env.iter().filter(|(k, _)| k != "PATH").cloned()).output().unwrap().status.success()
    }

    fn lane_gc(&self, now: &str, extra: &[(&str, &str)], args: &[&str]) -> String {
        let mut child = Command::new(crate::support::bin_exe!("CARGO_BIN_EXE_north"))
            .args(["lane", "gc", "--sessions-stdin"])
            .arg("--root")
            .arg(self.home.join("code"))
            .arg("--state")
            .arg(self.root.join("unlanded.txt"))
            .args(args)
            .envs(self.env.iter().cloned())
            .env("LANE_GC_NOW", now)
            .envs(extra.iter().copied())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let sessions = fs::read_to_string(self.root.join("fleet.json")).unwrap_or_else(|_| "[]".into());
        child.stdin.take().unwrap().write_all(sessions.as_bytes()).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8_lossy(&output.stdout).into_owned() + &String::from_utf8_lossy(&output.stderr)
    }

    fn stub_github(&self) {
        self.stub("gh", "printf '%s\\n' \"$*\" >>\"$LANE_GC_FIXTURE/gh-calls\"\nexit 1\n");
    }

    fn github_calls(&self) -> usize {
        fs::read_to_string(self.root.join("gh-calls")).map_or(0, |calls| calls.lines().count())
    }

    fn read(&self, name: &str) -> String {
        fs::read_to_string(self.root.join(name)).unwrap_or_default()
    }

    fn fleet(&self, sessions: &[(&str, Option<&str>, &str)]) {
        let rows: Vec<Value> = sessions
            .iter()
            .map(|(name, parent, repo)| json!({"name": name, "conversation": name, "parent": parent, "provider": "claude", "status": "working", "cwd": self.home.join("code").join(repo).join("main")}))
            .collect();
        fs::write(self.root.join("fleet.json"), serde_json::to_string(&rows).unwrap()).unwrap();
    }
}

static REAL_GIT: LazyLock<String> = LazyLock::new(|| {
    let output = Command::new("sh").args(["-c", "command -v git"]).output().unwrap();
    String::from_utf8_lossy(&output.stdout).trim().to_string()
});

fn real_git() -> String {
    REAL_GIT.clone()
}

fn write(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
}

static CLASSIFIED: LazyLock<Template> = LazyLock::new(|| Template::build(Classified::build));

struct Classified {
    rig: Rig,
    main: PathBuf,
    now: String,
    _holder: Holder,
}

impl Classified {
    fn project(rig: &Rig, name: &str) -> PathBuf {
        let origin = rig.root.join(format!("origin/{name}.git"));
        let main = rig.home.join(format!("code/{name}/main"));
        fs::create_dir_all(&origin).unwrap();
        rig.git(&origin, &["init", "-q", "--bare"]);
        rig.git(&rig.root, &["clone", "-q", origin.to_str().unwrap(), main.to_str().unwrap()]);
        fs::create_dir_all(rig.home.join(format!("code/{name}/worktrees"))).unwrap();
        write(&main.join("README"), "readme\n");
        write(&main.join(".gitignore"), "*.log\n");
        rig.git(&main, &["add", "README", ".gitignore"]);
        rig.git(&main, &["commit", "-qm", "init"]);
        rig.git(&main, &["push", "-q", "origin", "main"]);
        main
    }

    fn lane(rig: &Rig, main: &Path, name: &str, message: Option<&str>) -> PathBuf {
        let path = rig.home.join("code/north/worktrees").join(name);
        rig.git(main, &["worktree", "add", "-q", path.to_str().unwrap(), "-b", name]);
        if let Some(message) = message {
            write(&path.join(format!("{name}.txt")), &format!("{name}\n"));
            rig.git(&path, &["add", &format!("{name}.txt")]);
            rig.git(&path, &["commit", "-qm", message]);
        }
        path
    }

    fn build(root: &Path) {
        let rig = Rig::new(root, &root.join("home"));
        rig.stub_github();
        let main = Self::project(&rig, "north");
        Self::lane(&rig, &main, "equiv", Some("equiv change"));
        write(&main.join("other.txt"), "other\n");
        rig.git(&main, &["add", "other.txt"]);
        rig.git(&main, &["commit", "-qm", "other"]);
        rig.git(&main, &["cherry-pick", "equiv"]);
        rig.git(&main, &["push", "-q", "origin", "main"]);
        Self::lane(&rig, &main, "sup", Some("superseded change\n\nRefs north#5"));
        rig.git(&main, &["push", "-q", "origin", "sup"]);
        Self::lane(&rig, &main, "own", Some("half done\n\nRefs north#5\nRefs north#6"));
        let wip = Self::lane(&rig, &main, "wip", None);
        fs::write(wip.join("README"), "readme\nedited\n").unwrap();
        write(&wip.join("new.txt"), "new\n");
        write(&wip.join("build.log"), "noise\n");
        let held = Self::lane(&rig, &main, "held", None);
        fs::write(held.join("README"), "readme\nedited\n").unwrap();
        rig.git(&main, &["checkout", "-q", "-b", "loose"]);
        write(&main.join("loose.txt"), "loose\n");
        rig.git(&main, &["add", "loose.txt"]);
        rig.git(&main, &["commit", "-qm", "no issue named"]);
        rig.git(&main, &["checkout", "-q", "main"]);
        let galileo = Self::project(&rig, "galileo");
        rig.git(&galileo, &["checkout", "-q", "-b", "unique"]);
        write(&galileo.join("u.txt"), "u\n");
        rig.git(&galileo, &["add", "u.txt"]);
        rig.git(&galileo, &["commit", "-qm", "unique"]);
        rig.git(&galileo, &["checkout", "-q", "main"]);
        let unique = rig.git(&galileo, &["rev-parse", "unique"]);
        let lanes: String = (1..=26).map(|i| format!("create refs/heads/lane-{i} {unique}\n")).collect();
        rig.git_stdin(&galileo, &["update-ref", "--stdin"], &lanes);
        rig.git(&galileo, &["branch", "-q", "-D", "unique"]);
        let legacy = rig.git(&main, &["rev-parse", "equiv"]);
        let legacy_tag = format!("archive/north/equiv/{}", &legacy[..12]);
        rig.git(&main, &["tag", &legacy_tag, &legacy]);
        rig.git(&main, &["push", "-q", "origin", &format!("refs/tags/{legacy_tag}")]);
    }

    fn new(root: &Path) -> Self {
        CLASSIFIED.copy_into(root);
        let rig = Rig::new(root, &root.join("home"));
        Self::relocate(&rig, CLASSIFIED.built_at());
        rig.fleet(&[("north-parent", None, "north"), ("north-worker", Some("north-parent"), "north"), ("scratch-parent", None, "galileo"), ("scratch-worker", Some("scratch-parent"), "galileo")]);
        let main = rig.home.join("code/north/main");
        let holder = Holder(Command::new("sleep").arg("300").current_dir(rig.home.join("code/north/worktrees/held")).spawn().unwrap());
        let built: u64 = rig.git(&main, &["log", "--all", "-1", "--format=%ct"]).parse().unwrap();
        let at = built + 30 * 3600;
        let now = String::from_utf8(Command::new("date").args(["-u", "-d", &format!("@{at}"), "+%FT%TZ"]).output().unwrap().stdout).unwrap().trim().to_string();
        Self { rig, main, now, _holder: holder }
    }

    fn relocate(rig: &Rig, template: &Path) {
        let code = rig.home.join("code");
        let worktrees: Vec<PathBuf> = fs::read_dir(code.join("north/worktrees")).unwrap().map(|entry| entry.unwrap().path()).collect();
        let admin: Vec<PathBuf> = fs::read_dir(code.join("north/main/.git/worktrees")).unwrap().map(|entry| entry.unwrap().path().join("gitdir")).collect();
        let configs = ["north", "galileo"].map(|name| code.join(format!("{name}/main/.git/config")));
        let (from, to) = (template.to_str().unwrap(), rig.root.to_str().unwrap());
        for file in worktrees.iter().map(|tree| tree.join(".git")).chain(admin).chain(configs) {
            let text = fs::read_to_string(&file).unwrap();
            assert!(text.contains(from), "{} names no template path", file.display());
            write(&file, &text.replace(from, to));
        }
        let stale = Command::new("grep").args(["-rlF", from]).arg(&rig.root).output().unwrap();
        assert!(stale.stdout.is_empty(), "template paths left in the copy: {}", String::from_utf8_lossy(&stale.stdout));
    }

    fn run(&self, args: &[&str]) -> String {
        let stamp = String::from_utf8(Command::new("date").args(["-u", "+%FT%TZ"]).output().unwrap().stdout).unwrap();
        let stamp = stamp.trim();
        self.rig.lane_gc(&self.now, &[("GIT_AUTHOR_DATE", stamp), ("GIT_COMMITTER_DATE", stamp)], args)
    }

    fn kept_intact(&self) -> bool {
        ["wip", "held"]
            .iter()
            .all(|b| self.rig.home.join("code/north/worktrees").join(b).is_dir() && self.rig.git_ok(&self.main, &["rev-parse", "-q", "--verify", &format!("refs/heads/{b}")]))
    }

    fn gone(&self, lane: &str) -> bool {
        !self.rig.home.join("code/north/worktrees").join(lane).exists() && !self.rig.git_ok(&self.main, &["rev-parse", "-q", "--verify", &format!("refs/heads/{lane}")])
    }
}

#[test]
fn unheld_unlanded_lanes_are_archived_after_a_day_and_held_or_ignored_ones_stay_reported() {
    let scratch = tempfile::tempdir().unwrap();
    let fixture = Classified::new(scratch.path());
    let rig = &fixture.rig;
    for branch in ["gone", "equiv", "wip"] {
        rig.git(&fixture.main, &["config", &format!("branch.{branch}.remote"), "origin"]);
    }
    rig.git(&fixture.main, &["config", "branch.own.remote", "origin"]);
    rig.git(&fixture.main, &["config", "branch.own.merge", "refs/heads/main"]);

    let dry = fixture.run(&["--dry-run"]);
    for expected in [
        "north: retire ~/code/north/worktrees/equiv (equiv, ",
        "north: archive ~/code/north/worktrees/sup (",
        "north: stale-dirty ~/code/north/worktrees/wip: surface to north-parent",
        "north: archive ~/code/north/worktrees/own (unheld, issues north#5 north#6) to ~/archive/lanes/north, then reap",
        "north: owned ~/code/north/worktrees/held: surface to north-parent",
        "north: archive branch loose (unheld, names no issue) to ~/archive/lanes/north, then reap",
        "galileo: archive branch lane-26 (unheld, names no issue) to ~/archive/lanes/galileo, then reap",
    ] {
        assert!(dry.contains(expected), "{expected}\n{dry}");
    }
    assert!(!dry.contains("would alert"), "{dry}");
    assert!(fixture.kept_intact() && !fixture.gone("equiv") && !rig.root.join("alerts").exists());
    assert!(rig.git_ok(&fixture.main, &["config", "--get", "branch.gone.remote"]), "a dry run prunes nothing");
    assert!(dry.contains("north: drop the origin/main upstream of 1 lane branches"), "{dry}");

    let out = fixture.run(&[]);
    assert!(fixture.gone("equiv") && fixture.gone("sup") && fixture.gone("own") && fixture.kept_intact(), "{out}");
    assert!(!rig.git_ok(&fixture.main, &["rev-parse", "-q", "--verify", "refs/heads/loose"]), "{out}");
    assert!(!rig.git_ok(&fixture.main, &["config", "--get", "branch.gone.remote"]), "a section whose branch no longer exists is pruned: {out}");
    assert!(!rig.git_ok(&fixture.main, &["config", "--get", "branch.own.merge"]) && !rig.git_ok(&fixture.main, &["config", "--get", "branch.own.remote"]), "a kept lane drops the origin/main upstream lanes no longer write: {out}");
    assert!(!rig.git_ok(&fixture.main, &["config", "--get", "branch.equiv.remote"]), "a section whose branch this run deleted is pruned: {out}");
    assert!(rig.git_ok(&fixture.main, &["config", "--get", "branch.wip.remote"]), "a kept branch keeps its config: {out}");
    assert_eq!(rig.github_calls(), 0, "no GitHub call: {}", rig.read("gh-calls"));
    let wip = rig.home.join("code/north/worktrees/wip");
    assert!(wip.join("new.txt").is_file() && fs::read_to_string(wip.join("README")).unwrap().contains("edited"));

    let mut unlanded: Vec<String> = rig.read("unlanded.txt").lines().filter(|line| !line.starts_with('#')).map(|line| line.split('\t').collect::<Vec<_>>()).filter(|fields| fields[1] == "north").map(|fields| fields[3].to_string()).collect();
    unlanded.sort();
    assert_eq!(unlanded, ["held", "wip"]);
    let stale: Value = serde_json::from_str(&rig.read("stale.json")).unwrap();
    let mut owned: Vec<(String, String)> = stale["repos"]["north"]["owned"].as_array().unwrap().iter().map(|row| (row["branch"].as_str().unwrap().into(), row["owner"].as_str().unwrap().into())).collect();
    owned.sort();
    let parent = "north-parent".to_string();
    assert_eq!(owned, [("held".into(), parent.clone()), ("wip".into(), parent)]);
    assert!(!rig.root.join("alerts").exists(), "no parent holds lanes past the alert threshold once the unheld ones are archived");

    let bundles: Vec<PathBuf> = fs::read_dir(rig.home.join("archive/lanes/north")).unwrap().map(|entry| entry.unwrap().path()).filter(|path| path.extension().is_some_and(|ext| ext == "bundle")).collect();
    assert_eq!(bundles.len(), 3, "{out}");
    for bundle in &bundles {
        assert!(rig.git_ok(&fixture.main, &["bundle", "verify", "-q", bundle.to_str().unwrap()]));
    }
}

impl Rig {
    fn stub_origin(&self) {
        self.stub("git", "case \"$*\" in *\"remote get-url origin\"*) echo https://github.com/tompassarelli/north.git; exit 0 ;; esac\nexec \"$LANE_GC_REAL_GIT\" \"$@\"\n");
        self.stub_github();
    }
}

#[test]
fn the_origin_sweep_abandons_branches_whose_changes_are_on_main_and_keeps_unlanded_ones_without_a_github_call() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    let rig = Rig::new(root, &root.join("home"));
    let main = Classified::project(&rig, "north");
    let lane = |branch: &str, file: &str, lines: &str, message: &str| {
        rig.git(&main, &["checkout", "-q", "-b", branch, "main"]);
        write(&main.join(file), lines);
        rig.git(&main, &["add", file]);
        rig.git(&main, &["commit", "-qm", message]);
        rig.git(&main, &["push", "-q", "origin", branch]);
        rig.git(&main, &["checkout", "-q", "main"]);
    };
    lane("scratch/n882-climb", "climb.txt", "a\nb\nc\n", "Climb the tiers\n\n(north#882)");
    lane("scratch/n882-lines", "lines.txt", "a\nb\nmissing\n", "Keep the lines\n\n(north#882)");
    lane("scratch/n882-retain", "retain.txt", "r\n", "Retain the record\n\n(north#882)");
    lane("scratch/n882-textrel", "textrel.txt", "t\n", "Text release\n\n(north#882)");
    let unlanded = [
        ("scratch/n892-promise", 892),
        ("scratch/n892-expiry", 892),
        ("scratch/n892-solo", 892),
        ("scratch/n892-solo-v2", 892),
        ("scratch/n892-shared-config", 892),
        ("scratch/n892-addressee", 892),
        ("q902-bets", 842),
        ("scratch/n903-waits-wip", 903),
        ("n706-pin-retire", 706),
    ];
    for (branch, issue) in unlanded {
        lane(branch, &format!("{}.txt", branch.replace('/', "-")), &format!("{branch}\n"), &format!("Work of {branch}\n\n(north#{issue})"));
    }
    rig.git(&main, &["merge", "-q", "--ff-only", "scratch/n882-textrel"]);
    write(&main.join("climb.txt"), "a\nb\nc\nd\n");
    rig.git(&main, &["add", "climb.txt"]);
    rig.git(&main, &["commit", "-qm", "Climb the tiers\n\n(north#882)"]);
    write(&main.join("lines.txt"), "a\nb\nc\n");
    rig.git(&main, &["add", "lines.txt"]);
    rig.git(&main, &["commit", "-qm", "Keep the lines\n\n(north#882)"]);
    rig.git(&main, &["cherry-pick", "scratch/n882-retain"]);
    rig.git(&main, &["push", "-q", "origin", "main"]);
    let head = rig.git(&main, &["rev-parse", "main"]);
    rig.stub_origin();
    rig.fleet(&[("north-parent", None, "north")]);
    let now = String::from_utf8(Command::new("date").args(["-u", "-d", "+30 hours", "+%FT%TZ"]).output().unwrap().stdout).unwrap();

    let out = rig.lane_gc(now.trim(), &[], &["--remote", "--repo", "north"]);

    let abandoned: Vec<String> = rig.read("abandons").lines().map(|line| line.split(' ').nth(3).unwrap().to_owned()).collect();
    let mut abandoned_sorted = abandoned.clone();
    abandoned_sorted.sort();
    assert_eq!(abandoned_sorted, ["scratch/n882-climb", "scratch/n882-retain", "scratch/n882-textrel"], "{out}");
    assert!(rig.read("abandons").lines().all(|line| line.starts_with("safe-push abandon --superseded ") && line.contains(" --by ")), "{}", rig.read("abandons"));
    let remaining = rig.git(&main, &["ls-remote", "--heads", "origin"]);
    for gone in &abandoned {
        assert!(!remaining.contains(&format!("refs/heads/{gone}")), "{gone} still on origin");
    }
    for (kept, _) in unlanded.iter().chain(&[("scratch/n882-lines", 0)]) {
        assert!(remaining.lines().any(|line| line.ends_with(&format!("refs/heads/{kept}"))), "{kept} was deleted: {out}");
    }
    assert_eq!(rig.git(&main, &["rev-parse", "origin/main"]), head);

    assert_eq!(rig.github_calls(), 0, "{}", rig.read("gh-calls"));
    assert!(out.contains("origin scratch/n882-lines: KEEP unlanded"), "{out}");
}
