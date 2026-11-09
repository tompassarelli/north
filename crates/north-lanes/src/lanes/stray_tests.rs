use std::path::Path;
use std::process::Command;

use super::stray::remove;

fn tool_call(at: &str, path: &Path) -> String {
    serde_json::json!({"type":"assistant","timestamp":at,"message":{"content":[{"type":"tool_use","name":"Bash","input":{"command":format!("touch {}", path.display())}}]}}).to_string()
}

#[test]
fn only_an_untracked_file_born_after_the_sessions_own_tool_call_named_it_is_removed_north_937() {
    let checkout = tempfile::tempdir().unwrap();
    assert!(Command::new("git").arg("init").arg("-q").arg(checkout.path()).status().unwrap().success());
    let stray = checkout.path().join("zz-n937-untracked-probe");
    let human = checkout.path().join("notes.txt");
    let unnamed = checkout.path().join("other.txt");
    for path in [&stray, &human, &unnamed] {
        std::fs::write(path, "").unwrap();
    }
    let transcript = [tool_call("2000-01-01T00:00:00Z", &stray), tool_call("2999-01-01T00:00:00Z", &human)].join("\n");

    assert_eq!(remove(&human, &transcript), Err("it existed before this session's first tool call naming it".to_owned()));
    assert_eq!(remove(&unnamed, &transcript), Err("no tool call of this session names it".to_owned()));
    assert_eq!(remove(&stray, &transcript), Ok(()));
    assert!(!stray.exists() && human.exists() && unnamed.exists());
}

#[test]
fn a_merge_temporary_in_a_main_checkout_goes_without_a_transcript_north_1006() {
    let project = tempfile::tempdir().unwrap();
    let main = project.path().join("main");
    std::fs::create_dir_all(&main).unwrap();
    assert!(Command::new("git").arg("init").arg("-q").arg(&main).status().unwrap().success());
    let leftover = main.join(".merge_file_3GaoPz");
    let lookalike = main.join(".merge_file_notes");
    for path in [&leftover, &lookalike] {
        std::fs::write(path, "").unwrap();
    }
    let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
    std::fs::File::options().write(true).open(&leftover).unwrap().set_modified(old).unwrap();

    assert_eq!(remove(&lookalike, ""), Err("no tool call of this session names it".to_owned()));
    assert_eq!(remove(&leftover, ""), Ok(()));
}
