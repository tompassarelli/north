use super::*;
use serde_json::json;

struct Fake {
    body: String,
}

impl World for Fake {
    fn repo(&mut self, named: Option<&str>, _: Option<&str>) -> Option<String> {
        Some(named.unwrap_or("tompassarelli/north").to_owned())
    }

    fn body(&mut self, _: &str, _: u64) -> Option<String> {
        Some(self.body.clone())
    }
}

fn verdict(dir: &std::path::Path, command: &str) -> Verdict {
    let payload = json!({"tool_name": "Bash", "cwd": dir, "tool_input": {"command": command}});
    let input = HookInput::parse(payload.to_string().as_bytes()).unwrap();
    refuse(&input, &mut Fake { body: "x".repeat(1000) })
}

#[test]
fn an_issue_edit_that_empties_or_halves_the_body_is_refused_and_an_ordinary_edit_or_a_named_shrink_passes_north_706() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("empty.md"), "").unwrap();
    std::fs::write(dir.path().join("short.md"), "y".repeat(400)).unwrap();
    std::fs::write(dir.path().join("grown.md"), "y".repeat(1200)).unwrap();
    let edit = |file: &str| format!("gh issue edit 706 -R tompassarelli/north --body-file {file}");
    assert!(matches!(verdict(dir.path(), &edit("empty.md")), Verdict::Deny(_)));
    assert!(matches!(verdict(dir.path(), &edit("short.md")), Verdict::Deny(_)));
    assert!(matches!(verdict(dir.path(), "gh issue edit 706 -R tompassarelli/north --body ''"), Verdict::Deny(_)));
    assert!(matches!(verdict(dir.path(), &edit("grown.md")), Verdict::Allow));
    assert!(matches!(verdict(dir.path(), &format!("{} --allow-shrink 'superseded by north#900'", edit("empty.md"))), Verdict::Allow));
    assert!(matches!(verdict(dir.path(), "gh issue edit 706 -R tompassarelli/north --title 'New title'"), Verdict::Allow));
    assert!(matches!(verdict(dir.path(), &format!("script > empty.md && {}", edit("empty.md"))), Verdict::Allow));
}

#[cfg(unix)]
#[test]
fn an_edit_after_a_cd_into_a_repository_is_judged_in_that_repository_not_the_hook_cwd_north_1104() {
    use std::os::unix::fs::PermissionsExt;
    let home = tempfile::tempdir().unwrap();
    let repository = home.path().join("code/north/main");
    std::fs::create_dir_all(&repository).unwrap();
    let empty = home.path().join("empty.md");
    std::fs::write(&empty, "").unwrap();
    let stub = home.path().join("gh");
    let body = "x".repeat(1000);
    std::fs::write(&stub, format!("#!/bin/sh\ncase \"$1\" in\nrepo) case \"$PWD\" in */code/north/main) echo tompassarelli/north;; *) exit 1;; esac;;\nissue) echo {body};;\nesac\n")).unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    let command = format!("cd {} && gh issue edit 902 --body-file {}", repository.display(), empty.display());
    let payload = json!({"tool_name": "Bash", "cwd": home.path(), "tool_input": {"command": command}});
    let input = HookInput::parse(payload.to_string().as_bytes()).unwrap();
    let verdict = refuse(&input, &mut Live { program: stub.to_string_lossy().into_owned() });
    assert!(matches!(verdict, Verdict::Deny(_)));
}
