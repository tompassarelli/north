use north_core::foundation::shim;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const GH: &str = r#"printf '%s\n' "$*" >>"$GH_STATE/calls"
case "$1 $2" in
  "repo view") echo PUBLIC ;;
  "release view") [ -e "$GH_STATE/release-$3" ] ;;
  "release create")
    git --git-dir="$GH_REMOTE" rev-parse -q --verify "refs/tags/$3" >/dev/null || { echo "tag $3 is not on the remote" >&2; exit 1; }
    [ ! -e "$GH_STATE/release-$3" ] || { echo "release $3 already exists" >&2; exit 1; }
    touch "$GH_STATE/release-$3" ;;
  "api "*)
    case "$2" in
      */git/ref/tags/*) printf 'tag %s\n' "$(git --git-dir="$GH_REMOTE" rev-parse "refs/tags/${2##*/}")" ;;
      */git/tags/*) git --git-dir="$GH_REMOTE" cat-file tag "${2##*/}" | sed '1,/^$/d' ;;
    esac ;;
esac
"#;

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git").current_dir(dir).args(args).output().unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim_end().to_owned()
}

struct Fixture {
    scratch: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path();
        git(root, &["init", "-q", "--bare", "remote.git"]);
        git(root, &["init", "-q", "-b", "main", "repo"]);
        let repo = root.join("repo");
        for (key, value) in [("user.name", "release-test"), ("user.email", "release-test@example.invalid"), ("gc.auto", "0")] {
            git(&repo, &["config", key, value]);
        }
        std::fs::write(repo.join("content.txt"), "initial\n").unwrap();
        git(&repo, &["add", "content.txt"]);
        git(&repo, &["commit", "-qm", "initial"]);
        git(&repo, &["remote", "add", "origin", &root.join("remote.git").to_string_lossy()]);
        git(&repo, &["push", "-q", "origin", "main"]);
        git(&repo, &["fetch", "-q", "origin"]);
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(root.join("gh-state")).unwrap();
        symlink(env!("CARGO_BIN_EXE_north"), bin.join("north")).unwrap();
        shim::write_script(&bin.join("gh"), GH);
        shim::write_script(&bin.join("gitleaks"), "exit 0\n");
        Self { scratch }
    }

    fn root(&self) -> &Path {
        self.scratch.path()
    }

    fn repo(&self) -> PathBuf {
        self.root().join("repo")
    }

    fn remote(&self) -> PathBuf {
        self.root().join("remote.git")
    }

    fn command(&self, program: &str, dir: &Path) -> Command {
        let path = format!("{}:{}", self.root().join("bin").display(), std::env::var("PATH").unwrap_or_default());
        let mut command = Command::new(program);
        command
            .current_dir(dir)
            .env("PATH", path)
            .env("GH_STATE", self.root().join("gh-state"))
            .env("GH_REMOTE", self.remote())
            .env("XDG_STATE_HOME", self.root().join("state"))
            .env("SAFE_PUSH_DENYLIST", self.root().join("absent-denylist.toml"))
            .env("SAFE_PUSH_SOUTH", self.root().join("absent-south"))
            .env("NORTH_NO_AUTOREAP", "1")
            .env_remove("SAFE_PUSH_OUTPUT_CLAIMED");
        command
    }

    fn release(&self, arguments: &[&str]) -> Output {
        self.command(env!("CARGO_BIN_EXE_north"), &self.repo()).arg("release").args(arguments).output().unwrap()
    }

    fn remote_tag(&self, tag: &str) -> Option<String> {
        let output = Command::new("git").current_dir(self.remote()).args(["rev-parse", "-q", "--verify", &format!("refs/tags/{tag}")]).output().unwrap();
        output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }

    fn creates(&self, tag: &str) -> usize {
        let calls = std::fs::read_to_string(self.root().join("gh-state/calls")).unwrap_or_default();
        calls.lines().filter(|line| line.starts_with(&format!("release create {tag} "))).count()
    }

    fn released(&self, tag: &str) -> bool {
        self.root().join(format!("gh-state/release-{tag}")).exists()
    }
}

fn text(output: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
}

#[test]
fn release_creates_the_annotated_tag_and_its_github_release_in_one_step_and_refuses_a_second() {
    let fixture = Fixture::new();
    let notes = fixture.root().join("notes.md");
    std::fs::write(&notes, "First release\n").unwrap();
    let output = fixture.release(&["1.2.0", "--notes", &notes.to_string_lossy()]);
    assert!(output.status.success(), "{}", text(&output));
    let tag = fixture.remote_tag("v1.2.0").expect("v1.2.0 reached origin");
    assert_eq!(git(&fixture.remote(), &["cat-file", "-t", &tag]), "tag");
    assert_eq!(git(&fixture.remote(), &["rev-list", "-n1", &tag]), git(&fixture.repo(), &["rev-parse", "HEAD"]));
    assert_eq!(git(&fixture.remote(), &["tag", "-l", "--format=%(contents)", "v1.2.0"]), "First release");
    let calls = std::fs::read_to_string(fixture.root().join("gh-state/calls")).unwrap();
    assert!(calls.contains("release create v1.2.0 --verify-tag --title v1.2.0 --notes-from-tag"), "{calls}");
    let again = fixture.release(&["1.2.0"]);
    assert!(!again.status.success());
    assert!(text(&again).contains("v1.2.0 already exists with its GitHub release"), "{}", text(&again));
    assert_eq!(fixture.creates("v1.2.0"), 1);
}

#[test]
fn release_refuses_a_dirty_tree_a_branch_off_main_and_a_head_off_origin() {
    let fixture = Fixture::new();
    let repo = fixture.repo();
    std::fs::write(repo.join("stray.txt"), "dirty\n").unwrap();
    let dirty = fixture.release(&["1.0.0"]);
    assert!(text(&dirty).contains("the working tree is dirty"), "{}", text(&dirty));
    std::fs::remove_file(repo.join("stray.txt")).unwrap();
    git(&repo, &["switch", "-qc", "topic"]);
    let off_main = fixture.release(&["1.0.0"]);
    assert!(text(&off_main).contains("release from the main branch's checkout, not 'topic'"), "{}", text(&off_main));
    git(&repo, &["switch", "-q", "main"]);
    git(&repo, &["commit", "--allow-empty", "-qm", "unlanded"]);
    let ahead = fixture.release(&["1.0.0"]);
    assert!(text(&ahead).contains("HEAD is not origin/main"), "{}", text(&ahead));
    for output in [&dirty, &off_main, &ahead] {
        assert!(!output.status.success());
    }
    assert_eq!(fixture.remote_tag("v1.0.0"), None);
    assert!(git(&repo, &["tag", "-l"]).is_empty());
    assert_eq!(fixture.creates("v1.0.0"), 0);
}

#[test]
fn release_rerun_finishes_a_tag_pushed_without_its_release_and_a_tag_made_but_not_pushed() {
    let fixture = Fixture::new();
    let repo = fixture.repo();
    git(&repo, &["tag", "-a", "v2.0.0", "-m", "pushed by hand"]);
    git(&repo, &["push", "-q", "origin", "v2.0.0"]);
    let pushed = fixture.remote_tag("v2.0.0");
    let finished = fixture.release(&["2.0.0"]);
    assert!(finished.status.success(), "{}", text(&finished));
    assert!(text(&finished).contains("on origin without its GitHub release"), "{}", text(&finished));
    assert_eq!(fixture.remote_tag("v2.0.0"), pushed);
    assert!(fixture.released("v2.0.0"));
    git(&repo, &["tag", "-a", "v2.0.1", "-m", "made, push failed"]);
    let resumed = fixture.release(&["v2.0.1"]);
    assert!(resumed.status.success(), "{}", text(&resumed));
    assert!(text(&resumed).contains("reusing the local annotated tag v2.0.1"), "{}", text(&resumed));
    assert!(fixture.remote_tag("v2.0.1").is_some());
    assert!(fixture.released("v2.0.1"));
}

#[test]
fn safe_push_refuses_a_version_tag_outside_north_release() {
    let fixture = Fixture::new();
    let repo = fixture.repo();
    git(&repo, &["tag", "-a", "v3.0.0", "-m", "made by hand"]);
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/safe-push");
    for program in [script.to_str().unwrap(), env!("CARGO_BIN_EXE_north")] {
        let mut command = fixture.command(program, &repo);
        if program == env!("CARGO_BIN_EXE_north") {
            command.arg("safe-push");
        }
        let output = command.args(["--tag", "v3.0.0"]).output().unwrap();
        assert!(!output.status.success(), "{program}: {}", text(&output));
        assert!(text(&output).contains("version tag v3.0.0 publishes only through north release"), "{program}: {}", text(&output));
    }
    assert_eq!(fixture.remote_tag("v3.0.0"), None);
}

fn workflow_script() -> String {
    let workflow = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows/release-on-tag.yml")).unwrap();
    assert!(workflow.contains("tags: ['v*']"), "the workflow runs on every v* tag push");
    let mut lines = workflow.lines().skip_while(|line| line.trim() != "run: |").skip(1);
    let first = lines.next().unwrap();
    let indent = first.len() - first.trim_start().len();
    std::iter::once(first).chain(lines.take_while(|line| line.len() - line.trim_start().len() >= indent || line.trim().is_empty())).map(|line| format!("{}\n", line.get(indent..).unwrap_or(""))).collect()
}

#[test]
fn tag_workflow_creates_the_missing_release_of_a_tag_pushed_another_way_once() {
    let fixture = Fixture::new();
    let repo = fixture.repo();
    git(&repo, &["tag", "-a", "v4.0.0", "-m", "Tagged without north release"]);
    git(&repo, &["push", "-q", "origin", "v4.0.0"]);
    let script = workflow_script();
    let run = || fixture.command("bash", fixture.root()).args(["-c", &script]).env("TAG", "v4.0.0").env("GH_REPO", "owner/repo").output().unwrap();
    let first = run();
    assert!(first.status.success(), "{}", text(&first));
    assert!(text(&first).contains("created the GitHub release for v4.0.0"), "{}", text(&first));
    let calls = std::fs::read_to_string(fixture.root().join("gh-state/calls")).unwrap();
    assert!(calls.contains("release create v4.0.0 --verify-tag --title v4.0.0 --notes Tagged without north release"), "{calls}");
    let second = run();
    assert!(second.status.success(), "{}", text(&second));
    assert!(text(&second).contains("v4.0.0 already has its GitHub release"), "{}", text(&second));
    assert_eq!(fixture.creates("v4.0.0"), 1);
}
