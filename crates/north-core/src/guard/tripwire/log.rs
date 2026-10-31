use super::{State, base, git};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

const RECORD_FACT: &str = r#"
n=$1 s=$2 ag=$3 at=$4 tg=$5 rs=$6
"$n" tell "$s" kind guard_denial
"$n" tell "$s" agent "$ag"
"$n" tell "$s" guard north-hook
"$n" tell "$s" tool Bash
"$n" tell "$s" source tripwire
[ -n "$at" ] && "$n" tell "$s" at "$at"
[ -n "$tg" ] && "$n" tell "$s" target "$tg"
[ -n "$rs" ] && "$n" tell "$s" reason "$rs"
"#;

fn or_default(value: String, fallback: impl FnOnce() -> String) -> String {
    if value.is_empty() { fallback() } else { value }
}

fn timestamp() -> String {
    Command::new("date")
        .arg("-Is")
        .output()
        .ok()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim_end_matches('\n')
                .to_owned()
        })
        .unwrap_or_default()
}

impl State<'_> {
    fn head(&self) -> String {
        self.input
            .command()
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .replace('\n', " ")
            .chars()
            .take(200)
            .collect()
    }
    fn append(&self, root: &str, line: &str) {
        let _ = std::fs::create_dir_all(root);
        if let Ok(mut log) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(Path::new(root).join("tripwire.log"))
        {
            let _ = writeln!(log, "{line}");
        }
    }
    fn agent_id(&self, env: &impl Fn(&str) -> String) -> String {
        let sid = self.input.data["session_id"].as_str().unwrap_or("");
        let runtime = or_default(env("XDG_RUNTIME_DIR"), || "/tmp".into());
        let cached = if sid.is_empty() {
            String::new()
        } else {
            std::fs::read_to_string(format!("{runtime}/north-agent-ids/{sid}"))
                .unwrap_or_default()
                .trim_end_matches('\n')
                .to_owned()
        };
        let id = or_default(cached, || env("NORTH_AGENT_ID"));
        or_default(id, || {
            let repo = git(&self.cwd, &["rev-parse", "--show-toplevel"])
                .unwrap_or_else(|| self.cwd.clone());
            let short = if sid.is_empty() {
                "unknown".into()
            } else {
                sid.chars().take(8).collect::<String>()
            };
            format!("session-{}-{short}", base(&repo))
        })
    }
    fn record_fact(&self, fact: [&str; 5], env: &impl Fn(&str) -> String) {
        let north = or_default(env("NORTH_BIN"), || "north".into());
        let _ = Command::new("setsid")
            .args(["bash", "-c", RECORD_FACT, "_", &north])
            .args(fact)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }
    pub(super) fn log(&self, reason: &str, env: &impl Fn(&str) -> String) {
        let root = or_default(env("TRIPWIRE_LOG_DIR"), || {
            format!("{}/.local/state/north", self.home)
        });
        let at = timestamp();
        let head = self.head();
        self.append(&root, &format!("{at}\t{}\t{reason}\t{head}", self.cwd));
        let id = self.agent_id(env);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let subject = format!("denial:{id}-{now}");
        self.record_fact([&subject, &id, &at, &head, reason], env);
    }
}
