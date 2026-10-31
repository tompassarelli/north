use std::fs;
use std::path::Path;
use std::process::{Command, Output};

const POINT: &str = "date = \"2026-10-12T07:19:38Z\"\nmodel = \"m\"\nvariant = \"base\"\npolicy_sha256 = \"old\"\nstartup_tokens = 5213\nconformance = 0.5\nn = 4\nevidence = \"e\"\nweak_scenarios = [\"b\"]\n# audit note kept by promote\n";
const CANDIDATE: &str = "date = \"2026-10-29T00:00:00Z\"\nmodel = \"m\"\nvariant = \"lean\"\npolicy_sha256 = \"new\"\nstartup_tokens = 2\nconformance = 1.0\nn = 2\nevidence = \"e\"\n\n[scenarios]\n\"a\" = [1, 1]\n\"b\" = [1, 1]\n";

fn promote(root: &Path, scenarios: &[&str]) -> Output {
    let replay = root.join("agents/replay");
    let mut command = Command::new(env!("CARGO_BIN_EXE_north"));
    command.args(["policy-replay", "promote"]);
    command.args(scenarios.iter().map(|id| replay.join(format!("{id}.json"))));
    command.arg("--candidate").arg(root.join("candidate.toml"));
    command.output().unwrap()
}

fn fixture() -> tempfile::TempDir {
    let scratch = tempfile::tempdir().unwrap();
    let replay = scratch.path().join("agents/replay");
    fs::create_dir_all(&replay).unwrap();
    for id in ["a", "b", "c"] {
        fs::write(replay.join(format!("{id}.json")), format!(r#"{{"id": "{id}", "prompt": "p"}}"#)).unwrap();
    }
    fs::write(scratch.path().join("agents/operating-point.toml"), POINT).unwrap();
    fs::write(scratch.path().join("candidate.toml"), CANDIDATE).unwrap();
    scratch
}

#[test]
fn the_promote_command_refuses_a_missed_scenario_with_exit_1_then_promotes_a_covering_candidate() {
    let scratch = fixture();
    let point = scratch.path().join("agents/operating-point.toml");

    let refused = promote(scratch.path(), &["a", "b", "c"]);
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    assert!(String::from_utf8_lossy(&refused.stderr).contains("does not cover ['c']"), "{refused:?}");
    assert_eq!(fs::read_to_string(&point).unwrap(), POINT);

    let promoted = promote(scratch.path(), &["a", "b"]);
    assert_eq!(promoted.status.code(), Some(0), "{promoted:?}");
    assert!(String::from_utf8_lossy(&promoted.stdout).starts_with("promoted lean -> "), "{promoted:?}");
    let text = fs::read_to_string(&point).unwrap();
    assert!(text.contains("conformance = 1.0") && text.contains("weak_scenarios = [\"b\"]") && text.contains("# audit note kept by promote"), "{text}");
}

#[test]
fn an_unknown_verb_or_flag_exits_2_without_touching_the_point() {
    let scratch = fixture();
    let verb = Command::new(env!("CARGO_BIN_EXE_north")).args(["policy-replay", "sweep"]).output().unwrap();
    assert_eq!(verb.status.code(), Some(2), "{verb:?}");
    let flag = Command::new(env!("CARGO_BIN_EXE_north")).args(["policy-replay", "promote", "a.json", "--bogus"]).output().unwrap();
    assert_eq!(flag.status.code(), Some(2), "{flag:?}");
    assert_eq!(fs::read_to_string(scratch.path().join("agents/operating-point.toml")).unwrap(), POINT);
}
