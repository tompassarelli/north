use super::json::Obj;
use super::request::Request;
use super::run::scope::scope_command;

fn launched(root: &std::path::Path, launcher: &str) -> (crate::capacity::subject_start::Exit, Option<serde_json::Value>, crate::capacity::subject_start::Start) {
    let lease = "00000000-0000-4000-8000-000000000004";
    let request = Request { name: "heavy".to_owned(), landing: true, ..Request::default() };
    let admitted = Obj::new().put("lease", lease).put("scope", serde_json::json!({ "cpus": 2, "memoryMiB": 1024 }));
    let command = ["bash".to_owned(), "bin/catalog-owner-check".to_owned()];
    let scoped = scope_command(root, &request, &admitted, Some(60), &command);
    let witness = crate::capacity::subject_start::witness(root, lease);
    let named = scoped.as_std().get_envs().find(|(name, _)| *name == crate::capacity::subject_start::ENV).and_then(|(_, value)| value.map(std::path::PathBuf::from));
    assert_eq!(named.as_deref(), Some(witness.as_path()), "the scope names the lease's start witness to the wrappers inside it");
    let program = root.join("systemd-run");
    north_base::foundation::shim::write_script(&program, &format!("{launcher}\n"));
    crate::capacity::subject_start::arm(&witness);
    let began = super::machine::now_ms() / 1000.0;
    let status = std::process::Command::new(&program).env(crate::capacity::subject_start::ENV, &witness).stderr(std::process::Stdio::null()).status().unwrap();
    let start = crate::capacity::subject_start::start(&witness, began, super::machine::now_ms() / 1000.0);
    let exit = crate::capacity::subject_start::Exit::judge(status.code().unwrap(), &witness);
    (exit, super::run::outcome::command_exit(&request, "land-check", &command, &admitted, exit).map(|line| line.value()), start)
}

#[test]
fn a_lease_whose_scope_never_started_its_subject_exits_no_verdict_not_the_launchers_failure() {
    let root = tempfile::tempdir().unwrap();
    let (exit, line, _) = launched(root.path(), "echo 'Failed to start transient scope unit: Remote peer disconnected' >&2; exit 1");
    assert_eq!(exit.code(), north_base::foundation::exit::NO_VERDICT_STATUS);
    let line = line.expect("a decision line");
    assert_eq!((line["decision"].as_str(), line["launcherExitCode"].as_i64()), (Some("SUBJECT_NOT_STARTED"), Some(1)));
    let (exit, line, start) = launched(root.path(), "printf 'gc root nan\\nnix develop 1\\n' >\"$NORTH_SUBJECT_STARTED.steps\"; touch -d @1 \"$NORTH_SUBJECT_STARTED\"; exit 1");
    assert!(start.steps.iter().all(|(_, seconds)| seconds.is_finite() && *seconds >= 0.0), "a witness dated before the launch and a stamp that is not a number leave the lease its release: {start:?}");
    assert_eq!(exit.code(), 1, "a subject that started and failed keeps its own exit");
    assert_eq!(line.expect("a decision line")["decision"].as_str(), Some("COMMAND_EXITED"));
}

#[test]
fn a_confined_workers_capacity_run_goes_through_the_broker_and_its_state_writing_verbs_are_refused() {
    use super::{Route, route};
    let words = |text: &str| text.split_whitespace().map(str::to_owned).collect::<Vec<_>>();
    assert_eq!(route(&words("run --class heavy --owner o -- cargo test"), true, false), Route::Broker, "a confined worker's lease is taken by the broker outside its domain");
    assert_eq!(route(&words("run --class heavy --owner o -- cargo test"), false, true), Route::Broker);
    for writing in ["session --class heavy --owner o", "reserve --class heavy", "renew --lease l", "release --lease l", "land-waiting", "hand-over --to h", "probe --class agent", "mode away"] {
        assert_eq!(route(&words(writing), true, false), Route::Refused, "a confined worker wrote the host's admission state in place: {writing}");
        assert_eq!(route(&words(writing), false, false), Route::InPlace, "{writing}");
    }
    for read_through_the_broker in ["status", "backlog"] {
        assert_eq!(route(&words(read_through_the_broker), true, false), Route::Broker, "a confined worker read {read_through_the_broker} in place, where its lock and reclaims are unwritable");
    }
    for reading in ["mode", "estimate --class heavy", "status x"] {
        assert_eq!(route(&words(reading), true, false), Route::InPlace, "{reading}");
    }
}
