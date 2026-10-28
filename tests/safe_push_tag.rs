use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::Command;

const PRIVATE_TERM: &str = "zebra-quokka-7731";
const DENYLIST: &str = "title = \"safe-push tag test deny-list\"\n[[rules]]\nid = \"fake-private-term\"\ndescription = \"fake private identity term\"\nregex = '''zebra-quokka-7731'''\n";

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git").current_dir(dir).args(args).output().unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim_end().to_owned()
}

fn commit(repo: &Path, path: &str, content: &str, message: &str) {
    std::fs::create_dir_all(repo.join(path).parent().unwrap()).unwrap();
    std::fs::write(repo.join(path), content).unwrap();
    git(repo, &["add", path]);
    git(repo, &["commit", "-qm", message]);
}

fn tag(repo: &Path, name: &str, message: &str) {
    git(repo, &["tag", "-a", name, "-m", message]);
}

struct Fixture {
    _scratch: tempfile::TempDir,
    template: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let scratch = tempfile::tempdir().unwrap();
        let template = scratch.path().join("template");
        let (remote, repo) = (template.join("remote.git"), template.join("repo"));
        std::fs::create_dir_all(&template).unwrap();
        git(&template, &["init", "-q", "--bare", "remote.git"]);
        git(&template, &["init", "-q", "-b", "main", "repo"]);
        git(&repo, &["config", "user.name", "safe-push-test"]);
        git(&repo, &["config", "user.email", "safe-push-test@example.invalid"]);
        commit(&repo, "content.txt", "initial\n", "initial");
        git(&repo, &["remote", "add", "origin", &remote.to_string_lossy()]);
        git(&remote, &["fetch", "-q", &repo.to_string_lossy(), "main:main", "main:other"]);
        git(&repo, &["fetch", "-q", "origin"]);
        git(&repo, &["branch", "-q", "--set-upstream-to=origin/main"]);
        git(&repo, &["commit", "--allow-empty", "-qm", "outgoing"]);
        Self { _scratch: scratch, template }
    }

    fn repo(&self) -> PathBuf {
        self.template.join("repo")
    }

    fn copy(&self, label: &str) -> PathBuf {
        let copy = tempfile::Builder::new().prefix(label).tempdir_in(self.template.parent().unwrap()).unwrap().keep();
        std::fs::remove_dir(&copy).unwrap();
        let status = Command::new("cp").arg("-a").arg(&self.template).arg(&copy).status().unwrap();
        assert!(status.success());
        git(&copy.join("repo"), &["remote", "set-url", "origin", &copy.join("remote.git").to_string_lossy()]);
        copy
    }
}

struct Case<'a> {
    arguments: &'a [&'a str],
    visibility: &'a str,
    denylist: bool,
    environment: &'a [(&'a str, &'a str)],
    cancel: Option<&'a str>,
}

impl<'a> Case<'a> {
    fn new(arguments: &'a [&'a str]) -> Self {
        Self { arguments, visibility: "PUBLIC", denylist: false, environment: &[], cancel: None }
    }
}

#[derive(Debug, Eq, PartialEq)]
struct Observed {
    status: i32,
    stdout: String,
    stderr: String,
    remote: String,
    scratch_refs: String,
}

fn shims(copy: &Path, case: &Case) -> PathBuf {
    let bin = copy.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    symlink(env!("CARGO_BIN_EXE_north"), bin.join("north")).unwrap();
    std::fs::write(bin.join("gh"), format!("#!/bin/sh\nprintf '%s\\n' {}\n", case.visibility)).unwrap();
    std::fs::set_permissions(bin.join("gh"), std::fs::Permissions::from_mode(0o755)).unwrap();
    if case.cancel == Some("scan") {
        std::fs::write(bin.join("gitleaks"), "#!/bin/sh\nkill -TERM $PPID\nsleep 5\n").unwrap();
        std::fs::set_permissions(bin.join("gitleaks"), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    if case.cancel == Some("push") {
        let hook = copy.join("repo/.git/hooks/pre-push");
        std::fs::write(&hook, "#!/bin/sh\nkill -TERM $(ps -o ppid= -p $PPID)\nsleep 1\n").unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    bin
}

fn run(copy: &Path, program: &[&str], case: &Case) -> Observed {
    let bin = shims(copy, case);
    let denylist = copy.join("denylist.toml");
    if case.denylist {
        std::fs::write(&denylist, DENYLIST).unwrap();
    }
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default());
    let output = Command::new(program[0])
        .args(&program[1..])
        .args(case.arguments)
        .current_dir(copy.join("repo"))
        .env("PATH", path)
        .env("XDG_STATE_HOME", copy.join("state"))
        .env("SAFE_PUSH_DENYLIST", &denylist)
        .env("SAFE_PUSH_SOUTH", copy.join("absent-south"))
        .env("SAFE_PUSH_TEST_REAL_NORTH", env!("CARGO_BIN_EXE_north"))
        .env_remove("NORTH_NO_AUTOREAP")
        .env_remove("SAFE_PUSH_OUTPUT_CLAIMED")
        .envs(case.environment.iter().copied())
        .output()
        .unwrap();
    let root = copy.to_string_lossy().into_owned();
    let normal = |bytes: &[u8]| String::from_utf8_lossy(bytes).replace(&root, "FIXTURE");
    let stderr = normal(&output.stderr).lines().filter(|line| !gitleaks_log(line)).map(|line| format!("{line}\n")).collect();
    Observed {
        status: output.status.code().unwrap(),
        stdout: normal(&output.stdout),
        stderr,
        remote: git(&copy.join("remote.git"), &["for-each-ref", "--format=%(refname) %(objectname)"]),
        scratch_refs: git(&copy.join("repo"), &["for-each-ref", "refs/safe-push-origin/"]),
    }
}

fn gitleaks_log(line: &str) -> bool {
    let line = &plain(line);
    [" INF ", " WRN ", " ERR "].iter().any(|level| line.contains(level))
        && line.split(' ').next().is_some_and(|clock| clock.ends_with("AM") || clock.ends_with("PM") || clock.contains(':'))
}

fn plain(line: &str) -> String {
    let mut plain = String::new();
    let mut escaped = false;
    for character in line.chars() {
        match (escaped, character) {
            (false, '\u{1b}') => escaped = true,
            (true, 'm') => escaped = false,
            (false, _) => plain.push(character),
            (true, _) => {}
        }
    }
    plain
}

fn parity(fixture: &Fixture, case: &Case) -> Observed {
    let label = case.arguments.join("_").replace(['-', '/'], "");
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/safe-push");
    let old = run(&fixture.copy(&format!("old{label}")), &[script.to_str().unwrap()], case);
    let new = run(&fixture.copy(&format!("new{label}")), &[env!("CARGO_BIN_EXE_north"), "safe-push"], case);
    assert_eq!(old.status, new.status, "exit code differs for {:?}\nold: {old:#?}\nnew: {new:#?}", case.arguments);
    assert_eq!(old.stdout, new.stdout, "stdout differs for {:?}", case.arguments);
    assert_eq!(old.stderr, new.stderr, "stderr differs for {:?}", case.arguments);
    assert_eq!(old.remote, new.remote, "origin differs for {:?}", case.arguments);
    assert_eq!(old.scratch_refs, new.scratch_refs, "fetched origin refs differ for {:?}", case.arguments);
    new
}

fn published(observed: &Observed, fixture: &Fixture, name: &str) -> bool {
    let tag_oid = git(&fixture.repo(), &["rev-parse", &format!("refs/tags/{name}")]);
    observed.remote.lines().any(|line| line == format!("refs/tags/{name} {tag_oid}"))
}

#[test]
fn safe_push_tag_publishes_like_the_shell_on_clean_fixtures() {
    let fixture = Fixture::new();
    tag(&fixture.repo(), "release-outgoing", "release annotation");
    git(&fixture.repo(), &["tag", "-a", "release-published", "-m", "published release", "origin/main"]);

    let cancelled = parity(&fixture, &Case { cancel: Some("scan"), ..Case::new(&["--tag", "release-outgoing"]) });
    assert_eq!(cancelled.status, 130, "{cancelled:#?}");
    assert!(cancelled.scratch_refs.is_empty() && !published(&cancelled, &fixture, "release-outgoing"), "{cancelled:#?}");

    let pushing = parity(&fixture, &Case { cancel: Some("push"), ..Case::new(&["--tag", "release-outgoing"]) });
    assert_eq!(pushing.status, 130, "{pushing:#?}");
    assert!(pushing.scratch_refs.is_empty() && published(&pushing, &fixture, "release-outgoing"), "{pushing:#?}");

    let dry = parity(&fixture, &Case::new(&["--dry-run", "--tag", "release-outgoing"]));
    assert_eq!(dry.status, 0, "{dry:#?}");
    assert!(dry.stdout.contains("would push tag release-outgoing -> origin"), "{dry:#?}");
    assert!(!published(&dry, &fixture, "release-outgoing"));

    let outgoing = parity(&fixture, &Case::new(&["--tag", "release-outgoing"]));
    assert_eq!(outgoing.status, 0, "{outgoing:#?}");
    assert!(published(&outgoing, &fixture, "release-outgoing"), "{outgoing:#?}");
    let main = git(&fixture.repo(), &["rev-parse", "origin/main"]);
    assert!(outgoing.remote.lines().any(|line| line == format!("refs/heads/main {main}")), "tag publication moved main: {outgoing:#?}");
    let heads: Vec<&str> = outgoing.remote.lines().filter_map(|line| line.split_once(' ')).map(|(name, _)| name).filter(|name| name.starts_with("refs/heads/")).collect();
    assert_eq!(heads, ["refs/heads/main", "refs/heads/other"], "tag publication created a remote branch");

    let already = parity(&fixture, &Case::new(&["--tag", "release-published"]));
    assert_eq!(already.status, 0, "{already:#?}");
    assert!(published(&already, &fixture, "release-published"));

    let fallback = Case {
        environment: &[("SAFE_PUSH_SOUTH", ""), ("SAFE_PUSH_DENYLIST", ""), ("HOME", "/nonexistent-safe-push-home")],
        ..Case::new(&["--dry-run", "--tag", "release-outgoing"])
    };
    let fallback = parity(&fixture, &fallback);
    assert!(fallback.stdout.contains("no south checkout at /nonexistent-safe-push-home/code/south"), "{fallback:#?}");

    let private = Case { visibility: "PRIVATE", denylist: true, ..Case::new(&["--tag", "release-outgoing"]) };
    assert_eq!(parity(&fixture, &private).status, 0);
}

#[test]
fn safe_push_tag_refuses_like_the_shell_before_any_scan() {
    let fixture = Fixture::new();
    git(&fixture.repo(), &["tag", "release-light"]);
    for arguments in [&["--tag", "missing"][..], &["--tag", "release-light"], &["--wat"], &["--to", "main", "--tag", "x"], &["--help"]] {
        let observed = parity(&fixture, &Case::new(arguments));
        assert_eq!(observed.status != 0, arguments != ["--help"], "{arguments:?}: {observed:#?}");
    }
    let raw = Case { environment: &[("NORTH_NO_AUTOREAP", "1")], ..Case::new(&["--tag", "--dry-run"]) };
    assert_eq!(parity(&fixture, &raw).status, 2);
    git(&fixture.repo(), &["checkout", "-q", "--detach"]);
    tag(&fixture.repo(), "release-detached", "detached release");
    let detached = parity(&fixture, &Case::new(&["--tag", "release-detached"]));
    assert!(detached.stdout.contains("detached HEAD"), "{detached:#?}");
}

#[test]
fn safe_push_tag_refuses_like_the_shell_when_a_guard_fails() {
    let fixture = Fixture::new();
    let repo = fixture.repo();
    let refused = |name: &str, case: &Case, expected: &str| {
        let observed = parity(&fixture, case);
        assert_eq!(observed.status, 1, "{name}: {observed:#?}");
        assert!(format!("{}{}", observed.stdout, observed.stderr).contains(expected), "{name}: {observed:#?}");
        assert!(!observed.remote.contains("refs/tags/"), "{name} published");
    };
    git(&repo, &["checkout", "-q", "-b", "python", "main"]);
    commit(&repo, "new.py", "print('x')\n", "add python");
    tag(&repo, "release-python", "python release");
    refused("language gate", &Case::new(&["--tag", "release-python"]), "language gate refused the landing");

    git(&repo, &["checkout", "-q", "-b", "agents", "main"]);
    commit(&repo, "AGENTS.md", &"line\n".repeat(101), "long instructions");
    tag(&repo, "release-agents", "agents release");
    refused("instruction limit", &Case::new(&["--tag", "release-agents"]), "AGENTS.md@");

    git(&repo, &["checkout", "-q", "-b", "carriage", "main"]);
    commit(&repo, "AGENTS.md", &"line\r".repeat(101), "long instructions split by carriage returns");
    tag(&repo, "release-carriage", "carriage release");
    refused("carriage-return instruction limit", &Case::new(&["--tag", "release-carriage"]), "AGENTS.md@");

    git(&repo, &["checkout", "-q", "main"]);
    tag(&repo, "release-native", "closes life#12");
    refused("native ref", &Case::new(&["--tag", "release-native"]), "native NorthThread item");

    tag(&repo, "release-owner", &format!("owner {PRIVATE_TERM}"));
    let owner = Case { denylist: true, ..Case::new(&["--tag", "release-owner"]) };
    refused("deny-list", &owner, "fake-private-term in tag annotation");

    tag(&repo, "release-token", &format!("token ghp_{}", "aB3dE5fG7hJ9kL1mN3pQ5rS7tU9vW1xY3zA5"));
    refused("annotation secret", &Case::new(&["--tag", "release-token"]), "SECRET DETECTED in tag annotation");

    git(&repo, &["checkout", "-q", "-b", "email", "main"]);
    git(&repo, &["-c", "user.email=someone@private.example", "commit", "--allow-empty", "-qm", "private author"]);
    tag(&repo, "release-email", "email release");
    refused("email", &Case::new(&["--tag", "release-email"]), "author email someone@private.example is not safe-push-test@example.invalid");
}
