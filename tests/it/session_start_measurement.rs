use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use north_core::foundation::shim;
use serde_json::json;

struct TomWindow {
    root: tempfile::TempDir,
    window: std::process::Child,
}

impl TomWindow {
    fn open() -> Self {
        let root = tempfile::tempdir().unwrap();
        let window = Command::new("sleep").arg("120").env_clear().spawn().unwrap();
        let tom = Self { root, window };
        let sessions = tom.path("profile/sessions");
        fs::create_dir_all(&sessions).unwrap();
        fs::create_dir_all(tom.path("home")).unwrap();
        fs::create_dir_all(tom.path("fleet")).unwrap();
        fs::write(tom.path("active"), "hook north-hook\n").unwrap();
        let record = json!({"pid": tom.window.id(), "sessionId": "tom-window", "kind": "interactive", "messagingSocketPath": tom.path("window.sock")});
        fs::write(sessions.join(format!("{}.json", tom.window.id())), record.to_string()).unwrap();
        let north = env!("CARGO_BIN_EXE_north");
        shim::write_script(&tom.path("recording-north"), &format!("echo \"$*\" >>'{}'\nexec '{north}' \"$@\"\n", tom.path("calls").display()));
        let hook = format!(
            "exec env -u NORTH_FLEET_SESSION -u NORTH_FLEET_ROLE -u NORTH_HOOK_ID HOME='{}' CLAUDE_PID={} CLAUDE_CONFIG_DIR='{}' NORTH_FLEET_DIR='{}' NORTH_BIN='{}' LANE_GC_UNLANDED=/dev/null NORTH_AGENT_ACTIVE='{}' '{north}' \"$@\"\n",
            tom.path("home").display(),
            tom.window.id(),
            tom.path("profile").display(),
            tom.path("fleet").display(),
            tom.path("recording-north").display(),
            tom.path("active").display(),
        );
        shim::write_script(&tom.path("north"), &hook);
        tom
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.path().join(name)
    }

    fn adoptions(&self) -> String {
        fs::read_to_string(self.path("calls")).unwrap_or_default().lines().filter(|call| call.starts_with("fleet adopt")).collect::<Vec<_>>().join("\n")
    }

    fn start(&self, session: &str) {
        let mut hook = Command::new(self.path("north")).args(["hook", "session-start"]).env_remove("AGENT_NO_AUTHORING_HOOKS").stdin(Stdio::piped()).stdout(Stdio::null()).spawn().unwrap();
        let payload = json!({"hook_event_name": "SessionStart", "source": "startup", "session_id": session, "cwd": self.path("home")});
        hook.stdin.take().unwrap().write_all(payload.to_string().as_bytes()).unwrap();
        assert!(hook.wait().unwrap().success());
    }
}

impl Drop for TomWindow {
    fn drop(&mut self) {
        let _ = self.window.kill();
        let _ = self.window.wait();
    }
}

fn held(fleet: &Path) -> bool {
    fleet.join("proxy/session.json").exists()
}

#[test]
fn the_token_measurements_budget_start_under_a_tom_window_with_the_proxy_name_free_runs_no_adoption_and_leaves_the_name_unheld() {
    let tom = TomWindow::open();
    let measure = north_core::agent_config_check::north_session_start(tom.path("north").into_os_string(), tom.path("home"));
    measure(false).unwrap();
    assert_eq!(tom.adoptions(), "", "the measurement's budget start is no conversation its window holds");
    assert!(!held(&tom.path("fleet")));

    tom.start("tom-window");
    assert!(tom.adoptions().contains("fleet adopt --name proxy --if-free"), "the window's own start runs the adoption: {:?}", tom.adoptions());
}
