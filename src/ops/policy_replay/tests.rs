use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;

use super::detect::{compile, detect, offered_command};
use super::events::{advisor_debug, advisor_usage, meter, parse_events, usage};
use super::json::Json;
use super::point::{PromoteArgs, promote, write_candidate};
use super::policy::{Variant, arm_minus, policy_for_arm, variant_policy};
use super::pyfmt::float_repr;
use super::rows::{RunRow, Scenarios, VariantRow, summary, variant_row};
use super::stats::{compare, operating_point, Verdict};
use crate::error::NorthError;

const ADVISOR_RUN: &str = include_str!("../../../harness/scripts/policy-replay.advisor.jsonl");
const PROBE_STREAM: &str = include_str!("../../../harness/scripts/fixtures/haiku-opus-advisor.stream.jsonl");
const PROBE_DEBUG: &str = include_str!("../../../harness/scripts/fixtures/haiku-opus-advisor.debug-advisor-lines.log");
const POINT: &str = "date = \"2026-10-12T07:19:38Z\"\nmodel = \"m\"\nvariant = \"base\"\npolicy_sha256 = \"old\"\nstartup_tokens = 5213\nconformance = 0.5\nn = 4\nevidence = \"e\"\nweak_scenarios = [\"b\"]\n# audit note kept by promote\n";

fn cells(pairs: &[(&str, [usize; 2])]) -> Scenarios {
    pairs.iter().map(|(name, cell)| ((*name).to_owned(), *cell)).collect()
}

fn variant(name: &str, startup: usize, conformance: f64, n: usize, scenarios: Scenarios) -> VariantRow {
    VariantRow {
        variant: name.to_owned(),
        policy_sha256: name.to_owned(),
        startup_tokens: startup,
        conformance: Some(conformance),
        n,
        scenarios,
        errors: 0,
        runs_tokens: None,
        usd_or_null: None,
        wall_s: 0.0,
        model: Some("m".into()),
        effort: None,
        claude_version: None,
    }
}

fn scenario_files(dir: &Path, ids: &[&str]) -> Vec<PathBuf> {
    ids.iter()
        .map(|id| {
            let path = dir.join(format!("{id}.json"));
            fs::write(&path, json!({"id": id, "prompt": "p", "detector": {"kind": "regex", "pattern": "ok"}}).to_string()).unwrap();
            path
        })
        .collect()
}

fn candidate_file(dir: &Path, name: &str, conformance: &str, n: usize, scenarios: &[(&str, &str)], evidence: &Path) -> PathBuf {
    let path = dir.join(format!("{name}.toml"));
    let cells: String = scenarios.iter().map(|(scenario, cell)| format!("\"{scenario}\" = {cell}\n")).collect();
    let text = format!(
        "date = \"{name}\"\nmodel = \"m\"\nvariant = \"{name}\"\npolicy_sha256 = \"{name}\"\nstartup_tokens = 1\nconformance = {conformance}\nn = {n}\nevidence = \"{}\"\n\n[scenarios]\n{cells}",
        evidence.display()
    );
    fs::write(&path, text).unwrap();
    path
}

fn promote_with(candidate: &Path, scenarios: &[PathBuf], point: &Path, accept_drop: Option<&str>) -> Result<String, NorthError> {
    promote(&PromoteArgs { candidate, scenarios, operating_point: Some(point), accept_drop })
}

#[test]
fn promote_refuses_a_candidate_that_misses_a_scenario() {
    let dir = tempfile::tempdir().unwrap();
    let scenarios = scenario_files(dir.path(), &["a", "b", "c"]);
    let point = dir.path().join("operating-point.toml");
    fs::write(&point, POINT).unwrap();
    let candidate = dir.path().join("candidate.toml");
    let row = variant("base", 2, 1.0, 2, cells(&[("a", [1, 1]), ("b", [1, 1])]));
    write_candidate(&candidate, &row, Path::new("e"), false, "2026-10-29T00:00:00Z").unwrap();
    let error = promote_with(&candidate, &scenarios, &point, None).unwrap_err().to_string();
    assert!(error.contains("does not cover ['c']"), "{error}");
    assert_eq!(fs::read_to_string(&point).unwrap(), POINT);
}

#[test]
fn promote_refuses_a_significant_drop_then_updates_keeping_weak_scenarios_and_comments() {
    let dir = tempfile::tempdir().unwrap();
    let scenarios = scenario_files(dir.path(), &["a", "b"]);
    let point = dir.path().join("operating-point.toml");
    fs::write(&point, POINT).unwrap();
    let candidate = dir.path().join("candidate.toml");
    let row = variant("base", 2, 1.0, 2, cells(&[("a", [1, 1]), ("b", [1, 1])]));
    write_candidate(&candidate, &row, Path::new("e"), false, "2026-10-29T00:00:00Z").unwrap();
    let dropped = dir.path().join("drop.toml");
    let drop_text = fs::read_to_string(&candidate).unwrap().replace("conformance = 1.0", "conformance = 0.0").replace("n = 2", "n = 40");
    fs::write(&dropped, drop_text).unwrap();
    let refusal = promote_with(&dropped, &scenarios, &point, None).unwrap_err().to_string();
    assert!(refusal.contains("significantly below"), "{refusal}");
    assert_eq!(fs::read_to_string(&point).unwrap(), POINT);

    let message = promote_with(&candidate, &scenarios, &point, None).unwrap();
    assert!(message.starts_with("promoted base -> "), "{message}");
    assert!(message.ends_with(": conformance 0.5 -> 1.0, startup tokens 5213 -> 2"), "{message}");
    let text = fs::read_to_string(&point).unwrap();
    let expected = "anchor_conformance = 0.5\nanchor_n = 4\nanchor_date = \"2026-10-12T07:19:38Z\"\ndate = \"2026-10-29T00:00:00Z\"\nmodel = \"m\"\nvariant = \"base\"\npolicy_sha256 = \"base\"\nstartup_tokens = 2\nconformance = 1.0\nn = 2\nevidence = \"e\"\nweak_scenarios = [\"b\"]\n# audit note kept by promote\n";
    assert_eq!(text, expected);
}

#[test]
fn promote_refuses_a_flip_and_a_ratchet_against_the_anchor_then_accept_drop_reanchors() {
    let dir = tempfile::tempdir().unwrap();
    let scenarios = scenario_files(dir.path(), &["a", "b"]);
    let evidence = dir.path().join("ev.jsonl");
    fs::write(&evidence, json!({"policy_sha256": "v0", "scenarios": {"a": [3, 3], "b": [3, 3]}}).to_string() + "\n").unwrap();
    let chain = dir.path().join("chain.toml");
    fs::write(&chain, format!("date = \"d0\"\nmodel = \"m\"\nvariant = \"v0\"\npolicy_sha256 = \"v0\"\nstartup_tokens = 1\nconformance = 0.881\nn = 42\nevidence = \"{}\"\n", evidence.display())).unwrap();

    let flip = candidate_file(dir.path(), "flip", "0.881", 42, &[("a", "[0, 3]"), ("b", "[3, 3]")], &evidence);
    let error = promote_with(&flip, &scenarios, &chain, None).unwrap_err().to_string();
    assert!(error.contains("flips a to 0 passes"), "{error}");

    let step = candidate_file(dir.path(), "s1", "0.7143", 42, &[("a", "[3, 3]"), ("b", "[2, 3]")], &evidence);
    promote_with(&step, &scenarios, &chain, None).unwrap();
    let ratchet = candidate_file(dir.path(), "s2", "0.6905", 42, &[("a", "[3, 3]"), ("b", "[2, 3]")], &evidence);
    let error = promote_with(&ratchet, &scenarios, &chain, None).unwrap_err().to_string();
    assert!(error.contains("below the anchor of d0 0.881"), "{error}");

    promote_with(&ratchet, &scenarios, &chain, Some("model release")).unwrap();
    let text = fs::read_to_string(&chain).unwrap();
    assert!(text.contains("accepted_drop = \"model release\"\n") && text.contains("anchor_conformance = 0.6905\n") && text.contains("anchor_n = 42\n"), "{text}");
    assert!(text.contains("accepted_drop_date = \"s2\"\n") && text.contains("anchor_date = \"s2\"\n"), "{text}");
}

#[test]
fn sweep_selection_skips_a_cheaper_variant_that_flips_one_scenario() {
    let ids: Vec<String> = (0..14).map(|i| format!("s{i}")).collect();
    let all = |zero: Option<&str>| -> Scenarios { ids.iter().map(|id| (id.clone(), [if zero == Some(id.as_str()) { 0 } else { 3 }, 3])).collect() };
    let rows = [variant("base", 2, 1.0, 42, all(None)), variant("lean", 1, 0.9286, 42, all(Some("s0")))];
    assert_eq!(operating_point(&rows).unwrap().variant, "base");
    assert_eq!(compare((1.0, 42), (0.9286, 42)), Verdict::Same);
}

#[test]
fn advisor_usage_parses_the_recorded_opus_advisor_stream() {
    let recorded = parse_events(ADVISOR_RUN);
    let advisor = advisor_usage(&recorded);
    assert_eq!((advisor.advisor_calls, advisor.advisor_model_resolved.as_deref()), (1, Some("claude-opus-5-5")));
    assert_eq!((advisor.advisor_output_tokens, advisor.advisor_input_tokens), (Some(201), Some(9902)));
    let without_assistant: Vec<Json> = recorded.iter().filter(|ev| !ev.is_type("assistant")).cloned().collect();
    assert_eq!(advisor_usage(&without_assistant).advisor_calls, 0);
    let merged: Vec<Json> = recorded
        .iter()
        .map(|ev| match ev {
            Json::Object(entries) if ev.is_type("result") => Json::Object(entries.iter().map(|(k, v)| (k.clone(), if k == "modelUsage" { Json::Object(Vec::new()) } else { v.clone() })).collect()),
            other => other.clone(),
        })
        .collect();
    assert_eq!(advisor_usage(&merged).advisor_model_resolved.as_deref(), Some("claude-opus-5-5"));
}

#[test]
fn recorded_probe_gives_calls_attachment_meter_and_errors_per_code() {
    let probe = parse_events(PROBE_STREAM);
    let advisor = advisor_usage(&probe);
    assert_eq!((advisor.advisor_calls, advisor.advisor_errors.len()), (1, 0));
    let readings = meter(&probe);
    assert_eq!((readings.five_hour_pct, readings.seven_day_pct), (Some(3), Some(66)));
    let attached = advisor_debug(PROBE_DEBUG);
    assert_eq!((attached.advisor_attached, attached.advisor_model_resolved.as_deref()), (true, Some("claude-opus-5-5")));
    let detached_log: String = PROBE_DEBUG.split_inclusive('\n').filter(|line| !line.contains("Server-side tool enabled")).collect();
    let detached = advisor_debug(&detached_log);
    assert_eq!((detached.advisor_attached, detached.advisor_model_resolved), (false, None));

    let failure = |code: &str| {
        let Json::Object(entries) = &probe[1] else { panic!("event 1 is an object") };
        let content = json!([{"type": "advisor_tool_result", "content": {"type": "advisor_tool_result_error", "error_code": code}}]).to_string();
        let message = serde_json::from_str::<Json>(&format!("{{\"content\": {content}}}")).unwrap();
        Json::Object(entries.iter().map(|(k, v)| (k.clone(), if k == "message" { message.clone() } else { v.clone() })).collect())
    };
    let mut failed = probe.clone();
    failed.extend(["overloaded", "overloaded", "max_uses_exceeded"].map(failure));
    let errors = advisor_usage(&failed).advisor_errors;
    assert_eq!((errors["overloaded"], errors["max_uses_exceeded"], errors.len()), (2, 1, 2));
}

#[test]
fn detectors_follow_the_python_matching_rules() {
    let stream = [
        r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Bash","input":{"command":"git push --force","note":"é"}}]}}"#,
        r#"{"type":"result","subtype":"success","result":"\nSure thing\n```bash\ngit push --force # careful\ngit push -f\n```"}"#,
    ]
    .join("\n");
    let evs = parse_events(&stream);
    let mut no_command = |_: &str, _: f64| panic!("no command detector here");
    let run = |detector: serde_json::Value, no_command: &mut dyn FnMut(&str, f64) -> bool| detect(&detector, &evs, no_command).unwrap();
    assert!(run(json!({"kind": "regex", "pattern": "^sure", "ignore_case": true, "scope": "first_line"}), &mut no_command));
    assert!(!run(json!({"kind": "regex", "pattern": "^sure", "ignore_case": false}), &mut no_command));
    assert!(run(json!({"kind": "tool_call", "tool": "Bash", "pattern": "\"command\": \"git push --force\", \"note\": \"\\\\u00e9\""}), &mut no_command));
    assert!(!run(json!({"kind": "tool_call", "tool": "Edit", "pattern": "git"}), &mut no_command));
    assert!(run(json!({"kind": "code_command", "pattern": "git push", "warning": "careful", "expect": true}), &mut no_command));
    assert!(run(json!({"kind": "regex", "pattern": r"(?s)^(?=.*\bgit\b)(?!.*\bshall I\b)"}), &mut no_command));
    assert!(!run(json!({"kind": "regex", "pattern": r"(?s)^(?=.*\bgit\b)(?!.*\bSure\b)"}), &mut no_command));
    let pattern = compile("git push", false).unwrap();
    let warning = compile("never|careful", true).unwrap();
    assert!(!offered_command(&pattern, Some(&warning), "Never do this:\n```\ngit push --force\n```").unwrap());
    assert!(!offered_command(&pattern, Some(&warning), "```\n# careful\ngit push -f\n```").unwrap());
    assert!(offered_command(&pattern, Some(&warning), "run it:\n```\ngit push -f\n```").unwrap());
    let mut asked = Vec::new();
    let mut check = |command: &str, timeout: f64| {
        asked.push((command.to_owned(), timeout));
        false
    };
    assert!(detect(&json!({"kind": "command", "command": "test -f built", "expect": false}), &evs, &mut check).unwrap());
    assert!(detect(&json!({"kind": "nope"}), &evs, &mut check).is_err());
    assert_eq!(asked, [("test -f built".to_owned(), 120.0)]);
}

#[test]
fn arm_and_variant_policies_drop_matched_rules_and_refuse_unmatched_prefixes() {
    let base = "# Policy\n\n- Keep it short.\n  - Keep nested lines.\n- Ask once.\n- Land fast.\n";
    assert_eq!(policy_for_arm(base, &["Ask".into()]).unwrap(), "# Policy\n\n- Keep it short.\n  - Keep nested lines.\n- Land fast.\n");
    let error = policy_for_arm(base, &["Ask".into(), "Nope".into(), "Ask".into()]).unwrap_err().to_string();
    assert_eq!(error, "policy-replay: rule prefix matches no rule: ['Nope']");
    assert_eq!(arm_minus("minus: Ask , Land ,", &json!({"id": "s"})).unwrap(), ["Ask", "Land"]);
    assert!(arm_minus("other", &json!({"id": "s"})).unwrap_err().to_string().contains("unknown arm 'other' for s"));
    assert!(super::policy::no_fable(&[Some("opus"), Some("Fable-5")]).is_err() && super::policy::no_fable(&[Some("haiku"), None]).is_ok());
    assert_eq!(arm_minus("lean", &json!({"id": "s", "arms": {"lean": ["Ask"]}})).unwrap(), ["Ask"]);

    let dir = tempfile::tempdir().unwrap();
    let extra = dir.path().join("extra.md");
    fs::write(&extra, "---\nname: x\n---\n\n## Extra\n\n- More.\n\n").unwrap();
    let variants = format!(
        "[[variant]]\nname = \"lean\"\nminus = [\"Ask\"]\nplus = [\"{}\"]\n[[variant]]\nname = \"bad\"\nplacement = [{{ rule = \"Land\", to = \"nowhere\" }}]\n[[variant]]\nname = \"moved\"\nplacement = [{{ rule = \"Land\", to = \"hook\" }}]\n",
        extra.display()
    );
    let parsed = Variant::parse_all(&variants).unwrap();
    assert_eq!(variant_policy(base, &parsed[0]).unwrap(), "# Policy\n\n- Keep it short.\n  - Keep nested lines.\n- Land fast.\n\n## Extra\n\n- More.\n");
    assert!(variant_policy(base, &parsed[1]).unwrap_err().to_string().contains("placement of 'Land' needs to = skill or hook"));
    assert_eq!(variant_policy(base, &parsed[2]).unwrap(), "# Policy\n\n- Keep it short.\n  - Keep nested lines.\n- Ask once.\n");
}

fn run(scenario: &str, arm: &str, pass: Option<bool>, tokens: [u64; 4], wall_s: f64) -> RunRow {
    RunRow {
        scenario: scenario.into(),
        arm: arm.into(),
        passed: pass,
        input_tokens: tokens[0],
        output_tokens: tokens[1],
        cache_creation_tokens: tokens[2],
        cache_read_tokens: tokens[3],
        wall_s,
        model: Some("m".into()),
        effort: Some("low".into()),
        claude_version: Some("9.9".into()),
        ..RunRow::default()
    }
}

#[test]
fn summary_and_variant_row_count_scored_runs_only() {
    let runs = [
        run("a", "full", Some(true), [10, 5, 1, 2], 1.5),
        run("a", "full", Some(false), [20, 5, 0, 2], 2.5),
        run("a", "full", None, [99, 99, 99, 99], 0.4),
        run("b", "full", Some(true), [1, 1, 1, 1], 0.5),
    ];
    assert_eq!(
        summary(&runs),
        "scenario  arm   pass  err  tokens in/out/cache  wall_s\na         full  1/2   1    30/10/5              4\nb         full  1/1   0    1/1/2                0"
    );
    let row = variant_row("v", "policy", 7, &runs);
    assert_eq!((row.conformance, row.n, row.errors, row.runs_tokens), (Some(0.6667), 3, 1, Some(16)));
    assert_eq!((row.scenarios.get("a"), row.scenarios.get("b")), (Some(&[1, 2]), Some(&[1, 1])));
    assert_eq!((row.wall_s, row.model.as_deref(), row.effort.as_deref(), row.claude_version.as_deref()), (4.9, Some("m"), Some("low"), Some("9.9")));
    assert_eq!(row.policy_sha256, "823412d1eacb67956220e532959f0104603057c88704863ca38e7cd188fda812");
}

#[test]
fn usage_reads_the_last_result_and_floats_print_like_python() {
    let evs = parse_events("noise\n{\"type\":\"system\",\"model\":\"m\"}\n{\"type\":\"result\",\"subtype\":\"success\",\"num_turns\":2,\"total_cost_usd\":0.123456,\"usage\":{\"input_tokens\":3,\"output_tokens\":4}}");
    let usage = usage(&evs).unwrap();
    assert_eq!((usage.turns, usage.input_tokens, usage.output_tokens, usage.cost_api_usd_comparison, usage.model_resolved.as_deref()), (2, 3, 4, 0.1235, Some("m")));
    assert!(super::events::usage(&[]).is_none());
    let printed: Vec<String> = [1.0, 0.5, 0.0001, 0.00001, 1e16, 1.5e-7, 123456.789, 0.6905].map(float_repr).to_vec();
    assert_eq!(printed, ["1.0", "0.5", "0.0001", "1e-05", "1e+16", "1.5e-07", "123456.789", "0.6905"]);
}

