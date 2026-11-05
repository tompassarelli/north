use north_core::grounding::store::{SCHEMA, Sqlite, quote};
use serde_json::{Value, json};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const ACCESS_PLAN: &str = "~/code/south/main/agents/decisions/harness/gex45-access-plan.md";
const RECOMMENDS: &str = "Landed `abc123`.\n\nI recommend building the laptop runtime on gex45 and shipping it from there.";

struct Lab {
    root: tempfile::TempDir,
}

impl Lab {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        for dir in ["home", "bin", "grounding"] {
            std::fs::create_dir_all(root.path().join(dir)).unwrap();
        }
        let lab = Self { root };
        let sqlite = std::env::split_paths(&std::env::var_os("PATH").unwrap()).map(|dir| dir.join("sqlite3")).find(|path| path.is_file()).expect("sqlite3 on PATH");
        std::os::unix::fs::symlink(sqlite, lab.path("bin/sqlite3")).unwrap();
        std::fs::write(lab.path("north.active"), "hook north-hook\n").unwrap();
        lab.index();
        lab
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.path().join(name)
    }

    fn index(&self) {
        let rows = [
            (ACCESS_PLAN, "south:agents/decisions/harness/gex45-access-plan.md § P3b", "The laptop runtime is never built on gex45 and never shipped from gex45; gex45 serves builds for its own lanes only (P3b, B3, B17)."),
            ("north#401", "north#401 flaky landing test", "The landing test is flaky; a seeded retry would fix it."),
            ("~/code/north/main/docs/garden.md", "north:docs/garden.md § Tomatoes", "Water the tomatoes at dawn and stake the vines."),
            ("~/code/north/main/docs/feedback-loop.md", "north:docs/feedback-loop.md § Signals", "Each signal row ends with a blank line, and its reader takes one line per row to the end of the file; we recommend adding nothing between rows."),
            ("~/code/south/main/agents/decisions/smashcraft/makgora-full-rename-plan.md", "south:agents/decisions/smashcraft/makgora-full-rename-plan.md § Order", "The full rename plan: renaming every README heading and title from Smashcraft to Makgora, and we recommend the README heading lands first."),
        ];
        let mut sql = format!("{SCHEMA}\nBEGIN;\n");
        for (reference, title, text) in rows {
            let source = if reference.starts_with("north#") { "threads" } else { "policy" };
            sql.push_str(&format!(
                "insert into docs(source,id,ref,title,text,at,decays) values ({},{},{},{},{},0,0);\n",
                quote(source),
                quote(&format!("{reference}#0")),
                quote(reference),
                quote(title),
                quote(text)
            ));
        }
        sql.push_str("COMMIT;");
        Sqlite::open(&self.path("grounding/index.db"), false).unwrap().exec(&sql).unwrap();
    }

    fn stop(&self, message: &str, active: bool) -> Option<Value> {
        let mut child = Command::new(env!("CARGO_BIN_EXE_north"))
            .args(["hook", "stop"])
            .env_clear()
            .env("HOME", self.path("home"))
            .env("PATH", self.path("bin"))
            .env("NORTH_AGENT_ACTIVE", self.path("north.active"))
            .env("NORTH_GROUNDING_STATE", self.path("grounding"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let payload = json!({"hook_event_name":"Stop", "session_id":"s", "stop_hook_active":active, "last_assistant_message":message});
        child.stdin.take().unwrap().write_all(payload.to_string().as_bytes()).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        serde_json::from_slice(&output.stdout).ok()
    }
}

fn reason(output: &Option<Value>) -> &str {
    output.as_ref().and_then(|value| value["reason"].as_str()).unwrap_or("")
}

#[test]
fn the_stop_hook_blocks_a_recommendation_that_a_recorded_decision_bears_on_once() {
    let lab = Lab::new();
    let blocked = lab.stop(RECOMMENDS, false);
    assert_eq!(blocked.as_ref().map(|value| value["decision"].clone()), Some(json!("block")), "{blocked:?}");
    assert_eq!(reason(&blocked), "These recorded decisions bear on your recommendation: south:agents/decisions/harness/gex45-access-plan.md; read them and confirm or revise.");
    assert_eq!(lab.stop(RECOMMENDS, true), None, "stop_hook_active lets the revised report through");
    assert_eq!(lab.stop("Landed.\n\nThe fix is a seeded retry in the flaky landing test.", false), None);
    assert_eq!(lab.stop("Landed `abc123`; the laptop runtime builds on gex45.", false), None);
    assert_eq!(lab.stop("Landed.\n\nI recommend adding a blank line at the end of the changelog.", false), None, "edit words alone are no bearing");
    assert_eq!(lab.stop("Landed.\n\nI recommend renaming the README heading.", false), None, "edit words alone are no bearing");
}

#[test]
fn the_stop_hook_stays_under_300_ms_on_a_warm_grounding_index() {
    let lab = Lab::new();
    lab.stop(RECOMMENDS, false);
    let started = Instant::now();
    let blocked = lab.stop(RECOMMENDS, false);
    let took = started.elapsed();
    assert!(reason(&blocked).contains("gex45-access-plan.md"), "{blocked:?}");
    assert!(took < Duration::from_millis(300), "{took:?}");
}
