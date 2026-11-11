use north_core::foundation::shim;
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
        let output = Command::new(crate::support::bin_exe!("CARGO_BIN_EXE_north"))
            .args(["lane", "gc"])
            .arg("--root")
            .arg(self.home.join("code"))
            .arg("--state")
            .arg(self.root.join("unlanded.txt"))
            .args(args)
            .envs(self.env.iter().cloned())
            .env("LANE_GC_NOW", now)
            .envs(extra.iter().copied())
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8_lossy(&output.stdout).into_owned() + &String::from_utf8_lossy(&output.stderr)
    }

    fn stub_github(&self, open: &[u64]) {
        let rows: Vec<String> = open.iter().map(|number| format!("{{\"number\":{number},\"repository\":{{\"name\":\"north\"}}}}")).collect();
        self.stub("gh", &format!("printf '%s\\n' \"$*\" >>\"$LANE_GC_FIXTURE/gh-calls\"\ncase \"$1 $2\" in\n\"search issues\") echo '[{}]' ;;\n*) exit 1 ;;\nesac\n", rows.join(",")));
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
        rig.stub_github(&[6]);
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
        let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() + 30 * 3600;
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
        ["wip", "own", "held"]
            .iter()
            .all(|b| self.rig.home.join("code/north/worktrees").join(b).is_dir() && self.rig.git_ok(&self.main, &["rev-parse", "-q", "--verify", &format!("refs/heads/{b}")]))
            && self.rig.git_ok(&self.main, &["rev-parse", "-q", "--verify", "refs/heads/loose"])
    }

    fn gone(&self, lane: &str) -> bool {
        !self.rig.home.join("code/north/worktrees").join(lane).exists() && !self.rig.git_ok(&self.main, &["rev-parse", "-q", "--verify", &format!("refs/heads/{lane}")])
    }
}

#[test]
fn unlanded_lanes_stay_whatever_their_age_or_dirt_and_are_reported_with_issue_state() {
    let scratch = tempfile::tempdir().unwrap();
    let fixture = Classified::new(scratch.path());
    let rig = &fixture.rig;
    for branch in ["gone", "equiv", "wip"] {
        rig.git(&fixture.main, &["config", &format!("branch.{branch}.remote"), "origin"]);
    }
    rig.git(&fixture.main, &["config", "branch.sup.remote", "origin"]);
    rig.git(&fixture.main, &["config", "branch.sup.merge", "refs/heads/main"]);

    let dry = fixture.run(&["--dry-run"]);
    for expected in [
        "north: retire ~/code/north/worktrees/equiv (equiv, ",
        "north: superseded ~/code/north/worktrees/sup: keep (touched within 7 days)",
        "north: stale-dirty ~/code/north/worktrees/wip: surface to north-parent",
        "north: owned ~/code/north/worktrees/own: surface to north-parent (north#5 north#6)",
        "north: owned ~/code/north/worktrees/held: surface to north-parent",
        "would alert scratch-parent: 26 owned unlanded lanes",
    ] {
        assert!(dry.contains(expected), "{expected}\n{dry}");
    }
    assert!(dry.lines().any(|line| line == "north: owned branch loose: surface to north-parent"), "{dry}");
    assert!(fixture.kept_intact() && !fixture.gone("equiv") && !rig.root.join("alerts").exists());
    assert!(rig.git_ok(&fixture.main, &["config", "--get", "branch.gone.remote"]), "a dry run prunes nothing");
    assert!(dry.contains("north: drop the origin/main upstream of 1 lane branches"), "{dry}");

    let out = fixture.run(&[]);
    assert!(fixture.gone("equiv") && !fixture.gone("sup") && fixture.kept_intact(), "{out}");
    assert!(!rig.git_ok(&fixture.main, &["config", "--get", "branch.gone.remote"]), "a section whose branch no longer exists is pruned: {out}");
    assert!(!rig.git_ok(&fixture.main, &["config", "--get", "branch.equiv.remote"]), "a section whose branch this run deleted is pruned: {out}");
    assert!(rig.git_ok(&fixture.main, &["config", "--get", "branch.wip.remote"]), "a kept branch keeps its config: {out}");
    assert!(!rig.git_ok(&fixture.main, &["config", "--get", "branch.sup.merge"]) && !rig.git_ok(&fixture.main, &["config", "--get", "branch.sup.remote"]), "a kept lane drops the origin/main upstream lanes no longer write: {out}");
    assert_eq!(rig.github_calls(), 2, "one search per run: {}", rig.read("gh-calls"));
    let wip = rig.home.join("code/north/worktrees/wip");
    assert!(wip.join("new.txt").is_file() && fs::read_to_string(wip.join("README")).unwrap().contains("edited"));

    let mut unlanded: Vec<String> = rig.read("unlanded.txt").lines().filter(|line| !line.starts_with('#')).map(|line| line.split('\t').collect::<Vec<_>>()).filter(|fields| fields[1] == "north").map(|fields| fields[3].to_string()).collect();
    unlanded.sort();
    assert_eq!(unlanded, ["held", "loose", "own", "sup", "wip"]);
    let stale: Value = serde_json::from_str(&rig.read("stale.json")).unwrap();
    let mut owned: Vec<(String, Vec<String>, String)> = stale["repos"]["north"]["owned"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| (row["branch"].as_str().unwrap().into(), row["issues"].as_array().unwrap().iter().map(|r| r.as_str().unwrap().into()).collect(), row["owner"].as_str().unwrap().into()))
        .collect();
    owned.sort();
    let parent = "north-parent".to_string();
    let fault = || vec!["fault: names no issue".to_string()];
    assert_eq!(
        owned,
        [("held".into(), fault(), parent.clone()), ("loose".into(), fault(), parent.clone()), ("own".into(), vec!["north#5=closed".into(), "north#6=open".into()], parent.clone()), ("sup".into(), vec!["north#5=closed".into()], parent.clone()), ("wip".into(), fault(), parent)]
    );
    let alerts = rig.read("alerts");
    assert_eq!(alerts.lines().count(), 1, "{alerts}");
    assert!(alerts.starts_with("normal|scratch-parent: 26 owned unlanded lanes"), "{alerts}");

    let later = String::from_utf8(Command::new("date").args(["-u", "-d", &format!("{} + 8 days", &fixture.now[..10]), "+%F"]).output().unwrap().stdout).unwrap();
    let week_on = fixture.now.replace(&fixture.now[..10], later.trim());
    let out = rig.lane_gc(&week_on, &[], &[]);
    assert!(fixture.gone("sup") && fixture.kept_intact(), "{out}");
    let bundles: Vec<PathBuf> = fs::read_dir(rig.home.join("archive/lanes/north")).unwrap().map(|entry| entry.unwrap().path()).filter(|path| path.extension().is_some_and(|ext| ext == "bundle")).collect();
    assert_eq!(bundles.len(), 1, "{out}");
    assert!(rig.git_ok(&fixture.main, &["bundle", "verify", "-q", bundles[0].to_str().unwrap()]));
}

#[test]
fn a_hundred_unlanded_lanes_cost_one_github_call() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    let rig = Rig::new(root, &root.join("home"));
    rig.stub_github(&(1..=100).collect::<Vec<u64>>());
    let main = Classified::project(&rig, "north");
    for number in 1..=100 {
        rig.git(&main, &["checkout", "-q", "-b", &format!("lane-{number}")]);
        write(&main.join(format!("lane-{number}.txt")), "lane\n");
        rig.git(&main, &["add", "."]);
        rig.git(&main, &["commit", "-qm", &format!("lane {number}\n\nRefs north#{number}")]);
        rig.git(&main, &["checkout", "-q", "main"]);
    }
    rig.fleet(&[("north-parent", None, "north")]);
    let out = rig.lane_gc("2099-01-01T00:00:00Z", &[], &["--dry-run"]);
    assert!(out.contains("100 unlanded") || out.contains("kept"), "{out}");
    assert_eq!(rig.github_calls(), 1, "{}", rig.read("gh-calls"));
}

#[test]
fn the_origin_sweep_waits_an_hour_for_its_own_record() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    let rig = Rig::new(root, &root.join("home"));
    rig.stub_github(&[]);
    Classified::project(&rig, "north");
    rig.fleet(&[("north-parent", None, "north")]);
    let record = "# lane gc 2099-01-01T00:00:00.000Z: origin branch decisions\nkept\n";
    write(&root.join("remote.txt"), record);
    rig.lane_gc("2099-01-01T00:30:00Z", &[], &[]);
    assert_eq!(rig.read("remote.txt"), record);
    rig.lane_gc("2099-01-01T01:00:00Z", &[], &[]);
    assert!(!rig.read("remote.txt").contains("kept"));
}

impl Rig {
    fn stub_origin(&self, open: &[u64]) {
        let rows: Vec<String> = open.iter().map(|number| format!("{{\"number\":{number},\"repository\":{{\"name\":\"north\"}}}}")).collect();
        self.stub("git", "case \"$*\" in *\"remote get-url origin\"*) echo https://github.com/tompassarelli/north.git; exit 0 ;; esac\nexec \"$LANE_GC_REAL_GIT\" \"$@\"\n");
        self.stub(
            "gh",
            &format!(
                "printf '%s\\n' \"$*\" >>\"$LANE_GC_FIXTURE/gh-calls\"\n\
                 case \"$1 $2\" in\n\
                 \"search issues\") echo '[{rows}]' ;;\n\
                 \"api graphql\") case \"$*\" in *refs*) cat \"$LANE_GC_FIXTURE/branches.json\" ;; *) cat \"$LANE_GC_FIXTURE/issues.json\" ;; esac ;;\n\
                 \"api repos/tompassarelli/north\") echo '[{{\"archived\":false}}]' ;;\n\
                 \"issue view\") body=$(sed -e 's/\\\\/\\\\\\\\/g' -e 's/\"/\\\\\"/g' \"$LANE_GC_FIXTURE/issue-$3.txt\" | awk 'BEGIN{{ORS=\"\\\\n\"}}{{print}}'); printf '{{\"body\":\"%s\",\"state\":\"OPEN\"}}\\n' \"$body\" ;;\n\
                 \"issue edit\") cat >\"$LANE_GC_FIXTURE/issue-$3.txt\" ;;\n\
                 \"issue create\") cat >>\"$LANE_GC_FIXTURE/created.txt\"; echo https://github.com/tompassarelli/north/issues/9999 ;;\n\
                 \"api repos/tompassarelli/north/\"*) case \"$2\" in *actions/runs*) echo '[{{\"total_count\":0,\"workflow_runs\":[]}}]' ;; *) echo '[[]]' ;; esac ;;\n\
                 *) exit 1 ;;\n\
                 esac\n",
                rows = rows.join(",")
            ),
        );
    }
}

#[test]
fn the_origin_sweep_abandons_branches_whose_changes_are_on_main_and_files_a_box_for_each_unlanded_one() {
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
    let branches: Vec<Value> = rig.git(&main, &["for-each-ref", "--format=%(refname:short) %(objectname) %(committerdate:iso-strict)", "refs/remotes/origin"]).lines().filter_map(|line| {
        let mut fields = line.split(' ');
        let name = fields.next()?.strip_prefix("origin/")?.to_owned();
        let (oid, date) = (fields.next()?, fields.next()?);
        (name != "HEAD").then(|| json!({"name": name, "target": {"oid": oid, "committedDate": date, "status": null}}))
    }).collect();
    let page = |connection: &str, nodes: Vec<Value>| json!({"data": {"repository": {connection: {"pageInfo": {"hasNextPage": false, "endCursor": null}, "nodes": nodes}}}}).to_string();
    write(&root.join("branches.json"), &page("refs", branches));
    write(&root.join("issues.json"), &page("issues", [892, 842, 903].map(|number| json!({"number": number, "title": "open", "body": "## Done when\n- [ ] the box"})).to_vec()));
    for number in [892, 842, 903] {
        write(&root.join(format!("issue-{number}.txt")), "## Done when\n- [ ] the box\n");
    }
    rig.stub_origin(&[892, 842, 903]);
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

    let body = |number: u64| rig.read(&format!("issue-{number}.txt"));
    for branch in ["scratch/n892-promise", "scratch/n892-expiry", "scratch/n892-solo", "scratch/n892-solo-v2", "scratch/n892-shared-config", "scratch/n892-addressee"] {
        assert!(body(892).contains(&format!("`{branch}`")), "north#892 names no box for {branch}:\n{}\n{out}", body(892));
    }
    assert!(body(842).contains("`q902-bets`") && body(903).contains("`scratch/n903-waits-wip`"), "{}\n{}", body(842), body(903));
    let created = rig.read("created.txt");
    assert!(created.contains("`n706-pin-retire`") && created.contains("`scratch/n882-lines`"), "{created}");
    assert!(out.contains("origin scratch/n882-lines: KEEP unlanded"), "{out}");
}
