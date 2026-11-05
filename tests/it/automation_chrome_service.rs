use std::os::unix::fs::PermissionsExt;
use std::process::Command;

#[test]
fn the_automation_chrome_service_starts_chrome_on_the_basic_password_store() {
    let scratch = tempfile::tempdir().unwrap();
    let recorded = scratch.path().join("argv");
    let fake = scratch.path().join("google-chrome");
    std::fs::write(&fake, format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n", recorded.display())).unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let state = scratch.path().join("state");
    let service = std::env::current_dir().unwrap().join("lib/automation-chrome/service.mjs");
    let path = format!("{}:{}", scratch.path().display(), std::env::var("PATH").unwrap_or_default());
    let output = Command::new("bun").arg(service).arg(&state).env("PATH", path).output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Chrome exited"), "{}", String::from_utf8_lossy(&output.stderr));
    let argv = std::fs::read_to_string(&recorded).unwrap();
    let argv: Vec<&str> = argv.lines().collect();
    assert!(argv.contains(&"--password-store=basic"), "{argv:?}");
    assert!(argv.contains(&format!("--user-data-dir={}", state.join("profile").display()).as_str()), "{argv:?}");
}
