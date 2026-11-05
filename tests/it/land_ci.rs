use north_core::foundation::shim;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const GH: &str = r#"#!/bin/sh
printf '%s\n' "$*" >>"$CI_TRACE"
case "$1" in
  run) exec cat "$CI_FIXTURE/runs.json" ;;
  api) id=${2%/jobs*}; exec cat "$CI_FIXTURE/${id##*/}.json" ;;
esac
exit 2
"#;

const CODE_HEALTH: &str = "#!/bin/sh\nprintf 'code-health %s\\n' \"$*\" >>\"$CI_TRACE\"\nexit \"${CODE_HEALTH_EXIT:-0}\"\n";

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(directory)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .args(arguments)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

struct Fixture {
    root: tempfile::TempDir,
    repo: PathBuf,
}

struct Gate {
    status: i32,
    output: String,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        git(root.path(), &["init", "-q", "--bare", "remote.git"]);
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["remote", "add", "origin", root.path().join("remote.git").to_str().unwrap()]);
        let fixture = Self { root, repo };
        fixture.write("content.txt", "initial\n");
        fixture.commit(&["initial"]);
        fixture.push();
        fs::create_dir_all(fixture.ci()).unwrap();
        fs::create_dir_all(fixture.root.path().join("bin")).unwrap();
        shim::write_executable(&fixture.root.path().join("bin/gh"), GH);
        shim::write_executable(&fixture.root.path().join("code-health"), CODE_HEALTH);
        fixture
    }

    fn write(&self, path: &str, text: &str) {
        let path = self.repo.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn append(&self, path: &str, text: &str) {
        let before = fs::read_to_string(self.repo.join(path)).unwrap_or_default();
        self.write(path, &format!("{before}{text}"));
    }

    fn ci(&self) -> PathBuf {
        self.root.path().join("ci")
    }

    fn ci_file(&self, name: &str, text: &str) {
        fs::write(self.ci().join(name), text).unwrap();
    }

    fn jobs(&self, id: u32, name: &str, seconds: u32) {
        let (minutes, rest) = (seconds / 60, seconds % 60);
        self.ci_file(&format!("{id}.json"), &format!(r#"[{{"jobs":[{{"name":"{name}","started_at":"2026-10-11T00:00:00Z","completed_at":"2026-10-11T00:{minutes:02}:{rest:02}Z"}}]}}]"#));
        self.forget_measurements();
    }

    fn state(&self) -> PathBuf {
        self.root.path().join("state/safe-push")
    }

    fn forget_measurements(&self) {
        let _ = fs::remove_dir_all(self.state().join("run-measurements"));
    }

    fn commit(&self, message: &[&str]) -> String {
        git(&self.repo, &["add", "-A", "."]);
        let mut arguments = vec!["commit", "-q", "--allow-empty"];
        for paragraph in message {
            arguments.extend(["-m", paragraph]);
        }
        git(&self.repo, &arguments);
        self.head()
    }

    fn amend(&self, message: &[&str]) {
        let mut arguments = vec!["commit", "-q", "--amend"];
        for paragraph in message {
            arguments.extend(["-m", paragraph]);
        }
        git(&self.repo, &arguments);
    }

    fn head(&self) -> String {
        git(&self.repo, &["rev-parse", "HEAD"])
    }

    fn push(&self) {
        git(&self.repo, &["push", "-q", "origin", "HEAD:main"]);
    }

    fn reset(&self, to: &str) {
        git(&self.repo, &["reset", "-q", "--hard", to]);
    }

    fn trace(&self) -> String {
        fs::read_to_string(self.root.path().join("trace")).unwrap_or_default()
    }

    fn gate(&self, name: &str, base: &str, head: &str, environment: &[(&str, &str)]) -> Gate {
        let path = format!("{}:{}", self.root.path().join("bin").display(), std::env::var("PATH").unwrap_or_default());
        let output = Command::new(env!("CARGO_BIN_EXE_north"))
            .current_dir(&self.repo)
            .args(["land", "scan", name, &format!("{base}..{head}")])
            .env("PATH", path)
            .env("HOME", self.root.path().join("home"))
            .env("XDG_STATE_HOME", self.root.path().join("state"))
            .env("CI_FIXTURE", self.ci())
            .env("CI_TRACE", self.root.path().join("trace"))
            .env("SAFE_PUSH_CODE_HEALTH", self.root.path().join("code-health"))
            .envs(environment.iter().copied())
            .stdin(Stdio::null())
            .output()
            .unwrap();
        Gate {
            status: output.status.code().unwrap_or(-1),
            output: format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr)),
        }
    }

    fn budget(&self) -> Gate {
        self.gate("ci-budget", "refs/remotes/origin/main", "HEAD", &[])
    }
}

impl Gate {
    fn passes(&self) -> &Self {
        assert_eq!(self.status, 0, "{}", self.output);
        self
    }

    fn refuses(&self) -> &Self {
        assert_ne!(self.status, 0, "{}", self.output);
        self
    }

    fn says(&self, text: &str) -> &Self {
        assert!(self.output.contains(text), "missing {text:?} in:\n{}", self.output);
        self
    }
}

const RUNS: &str = r#"[
  {"databaseId":106,"conclusion":"cancelled","createdAt":"2026-10-11T06:00:00Z"},
  {"databaseId":105,"conclusion":"skipped","createdAt":"2026-10-11T05:00:00Z"},
  {"databaseId":104,"conclusion":"failure","createdAt":"2026-10-11T04:00:00Z"},
  {"databaseId":103,"conclusion":"success","createdAt":"2026-10-11T03:00:00Z"},
  {"databaseId":102,"conclusion":"failure","createdAt":"2026-10-11T02:00:00Z"},
  {"databaseId":101,"conclusion":"success","createdAt":"2026-10-11T01:00:00Z"},
  {"databaseId":100,"conclusion":"success","createdAt":"2026-10-11T00:00:00Z"}
]"#;

fn ci_fixture(fixture: &Fixture) {
    fixture.ci_file("runs.json", RUNS);
    fixture.ci_file("104.json", r#"[{"jobs":[]}]"#);
    fixture.ci_file(
        "103.json",
        r#"[{"jobs":[{"name":"bootstrap","started_at":"2026-10-11T00:00:00Z","completed_at":"2026-10-11T00:02:00Z"}]},
            {"jobs":[{"name":"integration","started_at":"2026-10-11T00:03:20Z","completed_at":"2026-10-11T00:08:00Z"}]}]"#,
    );
    fixture.jobs(102, "older tests", 300);
    fixture.jobs(101, "oldest tests", 180);
}

fn budgeted(budget: &str) -> (Fixture, String) {
    let fixture = Fixture::new();
    ci_fixture(&fixture);
    fixture.write(".safe-push", &format!("[ci]\n\tbudget = {budget}\n"));
    fixture.commit(&["main CI budget"]);
    fixture.push();
    let base = fixture.head();
    (fixture, base)
}

#[test]
fn ci_budget() {
    let (fixture, base) = budgeted("farm-test.yml 240");
    fixture.append("content.txt", "new work\n");
    fixture.commit(&["new work"]);
    fixture.budget().refuses().says("farm-test.yml: median 300s > budget 240s").says("slowest job in most recent run: integration (280s) (checked after the push; the landing stands).");

    fixture.reset(&base);
    fixture.write("notes.md", "prose\n");
    fixture.commit(&["markdown only"]);
    fixture.budget().passes().says("every changed path is Markdown");

    fixture.reset(&base);
    fixture.append("content.txt", "new work\n");
    fixture.commit(&["new work"]);
    fixture.jobs(102, "older tests", 200);
    fixture.budget().passes();

    fixture.jobs(102, "older tests", 300);
    fixture.amend(&["speed up tests", "CI-budget: farm-test.yml 300s -> 200s"]);
    fixture.budget().passes().says("CI budget farm-test.yml exemption");

    fixture.amend(&["fix red main", "Main-red: 104"]);
    fixture.budget().passes().says("main is red at run 104");
    fixture.ci_file("runs.json", &RUNS.replace(r#""databaseId":104,"conclusion":"failure""#, r#""databaseId":104,"conclusion":"success""#));
    fixture.budget().refuses().says("farm-test.yml: median 300s > budget 240s");
    fixture.ci_file("runs.json", RUNS);

    ci_budget_since_a_speed_up(&fixture, &base);
    ci_budget_hold_then_override(&fixture, &base);
    ci_budget_local_source(&fixture, &base);
}

fn ci_budget_since_a_speed_up(fixture: &Fixture, base: &str) {
    fixture.reset(base);
    fixture.append("content.txt", "faster tests\n");
    let speedup = fixture.commit(&["cut farm-test jobs", "CI-budget: farm-test.yml 300s -> 150s", "Co-Authored-By: Claude <noreply@anthropic.com>"]);
    fixture.push();
    let runs: serde_json::Value = serde_json::from_str(RUNS).unwrap();
    let mut rows = vec![serde_json::json!({"databaseId":107,"conclusion":"success","createdAt":"2026-10-11T07:00:00Z","headSha":speedup})];
    rows.extend(runs.as_array().unwrap().iter().map(|run| {
        let mut run = run.clone();
        run["headSha"] = serde_json::Value::from(base);
        run
    }));
    fixture.ci_file("runs.json", &serde_json::Value::from(rows).to_string());
    fixture.jobs(107, "fast tests", 150);
    fixture.append("content.txt", "new work\n");
    fixture.commit(&["new work after speed-up"]);
    fixture.budget().passes().says("measuring 1 main runs since CI-budget commit");
    fixture.ci_file("runs.json", RUNS);
    git(&fixture.repo, &["push", "-q", "origin", &format!("+{base}:refs/heads/main")]);
    git(&fixture.repo, &["fetch", "-q", "origin"]);
}

fn backdate_hold(fixture: &Fixture) -> PathBuf {
    let hold = fs::read_dir(fixture.state()).unwrap().flatten().map(|entry| entry.path()).find(|path| path.to_string_lossy().ends_with("farm-test.yml")).expect("a CI budget hold");
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    fs::write(&hold, format!("{}\n", now - 1900)).unwrap();
    hold
}

fn ci_budget_hold_then_override(fixture: &Fixture, base: &str) {
    fixture.reset(base);
    fixture.append("content.txt", "held work\n");
    fixture.commit(&["work held by an over-budget history"]);
    fixture.budget().refuses().says("farm-test.yml: median 300s > budget 240s").says("each landing records an override instead");
    let hold = backdate_hold(fixture);
    fixture.budget().passes().says("CI budget farm-test.yml override: median 300s > budget 240s");
    let overrides = fs::read_to_string(fixture.state().join("ci-budget-overrides.tsv")).unwrap();
    assert!(overrides.contains("\tfarm-test.yml\t300\t240\t"), "override was not recorded: {overrides}");
    fixture.jobs(102, "older tests", 200);
    fixture.budget().passes();
    assert!(!hold.exists(), "a passing budget left the hold in place");
    fs::rename(fixture.ci().join("102.json"), fixture.ci().join("102.json.gone")).unwrap();
    fixture.budget().passes();
    fs::rename(fixture.ci().join("102.json.gone"), fixture.ci().join("102.json")).unwrap();
    fixture.jobs(102, "older tests", 300);
    fixture.budget().refuses().says("each landing records an override instead");
    fs::rename(fixture.ci().join("runs.json"), fixture.ci().join("runs.offline.json")).unwrap();
    backdate_hold(fixture);
    fixture.budget().passes().says("CI budget farm-test.yml override: gh failed to list completed main runs");
    fs::rename(fixture.ci().join("runs.offline.json"), fixture.ci().join("runs.json")).unwrap();
    fs::remove_file(hold).unwrap();
}

fn ci_budget_local_source(fixture: &Fixture, base: &str) {
    fixture.reset(base);
    fixture.write(".safe-push", "[ci]\n\tbudget = local:timings.tsv 447\n");
    fixture.write("timings.tsv", &format!("{base}\t600\tpassed\tparser (580s)\n{base}\t500\tfailed\tcompiler (480s)\n{base}\t480\tpassed\ttypechecker (450s)\n"));
    fixture.commit(&["local CI budget"]);
    fixture.push();
    fixture.commit(&["new work after local measurement"]);
    fixture.budget().refuses().says("local:timings.tsv: median 500s > budget 447s").says("slowest job in most recent run: typechecker (450s) (checked after the push; the landing stands).");
    fixture.write(".safe-push", "[ci]\n\tbudget = local:timings.tsv 447 600\n");
    fixture.commit(&["soft CI budget with a ceiling"]);
    fixture.push();
    fixture.commit(&["work over a soft CI budget"]);
    fixture.budget().passes().says("CI budget local:timings.tsv debt: median 500s over target 447s, under ceiling 600s");
    let debt = fs::read_to_string(fixture.state().join("ci-budget-debt.tsv")).unwrap();
    assert!(debt.contains("\tlocal:timings.tsv\t500\t447\t600\t"), "CI budget debt was not recorded: {debt}");
    fixture.write(".safe-push", "[ci]\n\tbudget = local:timings.tsv 447 480\n");
    fixture.commit(&["soft CI budget under the median"]);
    fixture.push();
    fixture.commit(&["work over a CI budget ceiling"]);
    fixture.budget().refuses().says("local:timings.tsv: median 500s > budget 447s");
}

#[test]
fn ci_unconfigured() {
    let fixture = Fixture::new();
    fixture.write(".safe-push", "[ci]\n\tbudget = farm-test.yml 240\n");
    fixture.commit(&["propose a budget for future landings"]);
    fixture.budget().passes();
    assert!(!fixture.trace().lines().any(|line| line.starts_with("run ") || line.starts_with("api ")), "unconfigured main queried CI history: {}", fixture.trace());
}

#[test]
fn ci_filtered() {
    let fixture = Fixture::new();
    ci_fixture(&fixture);
    fixture.write(".safe-push", "[ci]\n\tbudget = farm-test.yml 240\n");
    fixture.write(".github/workflows/farm-test.yml", "on:\n  push:\n    branches: [main]\n    paths:\n      - \"src/**\"\njobs: {}\n");
    fixture.commit(&["filtered workflow"]);
    fixture.push();
    fixture.append("content.txt", "new work\n");
    fixture.commit(&["outside the filter"]);
    fixture.budget().passes().says("farm-test.yml exemption — no changed path matches its push paths filter");
    fixture.write("src/deep/main.ts", "code\n");
    fixture.commit(&["inside the filter"]);
    fixture.budget().refuses().says("farm-test.yml: median 300s > budget 240s");
}

#[test]
fn code_health_landed() {
    let fixture = Fixture::new();
    let base = git(&fixture.repo, &["rev-parse", "refs/remotes/origin/main"]);
    let head = fixture.commit(&["outgoing"]);
    let exit = |code: &str| fixture.gate("code-health", &base, &head, &[("CODE_HEALTH_EXIT", code)]);
    exit("0").passes();
    assert!(fixture.trace().lines().any(|line| line == format!("code-health code-health gate {base} {head}")), "code-health did not measure the landed range: {}", fixture.trace());
    exit("1").refuses().says("✗ code-health gate refused");
    exit("2").passes().says("no structure.toml — continuing");
    fixture.write("structure.toml", "[thresholds]\n");
    let enrolled = fixture.commit(&["enroll in code-health"]);
    fixture.gate("code-health", &head, &enrolled, &[("CODE_HEALTH_EXIT", "2")]).refuses().says("code-health gate failed (exit 2) — fix the tool");
    let absent = fixture.root.path().join("absent-code-health");
    fixture.gate("code-health", &head, &enrolled, &[("SAFE_PUSH_CODE_HEALTH", absent.to_str().unwrap())]).refuses().says("code-health missing at");
}
