use north_core::foundation::shim;
use std::io::Read;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::LazyLock;
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

use crate::support::template::Template;

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

static TEMPLATE: LazyLock<Template> = LazyLock::new(|| {
    Template::build(|template| {
        let (remote, repo) = (template.join("remote.git"), template.join("repo"));
        git(template, &["init", "-q", "--bare", "remote.git"]);
        git(template, &["init", "-q", "-b", "main", "repo"]);
        for (dir, key, value) in [(&remote, "gc.auto", "0"), (&remote, "receive.autogc", "false"), (&repo, "gc.auto", "0"), (&repo, "maintenance.auto", "false")] {
            git(dir, &["config", key, value]);
        }
        git(&repo, &["config", "user.name", "safe-push-test"]);
        git(&repo, &["config", "user.email", "safe-push-test@example.invalid"]);
        commit(&repo, "content.txt", "initial\n", "initial");
        git(&repo, &["remote", "add", "origin", &remote.to_string_lossy()]);
        git(&remote, &["fetch", "-q", &repo.to_string_lossy(), "main:main", "main:other"]);
        git(&repo, &["fetch", "-q", "origin"]);
        git(&repo, &["branch", "-q", "--set-upstream-to=origin/main"]);
        git(&repo, &["commit", "--allow-empty", "-qm", "outgoing"]);
    })
});

impl Fixture {
    fn new() -> Self {
        let scratch = tempfile::tempdir().unwrap();
        let template = scratch.path().join("template");
        TEMPLATE.copy_into(&template);
        git(&template.join("repo"), &["remote", "set-url", "origin", &template.join("remote.git").to_string_lossy()]);
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

#[derive(Clone, Copy)]
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
    shim::write_script(&bin.join("gh"), &format!("printf '%s\\n' {}\n", case.visibility));
    if case.cancel == Some("scan") {
        shim::write_script(&bin.join("gitleaks"), "kill -TERM $PPID\nsleep 1\n");
    }
    if case.cancel == Some("push") {
        let hook = copy.join("repo/.git/hooks/pre-push");
        shim::write_script(&hook, "kill -TERM $(ps -o ppid= -p $PPID)\nsleep 1\n");
    }
    bin
}

const STALL_LIMIT: Duration = Duration::from_secs(60);
const STALL_SAMPLE: Duration = Duration::from_secs(1);

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
    let root = child.id();
    let (exited, exit) = std::sync::mpsc::channel();
    let waiter = std::thread::spawn(move || {
        let status = child.wait().unwrap();
        let _ = exited.send(());
        status
    });
    let (mut spent, mut idle_since) = (0, Instant::now());
    let status = loop {
        if exit.recv_timeout(STALL_SAMPLE) != Err(RecvTimeoutError::Timeout) {
            break waiter.join().unwrap();
        }
        let tree = process_tree(root);
        let ticks = tree.iter().map(|row| row.1).sum::<u64>();
        if ticks > spent {
            (spent, idle_since) = (ticks, Instant::now());
        } else if idle_since.elapsed() > stall_limit {
            for (pid, _) in &tree {
                Command::new("kill").args(["-KILL", &pid.to_string()]).status().unwrap();
            }
            panic!("the process tree used no CPU for {stall_limit:?}: it is deadlocked");
        }
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

struct Staged<'a> {
    case: Case<'a>,
    script: PathBuf,
    binary: PathBuf,
}

fn stage<'a>(fixture: &Fixture, case: Case<'a>) -> Staged<'a> {
    let label = case.arguments.join("_").replace(['-', '/'], "");
    Staged { case, script: fixture.copy(&format!("script{label}")), binary: fixture.copy(&format!("binary{label}")) }
}

fn all_entries(staged: &[Staged]) -> Vec<Observed> {
    let entry = Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/safe-push");
    let script_program = [entry.to_str().unwrap()];
    let binary_program = [env!("CARGO_BIN_EXE_north"), "safe-push"];
    std::thread::scope(|scope| {
        let runs: Vec<_> = staged.iter().map(|staged| (scope.spawn(|| run(&staged.script, &script_program, &staged.case)), scope.spawn(|| run(&staged.binary, &binary_program, &staged.case)))).collect();
        runs.into_iter().zip(staged).map(|((script, binary), staged)| compare(script.join().unwrap(), binary.join().unwrap(), &staged.case)).collect()
    })
}

fn compare(script: Observed, binary: Observed, case: &Case) -> Observed {
    assert_eq!(script.status, binary.status, "exit code differs for {:?}\nbin/safe-push: {script:#?}\nnorth safe-push: {binary:#?}", case.arguments);
    assert_eq!(script.stdout, binary.stdout, "stdout differs for {:?}", case.arguments);
    assert_eq!(script.stderr, binary.stderr, "stderr differs for {:?}", case.arguments);
    assert_eq!(script.remote, binary.remote, "origin differs for {:?}", case.arguments);
    assert_eq!(script.scratch_refs, binary.scratch_refs, "fetched origin refs differ for {:?}", case.arguments);
    binary
}

#[test]
fn safe_push_tag_refuses_failed_guards_through_either_entry() {
    let fixture = Fixture::new();
    let repo = fixture.repo();
    let mut refusals = Vec::new();
    git(&repo, &["checkout", "-q", "-b", "python", "main"]);
    commit(&repo, "new.py", "print('x')\n", "add python");
    tag(&repo, "release-python", "python release");
    refusals.push(("language gate", "language gate refused the landing", stage(&fixture, Case::new(&["--tag", "release-python"]))));

    git(&repo, &["checkout", "-q", "-b", "agents", "main"]);
    commit(&repo, "AGENTS.md", &"line\n".repeat(101), "long instructions");
    tag(&repo, "release-agents", "agents release");
    refusals.push(("instruction limit", "AGENTS.md@", stage(&fixture, Case::new(&["--tag", "release-agents"]))));

    git(&repo, &["checkout", "-q", "-b", "carriage", "main"]);
    commit(&repo, "AGENTS.md", &"line\r".repeat(101), "long instructions split by carriage returns");
    tag(&repo, "release-carriage", "carriage release");
    refusals.push(("carriage-return instruction limit", "AGENTS.md@", stage(&fixture, Case::new(&["--tag", "release-carriage"]))));

    git(&repo, &["checkout", "-q", "main"]);
    tag(&repo, "release-native", "closes life#12");
    refusals.push(("native ref", "native NorthThread item", stage(&fixture, Case::new(&["--tag", "release-native"]))));

    tag(&repo, "release-owner", &format!("owner {PRIVATE_TERM}"));
    let owner = Case { denylist: true, ..Case::new(&["--tag", "release-owner"]) };
    refusals.push(("deny-list", "fake-private-term in tag annotation", stage(&fixture, owner)));

    tag(&repo, "release-token", &format!("token ghp_{}", "aB3dE5fG7hJ9kL1mN3pQ5rS7tU9vW1xY3zA5"));
    refusals.push(("annotation secret", "SECRET DETECTED in tag annotation", stage(&fixture, Case::new(&["--tag", "release-token"]))));

    git(&repo, &["checkout", "-q", "-b", "email", "main"]);
    git(&repo, &["-c", "user.email=someone@private.example", "commit", "--allow-empty", "-qm", "private author"]);
    tag(&repo, "release-email", "email release");
    refusals.push(("email", "author email someone@private.example is not safe-push-test@example.invalid", stage(&fixture, Case::new(&["--tag", "release-email"]))));
    let (labels, staged): (Vec<(&str, &str)>, Vec<Staged>) = refusals.into_iter().map(|(name, expected, staged)| ((name, expected), staged)).unzip();
    for ((name, expected), observed) in labels.into_iter().zip(all_entries(&staged)) {
        assert_eq!(observed.status, 1, "{name}: {observed:#?}");
        assert!(format!("{}{}", observed.stdout, observed.stderr).contains(expected), "{name}: {observed:#?}");
        assert!(!observed.remote.contains("refs/tags/"), "{name} published");
    }
}

fn remote_has(observed: &Observed, name: &str) -> bool {
    observed.remote.lines().any(|line| line.starts_with(&format!("refs/tags/{name} ")))
}

#[test]
fn safe_push_delete_tag_deletes_only_a_held_non_release_tag_through_either_entry() {
    let fixture = Fixture::new();
    let repo = fixture.repo();
    git(&repo, &["tag", "-a", "stray-merged", "-m", "merged stray", "origin/main"]);
    git(&repo, &["tag", "stray-light", "origin/main"]);
    git(&repo, &["tag", "-a", "v1.0.0", "-m", "release", "origin/main"]);
    git(&repo, &["checkout", "-q", "-b", "side", "main"]);
    commit(&repo, "side.txt", "side\n", "side only");
    tag(&repo, "stray-unheld", "unheld stray");
    git(&repo, &["checkout", "-q", "-b", "side-mirrored", "main"]);
    commit(&repo, "mirrored.txt", "mirrored\n", "mirrored only");
    tag(&repo, "stray-mirrored", "mirrored stray");
    git(&repo, &["checkout", "-q", "main"]);
    git(&repo, &["push", "-q", "origin", "stray-merged", "stray-light", "v1.0.0", "stray-unheld", "stray-mirrored"]);

    let mirrors = fixture._scratch.path().join("mirrors");
    let mirror = mirrors.join("remote.git");
    std::fs::create_dir_all(&mirrors).unwrap();
    git(&mirrors, &["init", "-q", "--bare", "remote.git"]);
    git(&mirror, &["fetch", "-q", &repo.to_string_lossy(), "refs/tags/stray-mirrored:refs/tags/stray-mirrored"]);
    let mirrors_path = mirrors.to_string_lossy().into_owned();
    let empty = fixture._scratch.path().join("no-mirrors").to_string_lossy().into_owned();
    let held = [("LANE_GC_MIRRORS", mirrors_path.as_str())];
    let absent = [("LANE_GC_MIRRORS", empty.as_str())];

    let cases = [
        Case { environment: &absent, ..Case::new(&["--delete-tag", "stray-merged"]) },
        Case { environment: &absent, ..Case::new(&["--delete-tag", "stray-light"]) },
        Case { environment: &absent, ..Case::new(&["--dry-run", "--delete-tag", "stray-merged"]) },
        Case { environment: &held, ..Case::new(&["--delete-tag", "stray-mirrored"]) },
        Case { environment: &held, ..Case::new(&["--delete-tag", "v1.0.0"]) },
        Case { environment: &absent, ..Case::new(&["--delete-tag", "stray-unheld"]) },
        Case { environment: &held, ..Case::new(&["--delete-tag", "stray-unheld"]) },
        Case { environment: &held, ..Case::new(&["--delete-tag", "missing"]) },
        Case { environment: &held, ..Case::new(&["--delete-tag", "bad..name"]) },
    ];
    let staged: Vec<Staged> = cases.into_iter().map(|case| stage(&fixture, case)).collect();
    let [merged, light, dry, mirrored, release, unheld, unheld_in_mirror, missing, invalid]: [Observed; 9] = all_entries(&staged).try_into().unwrap();

    assert_eq!(merged.status, 0, "{merged:#?}");
    assert!(!remote_has(&merged, "stray-merged") && remote_has(&merged, "v1.0.0"), "{merged:#?}");
    assert!(merged.stdout.contains("deleted tag stray-merged from origin"), "{merged:#?}");
    assert_eq!(light.status, 0, "{light:#?}");
    assert!(!remote_has(&light, "stray-light"), "{light:#?}");
    assert_eq!(dry.status, 0, "{dry:#?}");
    assert!(remote_has(&dry, "stray-merged") && dry.stdout.contains("would delete tag stray-merged"), "{dry:#?}");
    assert_eq!(mirrored.status, 0, "{mirrored:#?}");
    assert!(!remote_has(&mirrored, "stray-mirrored") && mirrored.stdout.contains("is held by"), "{mirrored:#?}");
    assert_eq!(release.status, 1, "{release:#?}");
    assert!(remote_has(&release, "v1.0.0") && release.stdout.contains("is a release"), "{release:#?}");
    assert_eq!(unheld.status, 1, "{unheld:#?}");
    assert!(remote_has(&unheld, "stray-unheld") && unheld.stdout.contains("neither on origin's default branch nor held"), "{unheld:#?}");
    assert_eq!(unheld_in_mirror.status, 1, "{unheld_in_mirror:#?}");
    assert!(remote_has(&unheld_in_mirror, "stray-unheld"), "{unheld_in_mirror:#?}");
    assert_eq!(missing.status, 1, "{missing:#?}");
    assert!(missing.stdout.contains("origin has no tag"), "{missing:#?}");
    assert_eq!(invalid.status, 1, "{invalid:#?}");
    assert!(invalid.stdout.contains("not a valid tag name"), "{invalid:#?}");
}
