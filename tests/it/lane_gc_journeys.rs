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
            "case \"$1 $2\" in\n\"fleet list\") cat \"$FLEET_TEST_FILE\" ;;\n\"threads \"*) printf 'item\\tholder\\n' ;;\n\"fleet send\") [ ! -e \"$LANE_GC_FIXTURE/ping-fault\" ] || exit 1; printf '%s\\n' \"$*\" >>\"$FLEET_TEST_SENDS\" ;;\n*) exit 1 ;;\nesac\n",
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
        let output = Command::new(env!("CARGO_BIN_EXE_north"))
            .arg("lane-gc")
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

const PUSH_WRAPPER: &str = "case \" $* \" in *\" remote get-url origin \"*) echo https://github.com/tompassarelli/north.git; exit 0 ;; esac\ncase \" $* \" in *\" push \"*)\n  printf '%s\\n' \"$*\" >>\"$LANE_GC_FIXTURE/pushes\"\n  for arg; do case \"$arg\" in :refs/heads/*) exit 0 ;; esac; done ;;\nesac\nexec \"$LANE_GC_REAL_GIT\" \"$@\"\n";

const GH_REPLAY: &str = "dir=$LANE_GC_FIXTURE/gh\nfault=$(cat \"$LANE_GC_FIXTURE/api-fault\" 2>/dev/null)\n[ \"$fault\" = error ] && exit 1\ncase \"$2\" in\n  graphql) name=graphql ;;\n  *activity\\?*) name=activity ;;\n  */pulls\\?*) name=pulls ;;\n  */actions/runs\\?*) status=${2#*status=}; name=runs-${status%%&*} ;;\n  *) exit 1 ;;\nesac\n[ -n \"$fault\" ] && [ -e \"$dir/${name%%-*}.$fault.json\" ] && name=${name%%-*}.$fault\ncat \"$dir/$name.json\"\n";

fn landing_facts(rig: &Rig, main: &Path) -> Vec<(String, String)> {
    let listed = rig.git(main, &["for-each-ref", "--format=%(refname:short) %(objectname)", "refs/heads/claude/"]);
    let branches: Vec<(String, String)> = listed.lines().map(|line| line.split_once(' ').map(|(n, t)| (n.to_string(), t.to_string())).unwrap()).collect();
    let gh = rig.root.join("gh");
    fs::create_dir_all(&gh).unwrap();
    let nodes: Vec<Value> = branches
        .iter()
        .map(|(name, tip)| json!({"name": name, "target": {"oid": tip, "committedDate": "2026-10-14T12:00:00Z", "status": {"contexts": [{"context": "autoland", "state": "PENDING"}]}}}))
        .collect();
    let page = |more: bool| json!([{"data": {"repository": {"refs": {"nodes": nodes, "pageInfo": {"hasNextPage": more}}}}}]);
    write(&gh.join("graphql.json"), &page(false).to_string());
    write(&gh.join("graphql.refs-incomplete.json"), &page(true).to_string());
    let mut activity: Vec<Value> = branches
        .iter()
        .map(|(name, tip)| {
            let created = name.contains("/land-fresh-") || name.contains("/land-force-fresh-");
            let at = if name.contains("/land-fresh-") { "2026-10-14T09:00:00Z" } else { "2026-10-14T06:00:00Z" };
            json!({"ref": format!("refs/heads/{name}"), "activity_type": if created { "branch_creation" } else { "push" }, "timestamp": at, "before": "0".repeat(40), "after": tip})
        })
        .collect();
    let find = |lane: &str| branches.iter().find(|(name, _)| name.contains(&format!("/land-{lane}-"))).unwrap().clone();
    let (forced, forced_tip) = find("force-fresh");
    activity.push(json!({"ref": format!("refs/heads/{forced}"), "activity_type": "force_push", "timestamp": "2026-10-14T10:30:00Z", "before": forced_tip, "after": forced_tip}));
    let (orphan, orphan_tip) = find("orphan");
    activity.push(json!({"ref": format!("refs/heads/{orphan}"), "activity_type": "branch_deletion", "timestamp": "2026-10-14T10:59:00Z", "before": orphan_tip, "after": "0".repeat(40)}));
    write(&gh.join("activity.json"), &json!([activity[..3], activity[3..]]).to_string());
    write(&gh.join("activity.malformed.json"), &json!([[{"ref": "refs/heads/bad"}]]).to_string());
    write(&gh.join("pulls.json"), "[[]]");
    for status in ["queued", "in_progress", "waiting", "pending", "requested"] {
        let lane = match status {
            "queued" => Some("queued"),
            "in_progress" => Some("running"),
            "waiting" => Some("head-live"),
            _ => None,
        };
        let runs: Vec<Value> = lane.map(find).map(|(name, tip)| json!({"head_branch": if status == "waiting" { "train".to_string() } else { name }, "head_sha": tip})).into_iter().collect();
        write(&gh.join(format!("runs-{status}.json")), &json!([{"workflow_runs": runs, "total_count": runs.len()}]).to_string());
    }
    write(&gh.join("runs.runs-capped.json"), &json!([{"workflow_runs": [], "total_count": 1001}]).to_string());
    branches
}

#[test]
fn unlanded_landing_refs_are_kept_unpinged_and_landed_ones_are_deleted() {
    let scratch = tempfile::tempdir().unwrap();
    let rig = Rig::new(scratch.path(), &scratch.path().join("home"));
    rig.fleet(&[("fixture-parent", None, "north")]);
    let origin = rig.root.join("origin.git");
    let main = rig.home.join("code/north/main");
    fs::create_dir_all(&origin).unwrap();
    fs::create_dir_all(&main).unwrap();
    fs::create_dir_all(rig.home.join("code/north/worktrees")).unwrap();
    rig.git(&origin, &["init", "-q", "--bare"]);
    rig.git(&main, &["init", "-q", "-b", "main"]);
    write(&main.join("readme"), "main\n");
    rig.git(&main, &["add", "readme"]);
    rig.git(&main, &["commit", "-qm", "main"]);
    rig.git(&main, &["remote", "add", "origin", origin.to_str().unwrap()]);
    rig.git(&main, &["push", "-q", "origin", "main"]);
    for lane in ["orphan", "fresh", "force-fresh", "queued", "running", "head-live", "author", "pending"] {
        rig.git(&main, &["checkout", "-q", "-b", lane, "main"]);
        write(&main.join(lane), &format!("{lane}\n"));
        rig.git(&main, &["add", lane]);
        rig.git(&main, &["commit", "-qm", lane]);
        let tip = rig.git(&main, &["rev-parse", "HEAD"]);
        rig.git(&main, &["branch", &format!("claude/land-{lane}-{}", &tip[..12]), &tip]);
    }
    rig.git(&main, &["checkout", "-q", "main"]);
    let main_tip = rig.git(&main, &["rev-parse", "HEAD"]);
    rig.git(&main, &["branch", &format!("claude/land-landed-{}", &main_tip[..12]), &main_tip]);
    rig.git(&main, &["worktree", "add", "-q", rig.home.join("code/north/worktrees/author").to_str().unwrap(), "author"]);
    rig.stub("git", PUSH_WRAPPER);
    rig.stub("gh", GH_REPLAY);
    let branches = landing_facts(&rig, &main);
    let pushed = || rig.root.join("pushes").exists();

    let dry = rig.lane_gc("2026-10-14T11:00:00Z", &[], &["--repo", "north", "--dry-run"]);
    assert!(!pushed() && !rig.root.join("alerts").exists() && !dry.contains("would notify"), "{dry}");
    for lane in ["land-fresh-", "land-force-fresh-"] {
        assert!(dry.lines().any(|line| line.contains(lane) && line.contains("LIVE")), "{lane}: {dry}");
    }
    assert!(dry.lines().any(|line| line.contains("land-landed-") && line.contains("STALE landed")), "{dry}");
    for fault in ["error", "malformed", "refs-incomplete", "runs-capped"] {
        write(&rig.root.join("api-fault"), fault);
        let out = rig.lane_gc("2026-10-14T14:00:00Z", &[], &["--repo", "north"]);
        assert!(!pushed(), "{fault}: {out}");
    }
    fs::remove_file(rig.root.join("api-fault")).unwrap();
    let out = rig.lane_gc("2026-10-14T14:00:00Z", &[], &["--repo", "north"]);

    let pushes: Vec<String> = rig.read("pushes").lines().map(str::to_string).collect();
    let (landed, landed_tip) = branches.iter().find(|(name, _)| name.contains("/land-landed-")).unwrap();
    assert_eq!(pushes.len(), 1, "{pushes:?}\n{out}");
    assert!(pushes[0].contains(&format!("--force-with-lease=refs/heads/{landed}:{landed_tip}")) && pushes[0].contains(&format!(":refs/heads/{landed}")), "{}", pushes[0]);
    assert!(!rig.root.join("fleet-sends").exists(), "unlanded landing refs are not pinged");
    for lane in ["orphan", "pending", "queued", "running"] {
        assert!(out.lines().any(|line| line.contains(&format!("/land-{lane}-")) && (line.contains("KEEP unlanded") || line.contains("LIVE"))), "{lane}: {out}");
    }
    assert!(out.lines().any(|line| line.contains("/land-orphan-") && line.contains("fault: names no issue")), "{out}");
    assert!(!rig.read("notices.md").contains("archive"), "no archive notice");
}

static CLASSIFIED: LazyLock<Template> = LazyLock::new(|| Template::build(Classified::build));

struct Classified {
    rig: Rig,
    main: PathBuf,
    origin: PathBuf,
    legacy: String,
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
        rig.stub("gh", "case \"$*\" in\n\"issue view 5 -R tompassarelli/north --json state -q .state\") echo CLOSED ;;\n\"issue view 6 -R tompassarelli/north --json state -q .state\") echo OPEN ;;\n*) exit 1 ;;\nesac\n");
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
        let legacy = rig.git(&main, &["rev-parse", "equiv"]);
        let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() + 30 * 3600;
        let now = String::from_utf8(Command::new("date").args(["-u", "-d", &format!("@{at}"), "+%FT%TZ"]).output().unwrap().stdout).unwrap().trim().to_string();
        let origin = rig.root.join("origin/north.git");
        Self { rig, main, origin, legacy, now, _holder: holder }
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

    fn origin_extra_refs(&self) -> Vec<String> {
        let listed = self.rig.git(&self.main, &["ls-remote", "origin", "refs/tags/*", "refs/archive/*"]);
        listed.lines().map(|line| line.split('\t').next().unwrap().to_string()).collect()
    }

    fn gone(&self, lane: &str) -> bool {
        !self.rig.home.join("code/north/worktrees").join(lane).exists() && !self.rig.git_ok(&self.main, &["rev-parse", "-q", "--verify", &format!("refs/heads/{lane}")])
    }
}

#[test]
fn a_landed_or_superseded_lane_is_deleted_with_no_backup_and_no_ref_pushed_to_origin() {
    let scratch = tempfile::tempdir().unwrap();
    let fixture = Classified::new(scratch.path());
    let (rig, main) = (&fixture.rig, &fixture.main);
    executable(&fixture.origin.join("hooks/pre-receive"), "while read -r old new ref; do case \"$ref\" in refs/tags/*|refs/archive/*) echo \"archive ref pushed: $ref\" >&2; exit 1 ;; esac; done\n");
    let before = fixture.origin_extra_refs();
    assert_eq!(before, [fixture.legacy.as_str()]);
    let out = fixture.run(&[]);
    assert!(fixture.gone("equiv") && fixture.gone("sup"), "{out}");
    assert!(fixture.kept_intact(), "{out}");
    assert_eq!(fixture.origin_extra_refs(), before, "{out}");
    assert!(!rig.root.join("mirrors").exists() && !rig.read("notices.md").contains("restore="), "{out}");
    assert!(rig.git(main, &["tag", "-l", "archive/*"]).lines().all(|tag| tag.ends_with(&fixture.legacy[..12])), "{out}");
}

#[test]
fn unlanded_lanes_stay_whatever_their_age_or_dirt_and_are_reported_with_issue_state() {
    let scratch = tempfile::tempdir().unwrap();
    let fixture = Classified::new(scratch.path());
    let rig = &fixture.rig;

    let dry = fixture.run(&["--dry-run"]);
    for expected in [
        "north: retire ~/code/north/worktrees/equiv (equiv, ",
        "north: retire ~/code/north/worktrees/sup (sup, issues closed)",
        "north: stale-dirty ~/code/north/worktrees/wip: surface to north-parent",
        "north: owned ~/code/north/worktrees/own: surface to north-parent (north#5 north#6)",
        "north: owned ~/code/north/worktrees/held: surface to north-parent",
        "would alert scratch-parent: 26 owned unlanded lanes",
    ] {
        assert!(dry.contains(expected), "{expected}\n{dry}");
    }
    assert!(dry.lines().any(|line| line == "north: owned branch loose: surface to north-parent"), "{dry}");
    assert!(fixture.kept_intact() && !fixture.gone("equiv") && !rig.root.join("alerts").exists());

    let out = fixture.run(&[]);
    assert!(fixture.gone("equiv") && fixture.gone("sup") && fixture.kept_intact(), "{out}");
    let wip = rig.home.join("code/north/worktrees/wip");
    assert!(wip.join("new.txt").is_file() && fs::read_to_string(wip.join("README")).unwrap().contains("edited"));

    let mut unlanded: Vec<String> = rig.read("unlanded.txt").lines().filter(|line| !line.starts_with('#')).map(|line| line.split('\t').collect::<Vec<_>>()).filter(|fields| fields[1] == "north").map(|fields| fields[3].to_string()).collect();
    unlanded.sort();
    assert_eq!(unlanded, ["held", "loose", "own", "wip"]);
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
        [("held".into(), fault(), parent.clone()), ("loose".into(), fault(), parent.clone()), ("own".into(), vec!["north#5=closed".into(), "north#6=open".into()], parent.clone()), ("wip".into(), fault(), parent)]
    );
    let alerts = rig.read("alerts");
    assert_eq!(alerts.lines().count(), 1, "{alerts}");
    assert!(alerts.starts_with("normal|scratch-parent: 26 owned unlanded lanes"), "{alerts}");
}
