use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, SystemTime};

use north_base::platform::git;
use super::reap::{Counts, Sweep, disposable};
use north_base::test_inputs::git::{git_in, init_repo};

const AGED: Duration = Duration::from_secs(3 * 3600);

pub(super) struct Container {
    root: tempfile::TempDir,
}

impl Container {
    pub(super) fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let container = Self { root };
        init_repo(&container.main(), None);
        fs::write(container.main().join(".gitignore"), "docs/private/\n.direnv/\nresult\nresult-*\n").unwrap();
        git_in(&container.main(), &["add", ".gitignore"]);
        git_in(&container.main(), &["commit", "-qm", "ignores"]);
        container
    }

    pub(super) fn path(&self) -> PathBuf {
        self.root.path().canonicalize().unwrap()
    }

    pub(super) fn main(&self) -> PathBuf {
        self.root.path().canonicalize().unwrap().join("main")
    }

    pub(super) fn lane(&self, name: &str) -> PathBuf {
        self.path().join("worktrees").join(name)
    }

    pub(super) fn add(&self, name: &str, aged: bool) -> PathBuf {
        let lane = self.lane(name);
        git_in(&self.main(), &["worktree", "add", "-q", "-b", name, lane.to_str().unwrap(), "main"]);
        if aged {
            age(&lane);
        }
        lane
    }

    pub(super) fn commit_in(&self, lane: &Path, file: &str) {
        fs::write(lane.join(file), format!("{file} payload\n")).unwrap();
        git_in(lane, &["add", file]);
        git_in(lane, &["commit", "-qm", &format!("{file} work")]);
    }

    pub(super) fn squash_land(&self, branch: &str, message: &str) {
        git_in(&self.main(), &["merge", "-q", "--squash", branch]);
        git_in(&self.main(), &["commit", "-qm", message]);
    }

    fn land_equivalent(&self, branch: &str) {
        let sha = git_in(&self.main(), &["rev-parse", branch]);
        let log = self.main().join("log.txt");
        let previous = fs::read_to_string(&log).unwrap_or_default();
        fs::write(&log, format!("{previous}{branch} landed\n")).unwrap();
        git_in(&self.main(), &["add", "log.txt"]);
        git_in(&self.main(), &["commit", "-qm", &format!("main advances before {branch} lands")]);
        git_in(&self.main(), &["cherry-pick", &sha]);
    }

    fn sweep(&self, dry_run: bool) -> (Vec<String>, Counts) {
        let sweep = Sweep { dry_run, minimum_age: 7200, only: None, now: SystemTime::now(), proc_root: PathBuf::from("/proc"), state: self.path().join("state") };
        let (report, failure) = sweep.reap_containers(&[self.path()]);
        assert_eq!(failure, None);
        (report.lines, report.counts)
    }
}

fn age(lane: &Path) {
    fs::File::options().write(true).open(lane.join(".git")).unwrap().set_modified(SystemTime::now() - AGED).unwrap();
}

fn evil_merge_lane(container: &Container) -> PathBuf {
    let lane = container.add("merge-lane", false);
    git_in(&lane, &["checkout", "-q", "-b", "merge-side"]);
    fs::write(lane.join("f.txt"), "side version\n").unwrap();
    git_in(&lane, &["commit", "-qam", "side work"]);
    git_in(&lane, &["checkout", "-q", "merge-lane"]);
    fs::write(lane.join("merge-lane.txt"), "merge-lane payload\n").unwrap();
    fs::write(lane.join("f.txt"), "lane version\n").unwrap();
    git_in(&lane, &["add", "merge-lane.txt", "f.txt"]);
    git_in(&lane, &["commit", "-qm", "merge-lane work"]);
    container.land_equivalent("merge-lane");
    fs::write(container.main().join("f.txt"), "base\n").unwrap();
    git_in(&container.main(), &["commit", "-qam", "main restores f.txt"]);
    git_in(&container.main(), &["cherry-pick", "merge-side"]);
    let identity = ["-c", "user.name=lanes-test", "-c", "user.email=lanes-test@example.invalid"];
    let conflict = git::run(&lane, &[identity.as_slice(), &["merge", "--no-ff", "--no-edit", "merge-side"]].concat()).unwrap();
    assert!(!conflict.status.success(), "the side branch must conflict with the lane");
    fs::write(lane.join("f.txt"), "evil resolution found nowhere else\n").unwrap();
    git_in(&lane, &["add", "f.txt"]);
    git_in(&lane, &["commit", "-qm", "merge merge-side (evil resolution)"]);
    let cherry = git_in(&container.main(), &["cherry", "refs/heads/main", "refs/heads/merge-lane"]);
    assert!(!cherry.lines().any(|line| line.starts_with('+')), "only the merge gate may keep this lane: {cherry}");
    assert!(!git_in(&container.main(), &["rev-list", "--merges", "refs/heads/main..refs/heads/merge-lane"]).is_empty());
    lane
}

fn branch_exists(container: &Container, name: &str) -> bool {
    Command::new("git").current_dir(container.main()).args(["rev-parse", "--verify", "-q", &format!("refs/heads/{name}")]).output().unwrap().status.success()
}

#[test]
fn a_lane_whose_only_ignored_files_are_regenerable_caches_is_bundled() {
    let regenerable = [
        ("target/debug/north", "cargo build output"),
        ("crates/app/target/release/app", "nested cargo build output"),
        (".direnv/flake/profile", "direnv cache"),
        (".firn-cache/index", "firn cache"),
        ("buck-out/v2/gen/app", "buck build output"),
        (".buckconfig.flake", "buck flake config"),
        ("result", "nix build result link"),
        ("result-dev", "nix build result link"),
        ("tools/__pycache__/gen.cpython-312.pyc", "python bytecode cache"),
        ("web/node_modules/left-pad/index.js", "npm dependencies"),
        ("build/app.tsbuildinfo", "typescript incremental build info"),
        ("build/number-rules.json", "generated number rules"),
        (".safe-push-deps", "safe-push dependency cache"),
        (".safe-push-deps/lock.json", "safe-push dependency cache"),
        ("node_modules", "bare npm dependencies entry"),
        ("ts/build/app.tsbuildinfo", "typescript build info below the lane root"),
        ("examples/sample/tsconfig.tsbuildinfo", "typescript build info beside a config"),
        ("build/nested/app.tsbuildinfo", "typescript build info in a build subdirectory"),
        ("ts/build/src/game/sim.d.ts", "compiled typescript declarations"),
        ("build/lua-tests/src/runtime/config.js.map", "compiled typescript source map"),
        ("ts/build/number-rules.json", "generated number rules below the lane root"),
        ("dist/es/lib.mjs", "package build output"),
        ("site/.astro/types.d.ts", "astro generated types"),
    ];
    let kept = [
        ("secrets.env", "a credentials file"),
        ("src/build.js", "a script outside a build directory"),
        ("build/golden/golden.lua", "a golden output that is not compiled typescript"),
        ("distro/notes.md", "a directory named like dist"),
        ("build/number-rules.json.bak", "a backup of generated rules"),
        ("result-notes/todo.md", "a directory named like a result link"),
    ];
    for (path, kind) in regenerable {
        assert!(disposable(path), "{kind} is disposable: {path}");
    }
    for (path, kind) in kept {
        assert!(!disposable(path), "{kind} is kept: {path}");
    }
}

#[test]
fn a_landed_lane_leaves_no_cargo_target() {
    let container = Container::new();
    let lane = container.add("landed-live", true);
    fs::create_dir_all(lane.join("target/debug")).unwrap();
    fs::write(lane.join("target/debug/out"), vec![7u8; 64 * 1024]).unwrap();
    fs::write(container.main().join(".git/info/exclude"), "target/\n").unwrap();
    let mut live: Child = Command::new("sleep").arg("60").current_dir(&lane).spawn().unwrap();

    let (lines, counts) = container.sweep(false);
    live.kill().unwrap();
    live.wait().unwrap();

    assert!(lane.is_dir() && branch_exists(&container, "landed-live"), "a process inside keeps the lane: {lines:#?}");
    assert!(!lane.join("target").exists(), "a landed lane leaves no cargo target: {lines:#?}");
    assert_eq!(counts.kept_protected, 1, "{lines:#?}");
    a_landed_lane_named_by_its_ticket_loses_its_target_once_no_process_holds_it();
}

#[test]
fn a_sweep_reaps_only_spent_lanes_and_keeps_every_other_shape() {
    let container = Container::new();
    container.add("merged-fresh", false);
    container.add("merged-aged", true);
    container.add("rescue-merged", true);
    container.add("merged-locked", true);
    git_in(&container.main(), &["worktree", "lock", container.lane("merged-locked").to_str().unwrap()]);
    let live_lane = container.add("merged-live", true);
    let mut live: Child = Command::new("sleep").arg("60").current_dir(&live_lane).spawn().unwrap();

    let equiv = container.add("equiv-aged", false);
    container.commit_in(&equiv, "equiv-aged.txt");
    let equiv_sha = git_in(&container.main(), &["rev-parse", "equiv-aged"]);
    container.land_equivalent("equiv-aged");
    age(&equiv);

    let dirty = container.add("dirty", true);
    fs::write(dirty.join("f.txt"), "uncommitted\n").unwrap();
    let untracked = container.add("untracked", true);
    fs::write(untracked.join("scratch.txt"), "x\n").unwrap();
    let unmerged = container.add("unmerged", false);
    container.commit_in(&unmerged, "unique.txt");
    age(&unmerged);

    let ignored = container.add("equiv-ignored", false);
    container.commit_in(&ignored, "equiv-ignored.txt");
    container.land_equivalent("equiv-ignored");
    fs::create_dir_all(ignored.join("docs/private")).unwrap();
    fs::create_dir_all(ignored.join(".direnv/bin")).unwrap();
    fs::write(ignored.join("docs/private/note.md"), "notes\n").unwrap();
    fs::write(ignored.join(".direnv/bin/x"), "cache\n").unwrap();
    std::os::unix::fs::symlink("/nix/store/nonexistent", ignored.join("result")).unwrap();
    age(&ignored);

    let shadow = container.add("tag-shadow", false);
    container.commit_in(&shadow, "tag-shadow.txt");
    git_in(&container.main(), &["tag", "tag-shadow", "main"]);
    age(&shadow);

    let merge_lane = evil_merge_lane(&container);
    age(&merge_lane);

    let pin_oid = git_in(&container.main(), &["rev-parse", "HEAD"]);
    let pin = container.path().join("pins").join(&pin_oid);
    git_in(&container.main(), &["worktree", "add", "-q", "--detach", pin.to_str().unwrap(), &pin_oid]);
    age(&pin);
    let detached = container.lane("detached-aged");
    git_in(&container.main(), &["worktree", "add", "-q", "--detach", detached.to_str().unwrap(), "main"]);
    age(&detached);

    let (dry_lines, _) = container.sweep(true);
    assert!(container.lane("merged-aged").is_dir() && container.lane("equiv-aged").is_dir(), "dry run must change nothing");
    let (lines, counts) = container.sweep(false);
    live.kill().unwrap();
    live.wait().unwrap();

    for kept in ["merged-fresh", "rescue-merged", "merged-locked", "merged-live", "dirty", "untracked", "unmerged", "equiv-ignored", "tag-shadow", "merge-lane"] {
        assert!(container.lane(kept).is_dir() && branch_exists(&container, kept), "{kept} must be kept");
    }
    for reaped in ["merged-aged", "equiv-aged", "detached-aged"] {
        assert!(!container.lane(reaped).exists(), "{reaped} must be reaped");
    }
    assert!(!branch_exists(&container, "merged-aged") && !branch_exists(&container, "equiv-aged"));
    assert!(git_in(&container.main(), &["cat-file", "-t", &equiv_sha]) == "commit");
    assert!(pin.is_dir());
    assert!(!lines.iter().chain(&dry_lines).any(|line| line.contains(&pin_oid)));
    assert!(git_in(&container.main(), &["tag", "--list", "tag-shadow"]) == "tag-shadow");
    assert_eq!(counts, Counts { reaped: 2, reaped_equivalent: 1, kept_dirty: 2, kept_unmerged: 3, kept_protected: 5 }, "{lines:#?}");

    fs::remove_file(ignored.join("docs/private/note.md")).unwrap();
    fs::remove_dir(ignored.join("docs/private")).unwrap();
    fs::remove_dir(ignored.join("docs")).unwrap();
    container.sweep(false);
    assert!(!ignored.exists() && !branch_exists(&container, "equiv-ignored"));
}


fn a_landed_lane_named_by_its_ticket_loses_its_target_once_no_process_holds_it() {
    let container = Container::new();
    let origin = "https://github.com/example/north.git";
    git_in(&container.main(), &["remote", "add", "origin", origin]);
    fs::write(container.main().join(".git/info/exclude"), "target/\n").unwrap();
    let state = container.path().join("state");
    let done = north_base::platform::land_queue::directory(&state, origin).join("done");
    fs::create_dir_all(&done).unwrap();
    let ticket = |lane: &Path, commit: &str| {
        let head = git_in(lane, &["rev-parse", "HEAD"]);
        let record = serde_json::json!({"verdict": format!("landed {commit}"), "host": north_base::platform::land_queue::local_host(), "root": lane, "head": head});
        fs::write(done.join(format!("{}.json", lane.file_name().unwrap().to_string_lossy())), record.to_string()).unwrap();
    };
    let built = |lane: &Path| {
        fs::create_dir_all(lane.join("target/debug")).unwrap();
        fs::write(lane.join("target/debug/out"), vec![7u8; 64 * 1024]).unwrap();
    };
    let landed = container.add("landed-squash", false);
    container.commit_in(&landed, "landed.txt");
    container.squash_land("landed-squash", "land landed-squash");
    ticket(&landed, &git_in(&container.main(), &["rev-parse", "HEAD"]));
    built(&landed);
    let unlanded = container.add("unlanded", false);
    container.commit_in(&unlanded, "unlanded.txt");
    ticket(&unlanded, &git_in(&unlanded, &["rev-parse", "HEAD"]));
    built(&unlanded);
    let moved = container.add("moved-on", false);
    container.commit_in(&moved, "moved.txt");
    container.squash_land("moved-on", "land moved-on");
    ticket(&moved, &git_in(&container.main(), &["rev-parse", "HEAD"]));
    container.commit_in(&moved, "more.txt");
    built(&moved);
    let code = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(container.path(), code.path().join("north")).unwrap();
    let proc_root = code.path().join("proc");
    let release = || super::release_landed_targets(code.path(), &state, &proc_root, false).into_iter().map(|gone| gone.path).collect::<Vec<_>>();

    fs::create_dir_all(proc_root.join("4242")).unwrap();
    std::os::unix::fs::symlink(&landed, proc_root.join("4242/cwd")).unwrap();
    assert!(release().is_empty(), "a process working in the landed lane owns it, so its target stays");
    fs::remove_file(proc_root.join("4242/cwd")).unwrap();

    assert_eq!(release(), vec![landed.join("target")], "once its session is gone the landed lane's target goes, and only its");
    assert!(landed.join(".git").exists() && unlanded.join("target/debug/out").exists() && moved.join("target/debug/out").exists(), "the worktree stays; a lane whose commit is not on main or that moved past its ticket's head keeps its target");
}
