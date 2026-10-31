use super::*;
use serde_json::{Value, json};
use std::path::PathBuf;

struct Workspace {
    _temporary: tempfile::TempDir,
    home: PathBuf,
    cwd: PathBuf,
}

fn workspace() -> Workspace {
    let temporary = tempfile::tempdir().unwrap();
    let home = temporary.path().join("home");
    let cwd = home.join("code/proj/main");
    std::fs::create_dir_all(cwd.join("src")).unwrap();
    for leaf in ["host.nix", "host.bnix", "plain.nix"] {
        std::fs::write(cwd.join("src").join(leaf), "").unwrap();
    }
    Workspace { _temporary: temporary, home, cwd }
}

impl Workspace {
    fn denial(&self, directory: &Path, payload: Value) -> Option<&'static str> {
        let input = HookInput::parse(payload.to_string().as_bytes()).unwrap();
        Policy::new(self.home.to_str().unwrap(), directory.to_str().unwrap()).decide(&input)
    }

    fn tool(&self, tool: &str, tool_input: Value) -> Option<&'static str> {
        let src = self.cwd.join("src");
        self.denial(&src, json!({"tool_name": tool, "cwd": src, "tool_input": tool_input}))
    }

    fn denied_from_cwd(&self, command: &str) -> bool {
        self.denial(
            &self.cwd,
            json!({"tool_name": "Bash", "cwd": self.cwd, "tool_input": {"command": command}}),
        )
        .is_some()
    }
}

#[test]
fn tool_fixtures_deny_and_pass_with_the_named_remedy() {
    let workspace = workspace();
    let home = workspace.home.display();
    let claude = format!("{home}/.claude/CLAUDE.md");
    let cases: Vec<(bool, &str, &str, Value)> = vec![
        (true, "agents sync", "Edit", json!({"file_path": claude, "old_string": "a", "new_string": "b"})),
        (true, "agents sync", "Write", json!({"file_path": format!("{home}/.codex/skills/x/SKILL.md"), "content": "x"})),
        (true, "agents sync", "NotebookEdit", json!({"notebook_path": format!("{home}/.codex/AGENTS.md")})),
        (true, ".bnix", "apply_patch", json!({"input": "*** Begin Patch\n*** Update File: host.nix\n@@\n-a\n+b\n*** End Patch"})),
        (true, ".bnix", "Bash", json!({"command": "sed -i s/a/b/ host.nix"})),
        (true, "agents sync", "Bash", json!({"command": "echo x > ~/.claude/CLAUDE.md"})),
        (true, "agents sync", "Bash", json!({"command": "printf x >>$HOME/.codex/AGENTS.md"})),
        (true, "agents sync", "Bash", json!({"command": "cd ~/.claude && sed -i s/a/b/ CLAUDE.md"})),
        (true, "agents sync", "Bash", json!({"command": "cd ~/.codex && echo x > AGENTS.md"})),
        (true, ".bnix", "Bash", json!({"command": "cd ~ && cd code/proj/main/src && sed -i s/a/b/ host.nix"})),
        (true, "agents sync", "Bash", json!({"command": "sed -Ei s/a/b/ ~/.claude/CLAUDE.md"})),
        (true, ".bnix", "Bash", json!({"command": "sed -ni p host.nix"})),
        (true, "agents sync", "Bash", json!({"command": "perl -pi -e s/a/b/ ~/.codex/AGENTS.md"})),
        (true, "agents sync", "Bash", json!({"command": "cp -t ~/.codex/skills/x src.md"})),
        (true, "agents sync", "Bash", json!({"command": "cp AGENTS.md ~/.codex/"})),
        (true, "agents sync", "Bash", json!({"command": "cd ~/.codex/$D && sed -i s/a/b/ AGENTS.md"})),
        (true, "agents sync", "Bash", json!({"command": "gawk -i inplace '{print}' ~/.claude/CLAUDE.md"})),
        (true, ".bnix", "Bash", json!({"command": "cd \"$D\" && sed -i s/a/b/ host.nix"})),
        (true, "agents sync", "Bash", json!({"command": "cp *.md ~/.claude/"})),
        (true, "absolute path", "Bash", json!({"command": "cd $CLAUDE_HOME && sed -i s/a/b/ CLAUDE.md"})),
        (true, "absolute path", "Bash", json!({"command": "cd ~/.cla* && echo x > CLAUDE.md"})),
        (true, "absolute path", "Bash", json!({"command": "popd && sed -i s/a/b/ AGENTS.md"})),
        (true, "agents sync", "Bash", json!({"command": "perl -0pi -e s/a/b/ ~/.codex/AGENTS.md"})),
        (true, "agents sync", "Bash", json!({"command": "cd /tmp && (cd ~/.codex && sed -i s/a/b/ AGENTS.md)"})),
        (false, "", "Bash", json!({"command": "(cd ~/.claude && ls) && sed -i s/a/b/ CLAUDE.md"})),
        (false, "", "Bash", json!({"command": "pushd ~/.claude && popd && sed -i s/a/b/ CLAUDE.md"})),
        (false, "", "Bash", json!({"command": "cd ~/.codex; cd - && echo x > AGENTS.md"})),
        (false, "", "Bash", json!({"command": "cd $X && cd /tmp && sed -i s/a/b/ CLAUDE.md"})),
        (false, "", "Bash", json!({"command": "cp *.md /tmp/"})),
        (false, "", "Bash", json!({"command": "cd ~/.codex && cat AGENTS.md"})),
        (false, "", "Bash", json!({"command": "cd ~/.claude && sed -n p CLAUDE.md"})),
        (false, "", "Bash", json!({"command": "perl -ne print ~/.claude/CLAUDE.md"})),
        (false, "", "Bash", json!({"command": "echo 'cd ~/.codex && sed -i s/a/b/ AGENTS.md'"})),
        (false, "", "Bash", json!({"command": "cd ~/.codex && sed -i s/a/b/ config.toml"})),
        (false, "", "Bash", json!({"command": "cd /tmp && sed -i s/a/b/ CLAUDE.md"})),
        (false, "", "Bash", json!({"command": "cp ~/.codex/AGENTS.md /tmp/x"})),
        (false, "", "Read", json!({"file_path": claude})),
        (false, "", "Bash", json!({"command": "cat ~/.claude/CLAUDE.md"})),
        (false, "", "Bash", json!({"command": "echo 'x > ~/.claude/CLAUDE.md'"})),
        (false, "", "Bash", json!({"command": "git commit -m \"sed -i s/a/b/ host.nix\""})),
        (false, "", "Edit", json!({"file_path": workspace.cwd.join("src/plain.nix")})),
        (false, "", "Bash", json!({"command": "agents sync"})),
        (false, "", "Write", json!({"file_path": format!("{home}/.codex/config.toml")})),
    ];
    let mut failures = Vec::new();
    for (want, remedy, tool, tool_input) in &cases {
        let reason = workspace.tool(tool, tool_input.clone());
        if reason.is_some() != *want || !reason.unwrap_or("").contains(remedy) {
            failures.push(format!("want={want} got={reason:?} {tool} {tool_input}"));
        }
    }
    patch_envelopes_nested_in_any_field_name_their_targets(&workspace);
    assert!(failures.is_empty(), "{failures:#?}");
}

fn patch_envelopes_nested_in_any_field_name_their_targets(workspace: &Workspace) {
    let envelope = format!(
        "*** Begin Patch\n*** Add File: x.md\n+a\n*** Move to: {}/.codex/AGENTS.md\n*** End Patch",
        workspace.home.display()
    );
    assert!(workspace.tool("functions.apply_patch", json!({"wrapper": {"text": envelope}})).is_some());
    assert!(workspace.tool("apply_patch", json!({"input": "*** Begin Patch\n*** Add File: x.md\n+a\n*** End Patch"})).is_none());
}

const TRIGGERS: &[&str] = &[
    "sed -i s/a/b/ {cwd}/src/host.nix",
    "tee {home}/.claude/CLAUDE.md",
    "sd old new {home}/.claude/CLAUDE.md",
    "cp /tmp/x {home}/.codex/AGENTS.md",
    "mv /tmp/x {home}/.codex/skills/a/SKILL.md",
];

const NEAR_MISSES: &[&str] = &[
    "cat {home}/.claude/CLAUDE.md",
    "sed -n p {cwd}/src/host.nix",
    "sed -i s/a/b/ {cwd}/src/plain.nix",
    "cp {home}/.claude/CLAUDE.md /tmp/claude.md",
    "agents sync",
];

const PREFIXES: &[&str] = &[
    "",
    "CHECK=1 ",
    "CHECK=\"some value\" ",
    "command ",
    "sudo ",
    "env CHECK=1 ",
    "CHECK=1 command sudo env OTHER=2 ",
    "CHECK_0=17 sudo env command ",
    "CHECK_1=4021 env sudo ",
    "CHECK_2=88 command env ",
    "sudo -u tom ",
    "env -u CHECK CHECK=1 ",
    "CHECK=${HOME} ",
    "CHECK=$HOME ",
];

fn single(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\"'\"'"))
}

fn double(text: &str) -> String {
    let escaped = text
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace('`', "\\`");
    format!("\"{escaped}\"")
}

fn fill(workspace: &Workspace, template: &str) -> String {
    template
        .replace("{home}", workspace.home.to_str().unwrap())
        .replace("{cwd}", workspace.cwd.to_str().unwrap())
}

#[test]
fn live_writes_are_refused_through_prefixes_chaining_and_substitution() {
    let workspace = workspace();
    for template in TRIGGERS {
        let trigger = fill(&workspace, template);
        assert!(workspace.denied_from_cwd(&trigger), "trigger {trigger}");
        let substitution = format!("CHECK=$({trigger}); echo $CHECK");
        assert!(workspace.denied_from_cwd(&substitution), "substitution {substitution}");
        for prefix in PREFIXES {
            let wrapped = format!("{prefix}{trigger}");
            assert!(workspace.denied_from_cwd(&wrapped), "prefix {wrapped}");
            for separator in [";", "&&", "|", "\n"] {
                let chained = format!("printf ready {separator} {wrapped}");
                assert!(workspace.denied_from_cwd(&chained), "chained {chained:?}");
            }
        }
    }
}

#[test]
fn quoted_heredoc_and_prose_mentions_stay_inert() {
    let workspace = workspace();
    for template in TRIGGERS {
        let trigger = fill(&workspace, template);
        let (single, double) = (single(&trigger), double(&trigger));
        for data in [&single, &double] {
            assert!(!workspace.denied_from_cwd(data), "bare quoted {data}");
        }
        for data in [
            format!("echo {single}"),
            format!("echo {double}"),
            format!("git commit -m {single}"),
            format!("cat <<'BODY'\n{trigger}\nBODY"),
        ] {
            assert!(!workspace.denied_from_cwd(&data), "inert {data}");
        }
        for separator in [";", "&&", "|", "\n", "("] {
            let prose = format!("example {separator} {trigger}");
            for data in [format!("echo {}", self::single(&prose)), format!("echo {}", self::double(&prose))] {
                assert!(!workspace.denied_from_cwd(&data), "quoted separators {data:?}");
            }
        }
    }
    for template in NEAR_MISSES {
        let command = fill(&workspace, template);
        assert!(!workspace.denied_from_cwd(&command), "near miss {command}");
    }
}
