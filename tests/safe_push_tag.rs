use std::io::Read;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

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

const STALL_LIMIT: Duration = Duration::from_secs(60);

fn process_tree(root: u32) -> Vec<(u32, u64)> {
    let table: Vec<(u32, u32, u64)> = std::fs::read_dir("/proc")
        .unwrap()
        .filter_map(|entry| {
            let pid: u32 = entry.ok()?.file_name().to_str()?.parse().ok()?;
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
            let fields: Vec<&str> = stat[stat.rfind(')')? + 2..].split(' ').collect();
            let ticks = (11..15).map(|index| fields[index].parse::<u64>().unwrap_or(0)).sum();
            Some((pid, fields[1].parse().ok()?, ticks))
        })
        .collect();
    let mut tree = vec![root];
    let mut index = 0;
    while index < tree.len() {
        let parent = tree[index];
        let children: Vec<u32> = table.iter().filter(|row| row.1 == parent && !tree.contains(&row.0)).map(|row| row.0).collect();
        tree.extend(children);
        index += 1;
    }
    table.iter().filter(|row| tree.contains(&row.0)).map(|row| (row.0, row.2)).collect()
}

fn output_unless_stalled(command: &mut Command, stall_limit: Duration) -> std::process::Output {
    let mut child = command.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let drain = |mut pipe: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            pipe.read_to_end(&mut bytes).unwrap();
            bytes
        })
    };
    let stdout = drain(Box::new(child.stdout.take().unwrap()));
    let stderr = drain(Box::new(child.stderr.take().unwrap()));
    let (mut spent, mut idle_since) = (0, Instant::now());
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        let tree = process_tree(child.id());
        let ticks = tree.iter().map(|row| row.1).sum::<u64>();
        if ticks > spent {
            (spent, idle_since) = (ticks, Instant::now());
        } else if idle_since.elapsed() > stall_limit {
            for (pid, _) in &tree {
                Command::new("kill").args(["-KILL", &pid.to_string()]).status().unwrap();
            }
            panic!("the process tree used no CPU for {stall_limit:?}: it is deadlocked");
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    std::process::Output { status, stdout: stdout.join().unwrap(), stderr: stderr.join().unwrap() }
}

fn run(copy: &Path, program: &[&str], case: &Case) -> Observed {
    let bin = shims(copy, case);
    let denylist = copy.join("denylist.toml");
    if case.denylist {
        std::fs::write(&denylist, DENYLIST).unwrap();
    }
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default());
    let mut command = Command::new(program[0]);
    command
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
        .envs(case.environment.iter().copied());
    let output = output_unless_stalled(&mut command, STALL_LIMIT);
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

fn both_entries(fixture: &Fixture, case: &Case) -> Observed {
    let label = case.arguments.join("_").replace(['-', '/'], "");
    let entry = Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/safe-push");
    let script = run(&fixture.copy(&format!("script{label}")), &[entry.to_str().unwrap()], case);
    let binary = run(&fixture.copy(&format!("binary{label}")), &[env!("CARGO_BIN_EXE_north"), "safe-push"], case);
    assert_eq!(script.status, binary.status, "exit code differs for {:?}\nbin/safe-push: {script:#?}\nnorth safe-push: {binary:#?}", case.arguments);
    assert_eq!(script.stdout, binary.stdout, "stdout differs for {:?}", case.arguments);
    assert_eq!(script.stderr, binary.stderr, "stderr differs for {:?}", case.arguments);
    assert_eq!(script.remote, binary.remote, "origin differs for {:?}", case.arguments);
    assert_eq!(script.scratch_refs, binary.scratch_refs, "fetched origin refs differ for {:?}", case.arguments);
    binary
}

fn published(observed: &Observed, fixture: &Fixture, name: &str) -> bool {
    let tag_oid = git(&fixture.repo(), &["rev-parse", &format!("refs/tags/{name}")]);
    observed.remote.lines().any(|line| line == format!("refs/tags/{name} {tag_oid}"))
}

#[test]
fn safe_push_tag_publishes_clean_fixtures_through_either_entry() {
    let fixture = Fixture::new();
    tag(&fixture.repo(), "release-outgoing", "release annotation");
    git(&fixture.repo(), &["tag", "-a", "release-published", "-m", "published release", "origin/main"]);

    let cancelled = both_entries(&fixture, &Case { cancel: Some("scan"), ..Case::new(&["--tag", "release-outgoing"]) });
    assert_eq!(cancelled.status, 130, "{cancelled:#?}");
    assert!(cancelled.scratch_refs.is_empty() && !published(&cancelled, &fixture, "release-outgoing"), "{cancelled:#?}");

    let pushing = both_entries(&fixture, &Case { cancel: Some("push"), ..Case::new(&["--tag", "release-outgoing"]) });
    assert_eq!(pushing.status, 130, "{pushing:#?}");
    assert!(pushing.scratch_refs.is_empty() && published(&pushing, &fixture, "release-outgoing"), "{pushing:#?}");

    let dry = both_entries(&fixture, &Case::new(&["--dry-run", "--tag", "release-outgoing"]));
    assert_eq!(dry.status, 0, "{dry:#?}");
    assert!(dry.stdout.contains("would push tag release-outgoing -> origin"), "{dry:#?}");
    assert!(!published(&dry, &fixture, "release-outgoing"));

    let outgoing = both_entries(&fixture, &Case::new(&["--tag", "release-outgoing"]));
    assert_eq!(outgoing.status, 0, "{outgoing:#?}");
    assert!(published(&outgoing, &fixture, "release-outgoing"), "{outgoing:#?}");
    let main = git(&fixture.repo(), &["rev-parse", "origin/main"]);
    assert!(outgoing.remote.lines().any(|line| line == format!("refs/heads/main {main}")), "tag publication moved main: {outgoing:#?}");
    let heads: Vec<&str> = outgoing.remote.lines().filter_map(|line| line.split_once(' ')).map(|(name, _)| name).filter(|name| name.starts_with("refs/heads/")).collect();
    assert_eq!(heads, ["refs/heads/main", "refs/heads/other"], "tag publication created a remote branch");

    let already = both_entries(&fixture, &Case::new(&["--tag", "release-published"]));
    assert_eq!(already.status, 0, "{already:#?}");
    assert!(published(&already, &fixture, "release-published"));

    let fallback = Case {
        environment: &[("SAFE_PUSH_SOUTH", ""), ("SAFE_PUSH_DENYLIST", ""), ("HOME", "/nonexistent-safe-push-home")],
        ..Case::new(&["--dry-run", "--tag", "release-outgoing"])
    };
    let fallback = both_entries(&fixture, &fallback);
    assert!(fallback.stdout.contains("no south checkout at /nonexistent-safe-push-home/code/south"), "{fallback:#?}");

    let private = Case { visibility: "PRIVATE", denylist: true, ..Case::new(&["--tag", "release-outgoing"]) };
    assert_eq!(both_entries(&fixture, &private).status, 0);
}

#[test]
fn safe_push_tag_refuses_before_any_scan_through_either_entry() {
    let fixture = Fixture::new();
    git(&fixture.repo(), &["tag", "release-light"]);
    for arguments in [&["--tag", "missing"][..], &["--tag", "release-light"], &["--wat"], &["--to", "main", "--tag", "x"], &["--help"]] {
        let observed = both_entries(&fixture, &Case::new(arguments));
        assert_eq!(observed.status != 0, arguments != ["--help"], "{arguments:?}: {observed:#?}");
    }
    let raw = Case { environment: &[("NORTH_NO_AUTOREAP", "1")], ..Case::new(&["--tag", "--dry-run"]) };
    assert_eq!(both_entries(&fixture, &raw).status, 2);
    git(&fixture.repo(), &["checkout", "-q", "--detach"]);
    tag(&fixture.repo(), "release-detached", "detached release");
    let detached = both_entries(&fixture, &Case::new(&["--tag", "release-detached"]));
    assert!(detached.stdout.contains("detached HEAD"), "{detached:#?}");
}

#[test]
fn safe_push_tag_refuses_failed_guards_through_either_entry() {
    let fixture = Fixture::new();
    let repo = fixture.repo();
    let refused = |name: &str, case: &Case, expected: &str| {
        let observed = both_entries(&fixture, case);
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

#[test]
fn bin_safe_push_refuses_a_tag_when_the_north_runtime_lacks_safe_push() {
    let fixture = Fixture::new();
    tag(&fixture.repo(), "release-stale", "release annotation");
    let copy = fixture.copy("stale");
    let stale = copy.join("stale-bin");
    std::fs::create_dir_all(&stale).unwrap();
    std::fs::write(stale.join("north"), "#!/bin/sh\nexit 2\n").unwrap();
    std::fs::set_permissions(stale.join("north"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let before = git(&copy.join("remote.git"), &["for-each-ref"]);
    let output = Command::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/safe-push"))
        .args(["--tag", "release-stale"])
        .current_dir(copy.join("repo"))
        .env("PATH", format!("{}:{}", stale.display(), std::env::var("PATH").unwrap_or_default()))
        .env("XDG_STATE_HOME", copy.join("state"))
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(1), "{stdout}");
    assert!(stdout.contains("this host's north runtime lacks; run north runtime update; NOT pushed."), "{stdout}");
    assert_eq!(git(&copy.join("remote.git"), &["for-each-ref"]), before, "a stale runtime published the tag");
}

#[test]
fn safe_push_tag_scans_an_outgoing_tag_against_an_origin_with_thousands_of_refs() {
    let fixture = Fixture::new();
    let repo = fixture.repo();
    let main = git(&repo, &["rev-parse", "origin/main"]);
    let refs: String = (0..6000).map(|n| format!("create refs/pull/{n}/head {main}\n")).collect();
    let mut update = Command::new("git").current_dir(fixture.template.join("remote.git")).args(["update-ref", "--stdin"]).stdin(std::process::Stdio::piped()).spawn().unwrap();
    std::io::Write::write_all(&mut update.stdin.take().unwrap(), refs.as_bytes()).unwrap();
    assert!(update.wait().unwrap().success());
    std::fs::create_dir_all(repo.join("bulk")).unwrap();
    for n in 0..2000 {
        std::fs::write(repo.join(format!("bulk/{n}.txt")), format!("{n}\n")).unwrap();
    }
    git(&repo, &["add", "bulk"]);
    git(&repo, &["commit", "-qm", "outgoing objects"]);
    tag(&repo, "release-bulk", "bulk release");
    let observed = run(&fixture.copy("bulk"), &[env!("CARGO_BIN_EXE_north"), "safe-push"], &Case::new(&["--tag", "release-bulk"]));
    assert_eq!(observed.status, 0, "a stalled scan panics before this: {observed:#?}");
    assert!(published(&observed, &fixture, "release-bulk"), "{observed:#?}");
}

#[test]
#[should_panic(expected = "deadlocked")]
fn a_process_tree_that_uses_no_cpu_is_reported_as_deadlocked() {
    output_unless_stalled(Command::new("sh").args(["-c", "sleep 30"]), Duration::from_secs(2));
}
