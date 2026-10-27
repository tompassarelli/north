use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

const TOKEN: &str = "tok-secret-1";

const FAKE_CLAUDE: &str = r#"#!/bin/sh
[ "$1" = --version ] && { echo 9.9; exit 0; }
[ "$CLAUDE_CODE_OAUTH_TOKEN" = tok-secret-1 ] && { echo '{"type":"result","subtype":"success","is_error":false,"result":"ok","model":"m"}'; exit 0; }
echo '{"type":"result","subtype":"success","is_error":true,"result":"Not logged in - Please run /login","model":"m"}'
exit 1
"#;

struct Replay {
    scratch: tempfile::TempDir,
}

impl Replay {
    fn new() -> Self {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path();
        let replay = root.join("agents/replay");
        fs::create_dir_all(&replay).unwrap();
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::create_dir_all(root.join("home/.local/state/north/claude-tokens")).unwrap();
        fs::write(
            root.join("agents/AGENTS.md"),
            "# Policy\n\n## Rules\n\n- Keep it short.\n",
        )
        .unwrap();
        fs::write(
            root.join("agents/catalog-config.json"),
            r#"{"activation": {}, "registrations": {}, "baselines": []}"#,
        )
        .unwrap();
        fs::write(root.join("agents/orchestration.toml"), "[projects]\n").unwrap();
        fs::write(
            replay.join("variants.toml"),
            "[[variant]]\nname = \"base\"\n",
        )
        .unwrap();
        for id in ["a", "b"] {
            fs::write(replay.join(format!("{id}.json")), format!(r#"{{"id": "{id}", "prompt": "p", "detector": {{"kind": "regex", "pattern": "ok"}}}}"#)).unwrap();
        }
        fs::write(root.join("bin/claude"), FAKE_CLAUDE).unwrap();
        fs::set_permissions(root.join("bin/claude"), fs::Permissions::from_mode(0o755)).unwrap();
        Self { scratch }
    }

    fn home(&self) -> std::path::PathBuf {
        self.scratch.path().join("home")
    }

    fn sweep(&self) -> Output {
        let root = self.scratch.path();
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("harness/scripts/policy-replay.py");
        Command::new("python3")
            .arg("-I")
            .arg(script)
            .args([
                root.join("agents/replay/a.json"),
                root.join("agents/replay/b.json"),
            ])
            .arg("--sweep")
            .arg(root.join("agents/replay/variants.toml"))
            .arg("--candidate")
            .arg(root.join("candidate.toml"))
            .arg("--out")
            .arg(root.join("sweep.jsonl"))
            .env_clear()
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    root.join("bin").display(),
                    std::env::var("PATH").unwrap_or_default()
                ),
            )
            .env("HOME", self.home())
            .env("NORTH_FIRN", root)
            .env(
                "CLAUDE_CONFIG_DIR",
                self.home().join(".local/state/north/profiles/claude-x"),
            )
            .output()
            .unwrap()
    }

    fn rows(&self, file: &Path) -> Vec<serde_json::Value> {
        fs::read_to_string(file)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}

#[test]
fn a_not_logged_in_sweep_counts_every_run_as_an_error_and_a_token_profile_authenticates_without_exporting_the_token()
 {
    let replay = Replay::new();
    let root = replay.scratch.path();

    let refused = replay.sweep();
    assert!(
        !refused.status.success(),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );
    assert!(!root.join("candidate.toml").exists());
    let rows = replay.rows(&root.join("sweep.jsonl"));
    assert_eq!(
        (
            rows[0]["conformance"].is_null(),
            rows[0]["n"].as_u64(),
            rows[0]["errors"].as_u64()
        ),
        (true, Some(0), Some(2))
    );
    let runs = replay.rows(
        &replay
            .home()
            .join(".local/state/agents/replay/sweep-runs.jsonl"),
    );
    assert_eq!(runs.len(), 2);
    for run in &runs {
        assert!(run["pass"].is_null());
        assert_eq!(run["error"], "Not logged in - Please run /login");
    }

    fs::write(
        replay
            .home()
            .join(".local/state/north/claude-tokens/claude-x"),
        format!("{TOKEN}\n"),
    )
    .unwrap();
    let authed = replay.sweep();
    assert!(
        authed.status.success(),
        "{}",
        String::from_utf8_lossy(&authed.stderr)
    );
    let candidate = fs::read_to_string(root.join("candidate.toml")).unwrap();
    assert!(candidate.contains("conformance = 1.0"), "{candidate}");

    let mut written = String::from_utf8_lossy(&authed.stdout).into_owned()
        + &String::from_utf8_lossy(&authed.stderr);
    written += &fs::read_to_string(root.join("sweep.jsonl")).unwrap();
    written += &fs::read_to_string(
        replay
            .home()
            .join(".local/state/agents/replay/sweep-runs.jsonl"),
    )
    .unwrap();
    assert!(
        !written.contains(TOKEN),
        "the token leaked into output or rows"
    );
}
