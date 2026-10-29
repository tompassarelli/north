use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};

use serde_json::{Value, json};

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
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

impl Rig {
    fn new(root: &Path, home: &Path) -> Self {
        let stubs = root.join("stubs");
        fs::create_dir_all(&stubs).unwrap();
        fs::create_dir_all(home.join("code")).unwrap();
        let config = root.join("gitconfig");
        fs::write(&config, "[user]\n\tname = fixture\n\temail = fixture@example.invalid\n[init]\n\tdefaultBranch = main\n[advice]\n\tdetachedHead = false\n").unwrap();
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
        rig.stub("ssh", "#!/bin/sh\ncat >/dev/null\necho '{\"ok\":true,\"branches\":{}}'\n");
        rig.stub("threads", "#!/bin/sh\nprintf 'item\\tholder\\n'\n");
        rig.stub("supply-alert", "#!/bin/sh\n[ ! -e \"$LANE_GC_FIXTURE/ping-fault\" ] || exit 1\nfor arg; do printf '%s|' \"$arg\"; done >>\"$LANE_GC_FIXTURE/alerts\"\necho >>\"$LANE_GC_FIXTURE/alerts\"\n");
        rig.stub(
            "north",
            "#!/bin/sh\ncase \"$1 $2\" in\n\"fleet list\") cat \"$FLEET_TEST_FILE\" ;;\n\"fleet send\") [ ! -e \"$LANE_GC_FIXTURE/ping-fault\" ] || exit 1; printf '%s\\n' \"$*\" >>\"$FLEET_TEST_SENDS\" ;;\n*) exit 1 ;;\nesac\n",
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

fn real_git() -> String {
    let output = Command::new("sh").args(["-c", "command -v git"]).output().unwrap();
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn write(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
}

const PUSH_WRAPPER: &str = "#!/bin/sh\ncase \" $* \" in *\" remote get-url origin \"*) echo https://github.com/tompassarelli/north.git; exit 0 ;; esac\ncase \" $* \" in *\" push \"*)\n  printf '%s\\n' \"$*\" >>\"$LANE_GC_FIXTURE/pushes\"\n  for arg; do case \"$arg\" in :refs/heads/*) exit 0 ;; esac; done ;;\nesac\nexec \"$LANE_GC_REAL_GIT\" \"$@\"\n";

const GH_REPLAY: &str = "#!/bin/sh\ndir=$LANE_GC_FIXTURE/gh\nfault=$(cat \"$LANE_GC_FIXTURE/api-fault\" 2>/dev/null)\n[ \"$fault\" = error ] && exit 1\ncase \"$2\" in\n  graphql) name=graphql ;;\n  *activity\\?*) name=activity ;;\n  */pulls\\?*) name=pulls ;;\n  */actions/runs\\?*) status=${2#*status=}; name=runs-${status%%&*} ;;\n  *) exit 1 ;;\nesac\n[ -n \"$fault\" ] && [ -e \"$dir/${name%%-*}.$fault.json\" ] && name=${name%%-*}.$fault\ncat \"$dir/$name.json\"\n";

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
fn an_orphan_landing_ref_is_pinged_held_through_grace_and_archived_to_the_mirror_before_deletion() {
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
    rig.git(&main, &["worktree", "add", "-q", rig.home.join("code/north/worktrees/author").to_str().unwrap(), "author"]);
    rig.stub("git", PUSH_WRAPPER);
    rig.stub("gh", GH_REPLAY);
    let branches = landing_facts(&rig, &main);
    let pushed = || rig.root.join("pushes").exists();

    let dry = rig.lane_gc("2026-10-14T11:00:00Z", &[], &["--repo", "north", "--dry-run"]);
    assert!(!pushed() && !rig.root.join("alerts").exists(), "{dry}");
    assert!(dry.contains("would notify fixture-parent"), "{dry}");
    for lane in ["land-fresh-", "land-force-fresh-"] {
        assert!(dry.lines().any(|line| line.contains(lane) && line.contains("LIVE")), "{lane}: {dry}");
    }
    write(&rig.root.join("ping-fault"), "");
    let refused = rig.lane_gc("2026-10-14T11:00:00Z", &[], &["--repo", "north"]);
    assert!(!pushed() && !rig.root.join("alerts").exists() && refused.contains("Fleet parent notification failed"), "{refused}");
    fs::remove_file(rig.root.join("ping-fault")).unwrap();
    let pinged = rig.lane_gc("2026-10-14T11:00:00Z", &[], &["--repo", "north"]);
    assert!(!pushed(), "{pinged}");
    assert!(rig.read("fleet-sends").lines().any(|line| line.starts_with("fleet send fixture-parent ") && line.contains("orphan landing")));
    let grace = rig.lane_gc("2026-10-14T11:30:00Z", &[], &["--repo", "north"]);
    assert!(!pushed() && grace.contains("owner ping grace"), "{grace}");
    for fault in ["error", "malformed", "refs-incomplete", "runs-capped"] {
        write(&rig.root.join("api-fault"), fault);
        let out = rig.lane_gc("2026-10-14T12:01:00Z", &[], &["--repo", "north"]);
        assert!(!pushed(), "{fault}: {out}");
    }
    fs::remove_file(rig.root.join("api-fault")).unwrap();
    let recovered = rig.lane_gc("2026-10-14T12:01:00Z", &[], &["--repo", "north"]);
    assert!(!pushed() && recovered.contains("notified fixture-parent"), "{recovered}");
    let regrace = rig.lane_gc("2026-10-14T13:00:00Z", &[], &["--repo", "north"]);
    assert!(!pushed() && regrace.contains("owner ping grace"), "{regrace}");
    assert!(regrace.lines().any(|line| line.contains("land-fresh-") && line.contains("notified fixture-parent")), "{regrace}");
    let archived = rig.lane_gc("2026-10-14T13:01:00Z", &[], &["--repo", "north"]);
    assert!(!rig.read("pushes").contains(":refs/heads/claude/land-fresh-"), "{archived}");
    let created = rig.lane_gc("2026-10-14T14:00:00Z", &[], &["--repo", "north"]);

    let pushes: Vec<String> = rig.read("pushes").lines().map(str::to_string).collect();
    for (name, tip) in &branches {
        let deletion = pushes.iter().position(|line| line.contains(&format!(":refs/heads/{name}")));
        let eligible = ["orphan", "pending", "fresh"].iter().any(|lane| name.contains(&format!("/land-{lane}-")));
        assert_eq!(deletion.is_some(), eligible, "{name}: {pushes:?}\n{created}");
        let Some(deletion) = deletion else { continue };
        let reference = format!("refs/archive/2026-10-14/{name}");
        let copied = pushes.iter().position(|line| line.contains(&format!("{tip}:{reference}")));
        assert!(copied.is_some_and(|copied| copied < deletion), "{name}: {pushes:?}");
        assert_eq!(rig.git(&rig.root.join("mirrors/north.git"), &["rev-parse", &format!("{reference}^{{commit}}")]), *tip);
        assert!(!rig.git_ok(&origin, &["rev-parse", "-q", "--verify", &format!("refs/tags/archive/north/{name}/{}", &tip[..12])]), "archive tags stay off origin");
        assert!(pushes[deletion].contains(&format!("--force-with-lease=refs/heads/{name}:{tip}")), "{}", pushes[deletion]);
    }
}

const ARCHIVE_FAULT_WRAPPER: &str = "#!/bin/sh\nfor last; do :; done\ncase \" $* \" in *\" ls-remote origin refs/tags/archive/\"*)\n  if [ -f \"$LANE_GC_FIXTURE/remote-fault\" ]; then\n    case $(cat \"$LANE_GC_FIXTURE/remote-fault\") in\n      missing) exit 0 ;;\n      unavailable) exit 1 ;;\n      mismatch) printf '%s\\t%s\\n' 0000000000000000000000000000000000000000 \"$last\"; exit 0 ;;\n    esac\n  fi ;;\nesac\nexec \"$LANE_GC_REAL_GIT\" \"$@\"\n";

struct Classified {
    rig: Rig,
    main: PathBuf,
    origin: PathBuf,
    tips: [String; 3],
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

    fn new(root: &Path) -> Self {
        let rig = Rig::new(root, &root.join("home"));
        rig.fleet(&[("north-parent", None, "north"), ("north-worker", Some("north-parent"), "north"), ("scratch-parent", None, "galileo"), ("scratch-worker", Some("scratch-parent"), "galileo")]);
        rig.stub("gh", "#!/bin/sh\ncase \"$*\" in\n\"issue view 5 -R tompassarelli/north --json state -q .state\") echo CLOSED ;;\n\"issue view 6 -R tompassarelli/north --json state -q .state\") echo OPEN ;;\n*) exit 1 ;;\nesac\n");
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
        let holder = Holder(Command::new("sleep").arg("300").current_dir(&held).spawn().unwrap());
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
        for i in 1..=26 {
            rig.git(&galileo, &["branch", &format!("lane-{i}"), "unique"]);
        }
        rig.git(&galileo, &["branch", "-q", "-D", "unique"]);
        let legacy = rig.git(&main, &["rev-parse", "equiv"]);
        let legacy_tag = format!("archive/north/equiv/{}", &legacy[..12]);
        rig.git(&main, &["tag", &legacy_tag, &legacy]);
        rig.git(&main, &["push", "-q", "origin", &format!("refs/tags/{legacy_tag}")]);
        rig.stub("git", ARCHIVE_FAULT_WRAPPER);
        let tips = ["equiv", "sup", "wip"].map(|b| rig.git(&main, &["rev-parse", b]));
        let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() + 30 * 3600;
        let now = String::from_utf8(Command::new("date").args(["-u", "-d", &format!("@{at}"), "+%FT%TZ"]).output().unwrap().stdout).unwrap().trim().to_string();
        let origin = rig.root.join("origin/north.git");
        Self { rig, main, origin, tips, legacy, now, _holder: holder }
    }

    fn run(&self, args: &[&str]) -> String {
        let stamp = String::from_utf8(Command::new("date").args(["-u", "+%FT%TZ"]).output().unwrap().stdout).unwrap();
        let stamp = stamp.trim();
        self.rig.lane_gc(&self.now, &[("GIT_AUTHOR_DATE", stamp), ("GIT_COMMITTER_DATE", stamp)], args)
    }

    fn lanes_intact(&self) -> bool {
        ["equiv", "sup", "wip", "own", "held"]
            .iter()
            .all(|b| self.rig.home.join("code/north/worktrees").join(b).is_dir() && self.rig.git_ok(&self.main, &["rev-parse", "-q", "--verify", &format!("refs/heads/{b}")]))
    }

    fn fault(&self, fault: Option<&str>) {
        let path = self.rig.root.join("remote-fault");
        match fault {
            Some(fault) => write(&path, fault),
            None => fs::remove_file(path).unwrap(),
        }
    }

    fn remote_tag(&self, lane: &str) -> String {
        let listed = self.rig.git(&self.main, &["ls-remote", "origin", &format!("refs/tags/archive/north/{lane}/*")]);
        let rows: Vec<(&str, &str)> = listed.lines().filter_map(|line| line.split_once('\t')).collect();
        rows.iter().find(|(_, name)| name.ends_with("^{}")).or(rows.first()).map(|(oid, _)| oid.to_string()).unwrap_or_default()
    }

    fn origin_has_sup(&self) -> bool {
        !self.rig.git(&self.main, &["ls-remote", "origin", "refs/heads/sup"]).is_empty()
    }

    fn tagged_gone(&self, lane: &str, tip: &str) -> bool {
        !self.rig.home.join("code/north/worktrees").join(lane).exists()
            && !self.rig.git_ok(&self.main, &["rev-parse", "-q", "--verify", &format!("refs/heads/{lane}")])
            && self.remote_tag(lane) == tip
            && self.rig.git(&self.main, &["rev-parse", "-q", "--verify", &format!("refs/tags/archive/north/{lane}/{}^{{commit}}", &tip[..12])]) == tip
    }
}

#[test]
fn a_lane_whose_archive_tag_github_declines_for_email_privacy_retires_to_the_mirror() {
    let scratch = tempfile::tempdir().unwrap();
    let fixture = Classified::new(scratch.path());
    let (rig, main) = (&fixture.rig, &fixture.main);
    executable(&fixture.origin.join("hooks/pre-receive"), "#!/bin/sh\nwhile read -r old new ref; do case \"$ref\" in refs/tags/*) echo 'push declined due to email privacy restrictions' >&2; exit 1 ;; esac; done\n");
    let out = fixture.run(&[]);
    let sup = &fixture.tips[1];
    assert!(!rig.home.join("code/north/worktrees/sup").exists() && !rig.git_ok(main, &["rev-parse", "-q", "--verify", "refs/heads/sup"]), "{out}");
    assert!(fixture.remote_tag("sup").is_empty() && !fixture.origin_has_sup(), "{out}");
    let mirrored = rig.git(&rig.root.join("mirrors/north.git"), &["for-each-ref", "--format=%(objectname) %(refname)", "refs/archive/"]);
    assert!(mirrored.lines().any(|line| line.starts_with(sup.as_str()) && line.ends_with("/sup")), "{mirrored}\n{out}");
}

#[test]
fn each_lane_class_retires_behind_a_verified_archive_tag_and_owned_work_stays_reported() {
    let scratch = tempfile::tempdir().unwrap();
    let fixture = Classified::new(scratch.path());
    let (rig, main) = (&fixture.rig, &fixture.main);

    let dry = fixture.run(&["--dry-run"]);
    for expected in [
        "north: landed-elsewhere ~/code/north/worktrees/equiv: archive to archive/north/equiv/",
        "north: superseded ~/code/north/worktrees/sup: archive to archive/north/sup/",
        "north: stale-dirty ~/code/north/worktrees/wip: archive to archive/north/wip/<snapshot>, then retire",
        "north: owned ~/code/north/worktrees/own: surface to north-parent (north#5 north#6)",
        "north: owned ~/code/north/worktrees/held: surface to north-parent",
        "would alert scratch-parent: 26 owned unlanded lanes",
    ] {
        assert!(dry.contains(expected), "{expected}\n{dry}");
    }
    assert!(dry.lines().any(|line| line == "north: owned branch loose: surface to north-parent"), "{dry}");
    assert!(fixture.lanes_intact() && !rig.root.join("alerts").exists());
    assert_eq!(rig.git(main, &["tag", "-l", "archive/*"]), format!("archive/north/equiv/{}", &fixture.legacy[..12]));

    let hook = fixture.origin.join("hooks/pre-receive");
    executable(&hook, "#!/bin/sh\nwhile read -r old new ref; do case \"$ref\" in refs/tags/*) exit 1 ;; esac; done\n");
    fixture.fault(Some("missing"));
    let sup_short = rig.git(main, &["rev-parse", "--short=12", "sup"]);
    let conflict = format!("archive/north/sup/{sup_short}");
    rig.git(main, &["tag", &conflict, "main"]);
    let out = fixture.run(&[]);
    assert!(fixture.lanes_intact() && rig.git(main, &["rev-parse", &conflict]) == rig.git(main, &["rev-parse", "main"]), "{out}");
    assert!(out.contains("kept (archive tag conflict)"), "{out}");
    rig.git(main, &["tag", "-d", &conflict]);
    fixture.fault(None);

    fixture.fault(Some("missing"));
    let out = fixture.run(&[]);
    let refused = Command::new(real_git()).args(["-C", main.to_str().unwrap(), "push", "-q", "origin", &format!("refs/tags/{conflict}:refs/tags/{conflict}")]).envs(rig.env.iter().filter(|(k, _)| k != "PATH").cloned()).output().unwrap();
    assert!(!refused.status.success());
    let push_error = String::from_utf8_lossy(&refused.stderr).lines().rfind(|line| !line.starts_with("error: failed to push some refs")).unwrap_or_default().to_string();
    assert!(push_error.contains("pre-receive hook declined"), "{push_error}");
    let reason = format!("archive tag push unverified: {push_error}");
    assert!(fixture.lanes_intact() && fixture.origin_has_sup(), "{out}");
    let origin_tags = rig.git(main, &["ls-remote", "origin", "refs/tags/*"]);
    assert_eq!(origin_tags.lines().map(|line| line.split('\t').next().unwrap()).collect::<Vec<_>>(), [fixture.legacy.as_str()], "{origin_tags}");
    assert!(out.contains(&format!("kept ({reason})")), "{reason}\n{out}");
    assert!(rig.read("notices.md").contains(&reason));
    let stale: Value = serde_json::from_str(&rig.read("stale.json")).unwrap();
    assert!(stale["notices"].as_array().unwrap().iter().any(|line| line.as_str().unwrap().contains(&reason)));
    rig.git(main, &["tag", "-f", "-a", &conflict, "-m", "recorded superseded archive", "sup"]);
    let annotated = rig.git(main, &["rev-parse", &conflict]);
    fs::remove_file(hook).unwrap();

    for fault in ["missing", "mismatch", "unavailable"] {
        fixture.fault(Some(fault));
        let out = fixture.run(&[]);
        assert!(fixture.lanes_intact() && fixture.origin_has_sup() && out.contains("kept (archive tag push unverified)"), "{fault}\n{out}");
    }
    fixture.fault(None);
    let out = fixture.run(&[]);
    let [equiv, sup, wip] = &fixture.tips;
    assert!(fixture.tagged_gone("equiv", equiv) && fixture.tagged_gone("sup", sup), "{out}");
    assert_eq!(rig.git(main, &["rev-parse", &conflict]), annotated);
    assert!(rig.git(main, &["ls-remote", "origin", &format!("refs/tags/{conflict}")]).starts_with(&annotated));
    assert!(!fixture.origin_has_sup());

    let snapshot = fixture.remote_tag("wip");
    assert!(!snapshot.is_empty() && !rig.home.join("code/north/worktrees/wip").exists(), "{out}");
    assert_eq!(rig.git(main, &["rev-parse", &format!("{snapshot}^")]), *wip);
    assert_eq!(rig.git(main, &["show", &format!("{snapshot}:new.txt")]), "new");
    assert!(rig.git(main, &["show", &format!("{snapshot}:README")]).contains("edited"));
    assert!(!rig.git_ok(main, &["cat-file", "-e", &format!("{snapshot}:build.log")]));

    assert!(rig.home.join("code/north/worktrees/own").is_dir() && rig.home.join("code/north/worktrees/held").is_dir());
    assert!(rig.git_ok(main, &["rev-parse", "-q", "--verify", "refs/heads/loose"]));
    let mut unlanded: Vec<String> = rig.read("unlanded.txt").lines().filter(|line| !line.starts_with('#')).map(|line| line.split('\t').collect::<Vec<_>>()).filter(|fields| fields[1] == "north").map(|fields| fields[3].to_string()).collect();
    unlanded.sort();
    assert_eq!(unlanded, ["held", "loose", "own"]);
    let stale: Value = serde_json::from_str(&rig.read("stale.json")).unwrap();
    let mut owned: Vec<(String, Vec<String>, String)> = stale["repos"]["north"]["owned"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| (row["branch"].as_str().unwrap().into(), row["refs"].as_array().unwrap().iter().map(|r| r.as_str().unwrap().into()).collect(), row["owner"].as_str().unwrap().into()))
        .collect();
    owned.sort();
    let parent = "north-parent".to_string();
    assert_eq!(owned, [("held".into(), vec![], parent.clone()), ("loose".into(), vec![], parent.clone()), ("own".into(), vec!["north#5".into(), "north#6".into()], parent)]);
    let alerts = rig.read("alerts");
    assert_eq!(alerts.lines().count(), 1, "{alerts}");
    assert!(alerts.starts_with("normal|scratch-parent: 26 owned unlanded lanes"), "{alerts}");
}
