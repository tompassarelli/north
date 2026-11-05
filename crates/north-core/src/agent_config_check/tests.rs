use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};
use tempfile::TempDir;

use crate::platform::rolling_issue::{Issue, Tracker};

use super::advisory::record;
use super::gate::{Gate, Mode, hex};
use super::roots::Roots;
use super::rules::rule_register;
use super::tree::tree_policy;
use super::programs::skill_programs;
use super::vocabulary::harness_vocabulary;

const FULL: &str = r#"{"force-push": [3, 3], "other": [3, 3]}"#;

struct Fixture {
    dir: TempDir,
    operator: PathBuf,
    sha: String,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let operator = dir.path().join("agents");
        fs::create_dir_all(operator.join("replay")).unwrap();
        fs::create_dir_all(dir.path().join("replay")).unwrap();
        fs::create_dir_all(dir.path().join("wisp")).unwrap();
        fs::write(operator.join("replay/force-push.json"), "{\"id\": \"force-push\"}\n").unwrap();
        fs::write(operator.join("replay/other.json"), "{\"id\": \"other\"}\n").unwrap();
        fs::write(dir.path().join("wisp/AGENTS.md"), "# Wisp\n").unwrap();
        fs::write(
            operator.join("catalog-config.json"),
            r#"{"activation": {}, "registrations": {}, "baselines": [{"targets": ["shared"], "owner": {"repo": "south", "path": "agents/AGENTS.md"}}]}"#,
        )
        .unwrap();
        fs::write(operator.join("orchestration.toml"), "[projects.wisp]\n").unwrap();
        fs::write(operator.join("AGENTS.md"), "# Policy\n\n## Rules\n\n- Keep it short.\n").unwrap();
        let sha = hex(&Sha256::digest(fs::read(operator.join("AGENTS.md")).unwrap()));
        Self { dir, operator, sha }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn roots(&self) -> Roots {
        Roots::parse(self.dir.path().to_path_buf(), Some(&format!(r#"{{"wisp": "{}"}}"#, self.path("wisp").display())))
    }

    fn gate(&self) -> Gate {
        Gate {
            repo: self.dir.path().to_path_buf(),
            operator: self.operator.clone(),
            replay: self.path("replay"),
            start: Box::new(|fixture| Ok(if fixture { 400 } else { 800 })),
            roots: self.roots(),
        }
    }

    fn point(&self, tokens: &str, conformance: &str, n: usize) {
        fs::write(
            self.operator.join("operating-point.toml"),
            format!("date = \"2026-10-12\"\nmodel = \"m\"\nvariant = \"full\"\npolicy_sha256 = \"p\"\nstartup_tokens = {tokens}\nconformance = {conformance}\nn = {n}\nevidence = \"e\"\n"),
        )
        .unwrap();
    }

    fn row(&self, conformance: &str, n: usize, model: &str, scenarios: &str) {
        let point = r#"{"variant": "point", "policy_sha256": "p", "model": "m", "conformance": 1.0, "n": 6, "scenarios": {"force-push": [3, 3], "other": [3, 3]}}"#;
        let live = format!(
            r#"{{"variant": "v", "policy_sha256": "{}", "model": "{model}", "startup_tokens": 1, "conformance": {conformance}, "n": {n}, "scenarios": {scenarios}}}"#,
            self.sha
        );
        fs::write(self.operator.join("replay/sweep-2026-10-12.jsonl"), format!("{point}\n{live}\n")).unwrap();
    }

    fn session(&self) -> (bool, String) {
        let verdicts = self.gate().run(Mode::Session).unwrap();
        (verdicts.passed, verdicts.text)
    }

    fn refuses(&self, expected: &str) {
        let (passed, text) = self.session();
        assert!(!passed && text.contains(expected), "expected a refusal containing {expected:?}, got passed={passed}: {text}");
    }

    fn accepts(&self, expected: &str) {
        let (passed, text) = self.session();
        assert!(passed && text.contains(expected), "expected a pass containing {expected:?}, got passed={passed}: {text}");
    }

    fn tokens(&self) -> String {
        let text = self.gate().run(Mode::Measure).unwrap().text;
        text.lines().next().unwrap().trim_start_matches("session start ").trim_end_matches(" tokens").to_owned()
    }
}

#[test]
fn gate_refuses_without_an_operating_point() {
    Fixture::new().refuses("no operating point; run the sweep and promote one");
}

#[test]
fn gate_refuses_a_live_policy_with_no_sweep_row() {
    let fixture = Fixture::new();
    fixture.point("1000000000", "0.8", 100);
    fixture.refuses("the live policy has no sweep row");
}

#[test]
fn gate_refuses_startup_growth_without_a_conformance_gain() {
    let fixture = Fixture::new();
    fixture.point("0", "0.8", 100);
    fixture.row("0.8", 100, "m", FULL);
    fixture.refuses("startup tokens grew 0→");
}

#[test]
fn gate_refuses_a_sweep_row_from_another_model() {
    let fixture = Fixture::new();
    fixture.point("0", "0.8", 100);
    fixture.row("0.95", 100, "other-model", FULL);
    fixture.refuses("sweep row ran on other-model, the operating point on m");
}

#[test]
fn gate_passes_a_matching_row_and_prints_the_interval() {
    let fixture = Fixture::new();
    fixture.point("0", "0.8", 100);
    fixture.row("0.95", 100, "m", FULL);
    fixture.accepts("operating point full 0 at 2026-10-12, conformance 0.8 [0.711, 0.867] n=100");
}

#[test]
fn gate_refuses_a_significant_conformance_drop() {
    let fixture = Fixture::new();
    fixture.point("1000000000", "0.8", 100);
    fixture.row("0.6", 100, "m", FULL);
    fixture.refuses("replay conformance dropped 0.8→0.6");
}

#[test]
fn gate_refuses_a_scenario_that_flipped_to_zero_passes() {
    let fixture = Fixture::new();
    fixture.point("1000000000", "0.8", 100);
    fixture.row("0.95", 100, "m", r#"{"force-push": [0, 3], "other": [3, 3]}"#);
    fixture.refuses("scenario force-push flipped to 0 passes");
}

#[test]
fn gate_drop_is_a_one_sided_fisher_test_at_n_42() {
    let fixture = Fixture::new();
    fixture.point("1000000000", "1.0", 42);
    fixture.row("0.9524", 42, "m", FULL);
    fixture.accepts("conformance 1 [0.916, 1.000] n=42");
    fixture.row("0.8333", 42, "m", FULL);
    fixture.refuses("replay conformance dropped 1→0.8333");
    fixture.point("1000000000", "0.881", 42);
    fixture.row("0.619", 42, "m", FULL);
    fixture.refuses("replay conformance dropped 0.881→0.619");
    fixture.row("0.857", 42, "m", FULL);
    fixture.accepts("conformance 0.881");
}

#[test]
fn gate_refuses_a_row_missing_trials_on_a_scenario() {
    let fixture = Fixture::new();
    fixture.point("1000000000", "0.881", 42);
    fixture.row("0.857", 42, "m", r#"{"force-push": [3, 3], "other": [2, 2]}"#);
    fixture.refuses(&format!("sweep row for {} covers 1 of 2 scenarios", &fixture.sha[..8]));
}

#[test]
fn gate_ignores_project_growth_later_short_rows_and_unmeasured_scenarios() {
    let fixture = Fixture::new();
    let tokens = fixture.tokens();
    fixture.point(&tokens, "0.881", 42);
    fixture.row("0.881", 42, "m", FULL);
    fs::write(fixture.path("wisp/AGENTS.md"), "# Wisp\nTwelve chars").unwrap();
    fixture.accepts(&format!("session start {tokens} tokens"));
    fs::write(
        fixture.path("replay/sweep-2026-10-19.jsonl"),
        format!(r#"{{"variant": "v", "policy_sha256": "{}", "model": "m", "startup_tokens": 1, "conformance": 1.0, "n": 2, "scenarios": {{"force-push": [1, 1], "other": [1, 1]}}}}"#, fixture.sha) + "\n",
    )
    .unwrap();
    fs::write(fixture.operator.join("replay/new-scenario.json"), "{\"id\": \"new-scenario\"}\n").unwrap();
    fixture.accepts(&format!("session start {tokens} tokens"));
}

#[test]
fn gate_measures_the_session_start_from_the_policy_and_the_probe() {
    let fixture = Fixture::new();
    let policy = fs::read_to_string(fixture.operator.join("AGENTS.md")).unwrap().chars().count() + "<!-- south:agents/AGENTS.md -->\n\n\n".chars().count();
    assert_eq!(fixture.tokens(), ((policy + 400) / 4).to_string());
}

fn git(directory: &Path, args: &[&str]) -> String {
    let output = Command::new("git").arg("-C").arg(directory).args(["-c", "user.name=t", "-c", "user.email=t@t"]).args(args).output().unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

struct Register {
    fixture: Fixture,
}

impl Register {
    fn new() -> Self {
        let fixture = Fixture::new();
        let operator = &fixture.operator;
        fs::write(operator.join("rules.toml"), "[[rule]]\nid = \"keep-short\"\nsection = \"Rules\"\ntext = \"Keep it short.\"\nplacement = \"global\"\nfixes = \"Long policy.\"\nincidents = []\n").unwrap();
        fs::write(
            operator.join("incidents.jsonl"),
            "{\"id\": \"inc-1\", \"session\": \"s1\"}\n{\"id\": \"inc-2\", \"session\": \"s1\"}\n{\"id\": \"inc-3\", \"session\": null, \"ref\": \"north#5\"}\n",
        )
        .unwrap();
        git(operator, &["init", "-q"]);
        git(operator, &["add", "rules.toml"]);
        git(operator, &["commit", "-qm", "base"]);
        assert!(rule_register(operator, &fixture.path("roles"), &fixture.roots()).unwrap().passed);
        let mut policy = fs::read_to_string(operator.join("AGENTS.md")).unwrap();
        policy.push_str("\n- Cite incidents.\n");
        fs::write(operator.join("AGENTS.md"), policy).unwrap();
        Self { fixture }
    }

    fn cite(&self, incidents: &str, roots: Roots) -> (bool, String) {
        let operator = &self.fixture.operator;
        let base = fs::read_to_string(operator.join("rules.toml")).unwrap();
        let base = base.split("\n[[rule]]\nid = \"cite\"").next().unwrap().to_owned();
        fs::write(
            operator.join("rules.toml"),
            format!("{base}\n[[rule]]\nid = \"cite\"\nsection = \"Rules\"\ntext = \"Cite incidents.\"\nplacement = \"global\"\nfixes = \"Unearned rules.\"\nincidents = {incidents}\n"),
        )
        .unwrap();
        let verdicts = rule_register(operator, &self.fixture.path("roles"), &roots).unwrap();
        (verdicts.passed, verdicts.text)
    }

    fn refused(&self, incidents: &str, roots: Roots) {
        let (passed, text) = self.cite(incidents, roots);
        assert!(!passed && text.contains("cite: a new rule must cite incidents.jsonl incidents from at least 2 sessions"), "accepted an unearned rule: {text}");
    }
}

#[test]
fn rules_new_rule_needs_incidents_from_two_sessions() {
    let register = Register::new();
    register.refused(r#"["inc-1", "inc-9"]"#, register.fixture.roots());
    register.refused(r#"["inc-1", "inc-2"]"#, register.fixture.roots());
    let (passed, text) = register.cite(r#"["inc-1", "inc-3"]"#, register.fixture.roots());
    assert!(passed, "{text}");
}

#[test]
fn rules_count_commits_and_issues_citing_one_issue_once() {
    let register = Register::new();
    let work = register.fixture.path("work");
    fs::create_dir_all(&work).unwrap();
    git(&work, &["init", "-q"]);
    for n in 1..=2 {
        git(&work, &["commit", "-q", "--allow-empty", "-m", &format!("fix {n} (work#7)")]);
    }
    let (first, second) = (git(&work, &["rev-parse", "HEAD~1"]), git(&work, &["rev-parse", "HEAD"]));
    let ledger = register.fixture.operator.join("incidents.jsonl");
    let mut body = fs::read_to_string(&ledger).unwrap();
    body.push_str(&format!("{{\"id\": \"inc-4\", \"source\": \"git\", \"session\": null, \"ref\": \"work@{first}\"}}\n{{\"id\": \"inc-5\", \"source\": \"github\", \"session\": null, \"ref\": \"t/work#7\"}}\n{{\"id\": \"inc-6\", \"source\": \"git\", \"session\": null, \"ref\": \"work@{second}\"}}\n"));
    fs::write(&ledger, body).unwrap();
    let roots = || Roots::parse(register.fixture.dir.path().to_path_buf(), Some(&format!(r#"{{"work": "{}"}}"#, work.display())));
    register.refused(r#"["inc-4", "inc-6", "inc-5"]"#, roots());
}

#[test]
fn rules_must_match_agents_bullets_one_to_one() {
    let fixture = Fixture::new();
    fs::write(
        fixture.operator.join("rules.toml"),
        "[[rule]]\nid = \"other\"\nsection = \"Rules\"\ntext = \"Something else.\"\nplacement = \"global\"\nfixes = \"x\"\nincidents = []\n",
    )
    .unwrap();
    let verdicts = rule_register(&fixture.operator, &fixture.path("roles"), &fixture.roots()).unwrap();
    assert!(!verdicts.passed);
    assert!(verdicts.text.contains("AGENTS.md bullet has no rules.toml entry: [Rules] Keep it short."), "{}", verdicts.text);
    assert!(verdicts.text.contains("rules.toml entry has no AGENTS.md bullet: [Rules] Something else."), "{}", verdicts.text);
}

#[test]
fn rules_placed_in_a_role_file_need_its_text_there() {
    let fixture = Fixture::new();
    let roles = fixture.path("roles");
    fs::create_dir_all(&roles).unwrap();
    fs::write(
        fixture.operator.join("rules.toml"),
        "[[rule]]\nid = \"keep-short\"\nsection = \"Rules\"\ntext = \"Keep it short.\"\nplacement = \"global\"\nfixes = \"x\"\nincidents = []\n\n[[rule]]\nid = \"names\"\nsection = \"Rules\"\ntext = \"Plato is the proxy.\"\nplacement = \"agent:proxy\"\nfixes = \"x\"\nincidents = []\n",
    )
    .unwrap();
    let verdicts = rule_register(&fixture.operator, &roles, &fixture.roots()).unwrap();
    assert!(!verdicts.passed && verdicts.text.contains("rules.toml names: agent:proxy names no role file proxy.md"), "{}", verdicts.text);
    fs::write(roles.join("proxy.md"), "---\nname: proxy\n---\n\n- Plato is someone else.\n").unwrap();
    let verdicts = rule_register(&fixture.operator, &roles, &fixture.roots()).unwrap();
    assert!(!verdicts.passed && verdicts.text.contains("rules.toml names: role file proxy.md does not hold its text"), "{}", verdicts.text);
    fs::write(roles.join("proxy.md"), "---\nname: proxy\n---\n\n- Plato is the proxy.\n").unwrap();
    let verdicts = rule_register(&fixture.operator, &roles, &fixture.roots()).unwrap();
    assert!(verdicts.passed, "{}", verdicts.text);
}

#[test]
fn proxy_role_file_allows_only_read_only_subagents() {
    let role = include_str!("../../../../harness/claude/agents/proxy.md");
    let tools = role.lines().find_map(|line| line.strip_prefix("tools:")).expect("proxy.md declares its tools");
    assert!(tools.contains("Agent(Explore, Plan)"), "{tools}");
    assert_eq!(tools.split([',', ' ']).filter(|tool| tool.starts_with("Agent")).count(), 1, "{tools}");
}

struct Vocabulary {
    dir: TempDir,
}

impl Vocabulary {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q"]);
        fs::write(dir.path().join("README.md"), "# Repo\n\nThe harness is under harness/hooks/.\n").unwrap();
        fs::write(dir.path().join("history.jsonl"), "{\"ref\": \"north:socrates/hooks/x.sh\"}\n").unwrap();
        git(dir.path(), &["add", "README.md", "history.jsonl"]);
        Self { dir }
    }

    fn check(&self) -> (String, bool) {
        harness_vocabulary(&[self.dir.path().to_path_buf()])
    }

    fn add(&self, file: &str, body: &str) {
        let path = self.dir.path().join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, body).unwrap();
        git(self.dir.path(), &["add", file]);
    }
}

#[test]
fn vocabulary_passes_harness_paths_and_history_lines() {
    let vocabulary = Vocabulary::new();
    let (text, clean) = vocabulary.check();
    assert!(clean, "{text}");
}

#[test]
fn vocabulary_refuses_a_socrates_path() {
    let vocabulary = Vocabulary::new();
    vocabulary.add("socrates/hook.sh", "x\n");
    let (text, clean) = vocabulary.check();
    assert!(!clean && text.contains("lives under harness/, not socrates/: socrates/hook.sh"), "{text}");
}

#[test]
fn vocabulary_refuses_a_socrates_caller() {
    let vocabulary = Vocabulary::new();
    vocabulary.add("notes.md", "Run socrates/scripts/check.sh.\n");
    let (text, clean) = vocabulary.check();
    assert!(!clean && text.contains("notes.md"), "{text}");
}

#[test]
fn vocabulary_refuses_socrates_called_the_agent_machinery() {
    let vocabulary = Vocabulary::new();
    vocabulary.add("notes.md", "Socrates is the agent machinery beside North.\n");
    let (text, clean) = vocabulary.check();
    assert!(!clean && text.contains("notes.md"), "{text}");
}

#[test]
fn shellcheck_comes_from_path_when_installed() {
    let command = super::shared::shellcheck_command(Some(PathBuf::from("/usr/bin/shellcheck")), Path::new("/tree"));
    assert_eq!(command.get_program(), "/usr/bin/shellcheck");
    assert_eq!(command.get_args().count(), 0);
}

#[test]
fn shellcheck_falls_back_to_the_north_flake_when_absent_from_path() {
    let command = super::shared::shellcheck_command(None, Path::new("/tree"));
    let arguments: Vec<_> = command.get_args().map(|argument| argument.to_string_lossy().into_owned()).collect();
    assert_eq!(command.get_program(), "nix");
    assert_eq!(arguments, ["shell", "--quiet", "path:/tree#shellcheck", "-c", "shellcheck"]);
}

#[test]
fn hooks_fail_closed_when_neither_path_nor_the_flake_supplies_shellcheck() {
    let north = tempfile::tempdir().unwrap();
    fs::create_dir_all(north.path().join("harness/hooks")).unwrap();
    fs::write(north.path().join("harness/hooks/guard.sh"), "#!/bin/sh\nexit 0\n").unwrap();
    let mut report = super::report::Report::new(false);
    let linted = super::shared::lint_hooks(north.path(), None, &mut report);
    assert_eq!((linted, report.fail), (1, 1));
    assert!(report.failures[0].starts_with("shellcheck"), "{:?}", report.failures);
}

#[test]
fn hooks_fail_closed_when_none_are_found() {
    let north = tempfile::tempdir().unwrap();
    let mut report = super::report::Report::new(false);
    let linted = super::shared::lint_hooks(north.path(), Some(PathBuf::from("/usr/bin/shellcheck")), &mut report);
    assert_eq!((linted, report.fail), (0, 1));
    assert!(report.failures[0].contains("no shared hooks") && report.failures[0].contains("harness/hooks"), "{:?}", report.failures);
}


#[derive(Default)]
struct Issues {
    open: Option<(u64, String)>,
    created: Vec<(String, String)>,
    edited: Vec<(u64, String)>,
    closed: Vec<u64>,
}

impl Tracker for Issues {
    fn open(&self, _: &str) -> Result<Option<Issue>, String> {
        Ok(self.open.as_ref().map(|(number, body)| Issue { number: *number, body: body.clone() }))
    }

    fn body(&self, number: u64) -> Result<String, String> {
        Err(format!("issue {number} is not read here"))
    }

    fn listed(&self, _: &str) -> Result<Vec<(u64, String)>, String> {
        Ok(Vec::new())
    }

    fn create(&mut self, title: &str, body: &str) -> Result<(), String> {
        self.created.push((title.into(), body.into()));
        self.open = Some((7, body.into()));
        Ok(())
    }

    fn edit(&mut self, number: u64, body: &str) -> Result<(), String> {
        self.edited.push((number, body.into()));
        self.open = Some((number, body.into()));
        Ok(())
    }

    fn close(&mut self, number: u64, _: &str) -> Result<(), String> {
        self.closed.push(number);
        self.open = None;
        Ok(())
    }
}

fn failing(text: &str) -> Vec<String> {
    vec![text.to_owned()]
}

#[test]
fn advisory_opens_one_issue_on_the_second_failing_run_in_a_row_and_updates_then_closes_it() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state/agent-config-check.json");
    let mut issues = Issues::default();
    record(&mut issues, &state, "gex45", "run-1", &failing("shared: shellcheck is required")).unwrap();
    assert!(issues.created.is_empty(), "one failure is not noticed yet");
    record(&mut issues, &state, "gex45", "run-1", &failing("shared: shellcheck is required")).unwrap();
    assert!(issues.created.is_empty(), "the same run recorded twice counts once");
    record(&mut issues, &state, "gex45", "run-2", &failing("shared: shellcheck is required")).unwrap();
    assert_eq!(issues.created.len(), 1);
    assert_eq!(issues.created[0].0, "Agent configuration check failing on gex45");
    assert!(issues.created[0].1.contains("- shared: shellcheck is required\n"));
    record(&mut issues, &state, "gex45", "run-3", &failing("shared: shellcheck is required")).unwrap();
    assert!(issues.created.len() == 1 && issues.edited.is_empty(), "an unchanged failure set touches nothing");
    record(&mut issues, &state, "gex45", "run-4", &failing("rules: diverge")).unwrap();
    assert_eq!(issues.edited.len(), 1);
    assert!(issues.edited[0].1.contains("- rules: diverge\n") && !issues.edited[0].1.contains("shellcheck"));
    record(&mut issues, &state, "gex45", "run-5", &[]).unwrap();
    assert_eq!(issues.closed, [7]);
}

#[test]
fn advisory_a_passing_run_between_failures_resets_the_count() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("agent-config-check.json");
    let mut issues = Issues::default();
    record(&mut issues, &state, "gex45", "run-1", &failing("a")).unwrap();
    record(&mut issues, &state, "gex45", "run-2", &[]).unwrap();
    record(&mut issues, &state, "gex45", "run-3", &failing("a")).unwrap();
    assert!(issues.created.is_empty());
}

fn skill_fixture(fixture: &Fixture, name: &str, bytes: usize) {
    let skill = format!("---\nname: {name}\ndescription: A fixture skill.\n---\n# Fixture\n");
    let text = format!("{skill}{}", "x".repeat(bytes - skill.len()));
    fs::create_dir_all(fixture.path(&format!("skills/{name}"))).unwrap();
    fs::write(fixture.path(&format!("skills/{name}/SKILL.md")), &text).unwrap();
    let mut catalog: serde_json::Value = serde_json::from_str(&fs::read_to_string(fixture.operator.join("catalog-config.json")).unwrap()).unwrap();
    catalog["activation"][name] = serde_json::json!({"distributions": [{"type": "skill", "targets": ["shared"]}]});
    catalog["registrations"][name] = serde_json::json!({"owner": {"repo": "firn", "path": format!("skills/{name}/SKILL.md")}});
    fs::write(fixture.operator.join("catalog-config.json"), catalog.to_string()).unwrap();
}

#[test]
fn a_skill_over_six_kilobytes_fails_the_budget_naming_it_and_one_at_the_limit_passes() {
    let fixture = Fixture::new();
    skill_fixture(&fixture, "at-limit", 6144);
    assert!(fixture.gate().run(Mode::Budget).unwrap().passed);
    skill_fixture(&fixture, "regrown", 6145);
    let verdicts = fixture.gate().run(Mode::Budget).unwrap();
    assert!(!verdicts.passed && verdicts.text.contains("regrown: SKILL.md is 6145 bytes"), "{}", verdicts.text);
    assert!(!verdicts.text.contains("at-limit"), "{}", verdicts.text);
}

fn tree_skill(tree: &Path, root: &str, name: &str, bytes: usize) {
    let skill = format!("---\nname: {name}\ndescription: A fixture skill.\n---\n# Fixture\n");
    let directory = tree.join(root).join(name);
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("SKILL.md"), format!("{skill}{}", "x".repeat(bytes - skill.len()))).unwrap();
}

#[test]
fn tree_policy_refuses_a_skill_one_byte_over_the_limit_with_the_sync_message_and_passes_at_the_limit() {
    let tree = tempfile::tempdir().unwrap();
    tree_skill(tree.path(), "harness/skills", "at-limit", 6144);
    tree_skill(tree.path(), "agent-machinery/skills", "also-at-limit", 6144);
    assert!(tree_policy(tree.path()).passed);
    tree_skill(tree.path(), "harness/skills", "regrown", 6145);
    let verdicts = tree_policy(tree.path());
    assert!(!verdicts.passed && verdicts.text.contains("regrown: SKILL.md is 6145 bytes; policy keeps each SKILL.md at most 6144"), "{}", verdicts.text);
    assert!(!verdicts.text.contains("at-limit"), "{}", verdicts.text);
}

#[test]
fn tree_policy_refuses_a_tree_with_no_skills() {
    let tree = tempfile::tempdir().unwrap();
    assert!(!tree_policy(tree.path()).passed);
}

#[test]
fn tree_policy_passes_this_repository_tree() {
    let tree = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let verdicts = tree_policy(&tree);
    assert!(verdicts.passed, "{}", verdicts.text);
}

#[test]
fn tree_policy_refuses_a_skill_with_paths_frontmatter_because_file_skill_loads_language_skills() {
    let tree = tempfile::tempdir().unwrap();
    tree_skill(tree.path(), "harness/skills", "listed", 200);
    assert!(tree_policy(tree.path()).passed);
    let directory = tree.path().join("harness/skills/scoped");
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("SKILL.md"), "---\nname: scoped\ndescription: A fixture skill.\npaths:\n  - \"**/*.rs\"\n---\n# Fixture\n").unwrap();
    let verdicts = tree_policy(tree.path());
    assert!(!verdicts.passed && verdicts.text.contains("scoped: SKILL.md sets paths:"), "{}", verdicts.text);
    assert!(!verdicts.text.contains("listed"), "{}", verdicts.text);
}

fn gex45_with(programs: &[&str]) -> (TempDir, PathBuf, Vec<super::programs::HostClass>) {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("gex45-bin");
    fs::create_dir_all(&bin).unwrap();
    for program in programs {
        let file = bin.join(program);
        crate::foundation::shim::write_script(&file, "");
    }
    let host = super::programs::HostClass { name: "gex45".into(), bins: vec![bin.clone()] };
    (dir, bin, vec![host])
}

fn machine_capacity_skill(root: &Path, text: &str) {
    let skill = root.join("harness/skills/machine-capacity");
    fs::create_dir_all(&skill).unwrap();
    fs::write(skill.join("SKILL.md"), text).unwrap();
}

#[test]
fn skill_programs_refuse_the_machine_capacity_line_694b2a05_ran_and_pass_once_it_is_installed_or_renamed() {
    let committed = include_str!("fixtures/machine-capacity-f1745de2e.md");
    let (dir, bin, hosts) = gex45_with(&["bun", "north"]);
    machine_capacity_skill(dir.path(), committed);
    let runtime = dir.path().join("runtime-bin");

    let refused = skill_programs(dir.path(), &runtime, &hosts);
    assert!(!refused.passed, "{}", refused.text);
    assert!(refused.text.contains("harness/skills/machine-capacity/SKILL.md:18: machine-capacity is on no PATH of gex45"), "{}", refused.text);

    crate::foundation::shim::write_script(&bin.join("machine-capacity"), "");
    let after_install = skill_programs(dir.path(), &runtime, &hosts);
    assert!(after_install.passed, "{}", after_install.text);
    fs::remove_file(bin.join("machine-capacity")).unwrap();

    let renamed = committed.replace("`machine-capacity run ", "`bun run ").replace("`machine-capacity estimate ", "`bun estimate ");
    machine_capacity_skill(dir.path(), &renamed);
    let after_rename = skill_programs(dir.path(), &runtime, &hosts);
    assert!(after_rename.passed, "{}", after_rename.text);
}

#[test]
fn skill_programs_resolve_in_the_runtime_bin_and_skip_skill_names() {
    let (dir, _, hosts) = gex45_with(&[]);
    let runtime = dir.path().join("runtime-bin");
    fs::create_dir_all(&runtime).unwrap();
    machine_capacity_skill(dir.path(), "# Skill\n\nRun `safe-push --to main` after `external-code` review.\nUse `external-code`.\n");
    fs::create_dir_all(dir.path().join("harness/skills/external-code")).unwrap();
    assert!(!skill_programs(dir.path(), &runtime, &hosts).passed);
    let tool = runtime.join("safe-push");
    crate::foundation::shim::write_script(&tool, "");
    let verdicts = skill_programs(dir.path(), &runtime, &hosts);
    assert!(verdicts.passed, "{}", verdicts.text);
}

#[test]
fn skill_programs_skip_programs_a_skill_forbids() {
    let (dir, _, hosts) = gex45_with(&[]);
    machine_capacity_skill(dir.path(), "# Skill\n\n- Never use `ydotool` on the normal desktop.\n");
    let verdicts = skill_programs(dir.path(), &dir.path().join("runtime-bin"), &hosts);
    assert!(verdicts.passed, "{}", verdicts.text);
}
