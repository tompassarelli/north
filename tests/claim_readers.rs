use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::Command;

use rusqlite::Connection;

const FAKE_GH: &str = r#"#!/bin/sh
iss() { printf '{"number":%s,"title":"T%s","labels":[%s],"body":"- [ ] a","comments":[]}' "$1" "$1" "$2"; }
now='{"name":"priority:now"}'
case "$*" in
  "issue list -R tompassarelli/smashcraft"*)
    printf '[%s,%s,%s,%s]\n' "$(iss 3 "$now")" "$(iss 4 "$now")" "$(iss 14 "$now")" "$(iss 16 '')"; exit ;;
  "issue list"*) echo '[]'; exit ;;
  *"state createdAt"*)
    printf '{"data":{"r0":{"i3":{"state":"OPEN","createdAt":"2026-01-01T00:00:00Z"},"i4":{"state":"OPEN","createdAt":"2026-01-02T00:00:00Z"},"i14":{"state":"OPEN","createdAt":"2026-01-02T00:00:00Z"},"i16":{"state":"OPEN","createdAt":"2026-01-01T00:00:00Z"}}}}\n'; exit ;;
  *refPrefix*) printf '{"data":{"repository":{"refs":{"nodes":[]}}}}\n'; exit ;;
  *compare*) printf '{"data":{"repository":{"defaultBranchRef":{}}}}\n'; exit ;;
  *name=smashcraft*) nodes='{"number":3,"title":"T3","blockedBy":{"nodes":[]}},{"number":4,"title":"T4","blockedBy":{"nodes":[]}}' ;;
  *) nodes='' ;;
esac
printf '{"data":{"repository":{"issues":{"pageInfo":{"hasNextPage":false,"endCursor":null},"nodes":[%s]}}}}\n' "$nodes"
"#;

struct Rig {
    _scratch: tempfile::TempDir,
    root: PathBuf,
}

impl Rig {
    fn new() -> Self {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path().to_path_buf();
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::write(root.join("bin/gh"), FAKE_GH).unwrap();
        fs::set_permissions(root.join("bin/gh"), fs::Permissions::from_mode(0o755)).unwrap();
        symlink(concat!(env!("CARGO_MANIFEST_DIR"), "/bin/threads"), root.join("bin/threads")).unwrap();
        fs::write(root.join("orchestration.toml"), "").unwrap();
        Rig { _scratch: scratch, root }
    }

    fn run(&self, program: &Path, arguments: &[&str]) -> String {
        let output = Command::new(program)
            .args(arguments)
            .env("HOME", &self.root)
            .env("THREADS_DB", self.root.join("threads.db"))
            .env("AGENTS_ORCHESTRATION", self.root.join("orchestration.toml"))
            .env("NORTH_BIN", env!("CARGO_BIN_EXE_north"))
            .env("THREADS_ALIVE_OWNERS", r#"["bob","carol"]"#)
            .env("PATH", format!("{}:{}", self.root.join("bin").display(), std::env::var("PATH").unwrap_or_default()))
            .env_remove("THREADS_NO_NATIVE")
            .env_remove("CLAUDE_CODE_REMOTE")
            .output()
            .unwrap();
        assert!(output.status.success(), "{arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8(output.stdout).unwrap()
    }

    fn threads(&self, arguments: &[&str]) -> String {
        self.run(&self.root.join("bin/threads"), arguments)
    }
}

fn column(output: &str, index: usize) -> Vec<&str> {
    output.lines().filter_map(|line| line.split('\t').nth(index)).collect()
}

#[test]
fn a_void_claim_leaves_its_issue_ready_unowned_and_unheld_while_a_live_claim_holds_it() {
    let rig = Rig::new();
    for (item, owner) in [("smashcraft#4", "ghost"), ("smashcraft#14", "ghost"), ("smashcraft#16", "carol"), ("smashcraft#3", "bob")] {
        rig.threads(&["claim", item, "--by", owner]);
    }
    Connection::open(rig.root.join("threads.db"))
        .unwrap()
        .execute("UPDATE claims SET since = '2026-01-01T00:00:01Z' WHERE owner = 'ghost'", [])
        .unwrap();

    let ready = rig.threads(&["ready"]);
    assert!(column(&ready, 0).contains(&"smashcraft#4"), "{ready}");
    assert!(!column(&ready, 0).contains(&"smashcraft#3"), "{ready}");

    let unowned = rig.threads(&["unowned"]);
    let owner_of = |item: &str| {
        unowned.lines().find(|line| line.starts_with(item)).and_then(|line| line.split('\t').nth(2)).unwrap_or_default().to_string()
    };
    assert_eq!(owner_of("smashcraft#14"), "UNOWNED", "{unowned}");
    assert_eq!(owner_of("smashcraft#16"), "carol", "{unowned}");

    let work = rig.run(Path::new(env!("CARGO_BIN_EXE_north")), &["work", "list"]);
    let line = |item: &str| work.lines().find(|line| line.starts_with(item)).unwrap_or_default().to_string();
    assert!(line("smashcraft#4").contains("ready"), "{work}");
    assert!(line("smashcraft#3").contains("held") && line("smashcraft#3").contains("bob"), "{work}");
}
