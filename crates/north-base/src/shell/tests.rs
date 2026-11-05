use crate::shell::*;
use serde_json::json;

fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|s| (*s).to_owned()).collect()
}

fn property_commands() -> Vec<String> {
    let tables: &[(&[&str], &[&str])] = &[
        (
            &[
                "git add -A",
                "git add --all",
                "git add -u",
                "git add --update",
                "git add .",
                "git commit -a",
                "git commit --all",
                "git commit -am msg",
            ],
            &[
                "git add path/to/file",
                "git add .gitignore",
                "git commit -m msg",
            ],
        ),
        (
            &[
                "git stash",
                "git stash push",
                "git stash save",
                "git stash pop",
                "git -C /tmp stash pop",
            ],
            &[
                "git stash list",
                "git stash apply abc",
                "git stash push -u -m tag",
                "git stash push --message=tag",
            ],
        ),
        (
            &[
                "wl-copy hi",
                "wl-copy --clear",
                "/run/current-system/sw/bin/wl-copy hi",
                "niri msg action screenshot-window",
                "niri msg -j action screenshot-screen",
                "wl-paste",
                "wl-paste --list-types",
                "xclip -o -selection clipboard",
                "xsel -o",
            ],
            &[
                "echo \"wl-paste\"",
                "rg wl-paste socrates",
                "agent-capture --screen /tmp/s.png",
                "niri msg action focus-window --id 3",
            ],
        ),
        (
            &[
                "sed -i s/a/b/ /home/test/code/proj/main/src/host.nix",
                "tee /home/test/.claude/CLAUDE.md",
                "sd old new /home/test/.claude/CLAUDE.md",
                "cp /tmp/x /home/test/.codex/AGENTS.md",
                "mv /tmp/x /home/test/.codex/skills/a/SKILL.md",
            ],
            &[
                "cat /home/test/.claude/CLAUDE.md",
                "sed -n p /home/test/code/proj/main/src/host.nix",
                "sed -i s/a/b/ /home/test/code/proj/main/src/plain.nix",
                "cp /home/test/.claude/CLAUDE.md /tmp/claude.md",
                "agents sync",
            ],
        ),
        (
            &[
                "kill -1",
                "kill -9 -1",
                "kill -- -1",
                "pkill -u tom",
                "pkill niri",
                "killall -u tom",
                "killall niri",
                "loginctl terminate-user tom",
                "systemctl --user exit",
                "systemctl --user restart niri.service",
                "nohup bun task",
                "setsid bun task",
                "disown",
                "bun /tmp/task.mjs",
            ],
            &[
                "kill -1 1234",
                "kill -9 1234",
                "pkill -u tom scoped",
                "systemctl --user restart wob.service",
                "run-bounded 30m -- bun /tmp/task.mjs",
                "bun task > /tmp/task.log 2>&1",
            ],
        ),
        (
            &[
                "grep -rn foo src",
                "find . -name '*.ts'",
                "rgrep foo src",
                "grep --recursive -il foo src",
            ],
            &[
                "grep foo a.ts",
                "git grep foo",
                "find . -maxdepth 1 -name '*.ts'",
                "find . -mtime +3 -delete",
            ],
        ),
        (
            &[
                "rg TARGET /proc/[0-9]*/cwd",
                "rg TARGET /sys",
                "rg TARGET /home/test/code/proj",
            ],
            &[
                "rg TARGET /proc/1234/status",
                "rg TARGET /home/test/code/proj/main/src",
                "find /proc -maxdepth 1 -type d",
            ],
        ),
        (
            &[
                "rg needle /home/test/code/north-data",
                "rg needle /home/test/code/north-data/accounts",
                "grep -R needle /home/test/code/north-data",
                "find /home/test/code/north-data -name rollout-*.jsonl",
            ],
            &[
                "rg needle /home/test/code/north-data/accounts/openai/acct/sessions/2026/08/12",
                "rg --max-depth 2 needle /home/test/code/north-data",
                "convo -x needle",
            ],
        ),
        (
            &[
                "wine game.exe",
                "wine64 game.exe",
                "proton run game.exe",
                "steam-run wine game.exe",
            ],
            &[
                "ls game.exe",
                "pgrep -f game.exe",
                "bun /opt/machine-capacity.mjs session --class native --owner x -- wine game.exe",
            ],
        ),
        (
            &[
                "gh issue comment 5 --body life#1",
                "gh pr edit 3 --body travel#2",
            ],
            &[
                "gh issue view 1",
                "gh issue comment 5 --body landed",
                "threads note life#1 progress",
            ],
        ),
        (
            &[
                "cargo build --release",
                "bun test",
                "bun wisp map build",
                "bun wisp headless --render out --frames 3",
                "bun wisp view scene out",
            ],
            &[
                "cargo check",
                "bun test test/a.test.ts",
                "bun wisp map rebuild",
                "bun /opt/machine-capacity.mjs run --owner x --timeout-seconds 60 -- cargo build",
            ],
        ),
    ];
    let mut prefixes = strings(&[
        "",
        "CHECK=1 ",
        "CHECK=\"some value\" ",
        "command ",
        "sudo ",
        "env CHECK=1 ",
        "CHECK=1 command sudo env OTHER=2 ",
        "sudo -u tom ",
        "env -u CHECK CHECK=1 ",
        "CHECK=${HOME} ",
        "CHECK=$HOME ",
    ]);
    for (index, wrappers) in [
        "command",
        "sudo",
        "env",
        "command sudo",
        "command env",
        "sudo command",
        "sudo env",
        "env command",
        "env sudo",
        "command sudo env",
        "sudo env command",
        "env command sudo",
    ]
    .iter()
    .enumerate()
    {
        prefixes.push(format!("CHECK_{index}={} {wrappers} ", index * 791));
    }
    let quote = |text: &str| format!("'{}'", text.replace('\'', "'\"'\"'"));
    let mut cases = Vec::new();
    for (triggers, near_misses) in tables {
        for trigger in *triggers {
            cases.push((*trigger).to_owned());
            cases.push(format!("CHECK=$({trigger}); echo $CHECK"));
            for prefix in &prefixes {
                for separator in [
                    "",
                    "printf ready ; ",
                    "printf ready && ",
                    "printf ready | ",
                    "printf ready \n ",
                ] {
                    cases.push(format!("{separator}{prefix}{trigger}"));
                }
            }
            let single = quote(trigger);
            let double = format!(
                "\"{}\"",
                trigger
                    .replace('\\', "\\\\")
                    .replace('"', "\\\"")
                    .replace('$', "\\$")
                    .replace('`', "\\`")
            );
            cases.extend([
                single.clone(),
                double.clone(),
                format!("echo {single}"),
                format!("echo {double}"),
                format!("git commit -m {single}"),
                format!("cat <<'BODY'\n{trigger}\nBODY"),
                format!("echo {trigger}"),
            ]);
            for separator in [";", "&&", "|", "\n", "("] {
                let prose = format!("example {separator} {trigger}");
                cases.push(format!("echo {}", quote(&prose)));
                cases.push(format!(
                    "echo \"{}\"",
                    prose.replace('\\', "\\\\").replace('"', "\\\"")
                ));
            }
        }
        cases.extend(near_misses.iter().map(|s| (*s).to_owned()));
    }
    for trigger in [
        "git -C /home/test/code/proj/main reset --hard HEAD",
        "rm /home/test/code/proj/main/src/main.txt",
        "sed -i s/a/b/ /home/test/code/proj/main/src/main.txt",
        "cargo test --target-dir /home/test/code/proj/target-agent",
    ] {
        for prefix in ["", "CHECK=1 ", "command ", "sudo ", "env CHECK=1 "] {
            for separator in ["", "printf ready ; ", "printf ready && ", "printf ready | "] {
                cases.push(format!("{separator}{prefix}{trigger}"));
            }
        }
        cases.extend([
            format!("echo {}", quote(trigger)),
            format!("git commit -m {}", quote(trigger)),
            format!("cat <<'BODY'\n{trigger}\nBODY"),
        ]);
    }
    for command in [
        "gh issue comment 5 --body \"the orange bicycle beside the gate\"",
        "gh issue comment 5 --body \"the parcel went to the neighbour\"",
        "gh issue comment 5 --body-file body.md",
        "gh api repos/x/y/issues -F body=@body.md",
        "gh issue comment 5 --body-file - <<'BODY'\nlife#1\nBODY",
        "gh issue edit 5 --title \"Harness landing RBAC: authorized roles across providers and models\"",
        "gh issue comment 5 --body \"Harness landing RBAC: authorized roles across providers and models\"",
        "gh issue comment 5 --body \"the orange bicycle beside the\"",
        "gh issue comment 5 --body \"ordinary authorization for landing and prose\"",
        "gh issue view life#1",
        "gh issue view 5 --json title,body",
        "gh issue comment 5 --body \"the orange bicycle\"",
    ] {
        cases.push(command.into());
    }
    for text in [
        "repaint the garden shed blue tomorrow",
        "THE ORANGE BICYCLE BESIDE THE GATE",
        "the orange\n bicycle beside, the gate",
        "873492",
        "(873492),",
        "[Q7R9Z2];",
        "native@example.test",
        "<native@example.test>",
        "@native",
        "life#1",
        "accounts#2",
        "travel#3",
        "ops#4",
    ] {
        cases.push(format!("gh issue comment 5 --body {}", quote(text)));
    }
    for arg in [
        "'/home/test/code/proj/main/patterns file'",
        "<(printf '%s\\n' matched)",
        "<(jq -r '.value' '/home/test/code/proj/main/source file.json')",
        "<(jq -r .value \"/home/test/code/proj/main/source file.json\")",
    ] {
        let command = format!("grep -qFf {arg} '/home/test/code/proj/main/source file.json'");
        cases.extend([
            command.clone(),
            command.replace("grep -qFf", "rg -qF -f"),
            format!("{command}; grep -r needle src"),
        ]);
    }
    cases.extend(strings(&[
        "grep -rn foo .",
        "grep -qFf <(jq -r .v f) f extra",
        "grep -rqFf <(jq -r .v f) src",
        "grep -qFf <(jq -r .v f) src/*",
        "grep -qFf <(jq -r .v f) $(echo f)",
        "grep -qFf <(jq -r .v f) f # c",
        "grep -qFf <(jq -r .v f) f > out",
        "grep -qFf patterns file",
    ]));
    cases
}

fn wire(text: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    for token in tripwire_tokens(text) {
        bytes.extend(token.as_bytes());
        bytes.push(0);
        bytes.extend(tripwire_word(&token).as_bytes());
        bytes.push(0);
    }
    bytes
}

#[test]
fn shellcmd_property_fixed_generator_matches_python_reference() {
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    for command in property_commands() {
        digest.update(wire(&command));
        digest.update([255]);
    }
    assert_eq!(
        format!("{:x}", digest.finalize()),
        "9d10d84bb15374bfba21556565578576944b4a01366ac5ce362df505cf584786"
    );
}

#[allow(dead_code)] // called only by the shellcmd_compare example
pub fn compare_reference() {
    let property = property_commands();
    let mut fixtures = Vec::new();
    fn collect_json(data: &serde_json::Value, fixtures: &mut Vec<String>) {
        if let Some(command) = data
            .get("tool_input")
            .and_then(|v| v.get("command"))
            .and_then(|v| v.as_str())
        {
            fixtures.push(command.to_owned());
        }
        if let Some(object) = data.as_object() {
            for value in object.values() {
                collect_json(value, fixtures);
            }
        }
        if let Some(array) = data.as_array() {
            for value in array {
                collect_json(value, fixtures);
            }
        }
    }
    for entry in std::fs::read_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../harness/hooks")).unwrap() {
        let path = entry.unwrap().path();
        if !path.to_string_lossy().ends_with(".test.sh") {
            continue;
        }
        let source = std::fs::read_to_string(&path).unwrap();
        for (start, quote) in source
            .char_indices()
            .filter(|(_, c)| *c == '\'' || *c == '"')
        {
            let mut escaped = false;
            for (offset, c) in source[start + 1..].char_indices() {
                if offset > 65536 {
                    break;
                }
                if c == quote && !escaped {
                    let text = &source[start + 1..start + 1 + offset];
                    fixtures.push(text.to_owned());
                    if let Ok(data) = serde_json::from_str::<serde_json::Value>(text) {
                        collect_json(&data, &mut fixtures);
                    }
                    let mut decoded = String::new();
                    let mut chars = text.chars();
                    while let Some(c) = chars.next() {
                        if c != '\\' {
                            decoded.push(c);
                            continue;
                        }
                        match chars.next() {
                            Some('n') => decoded.push('\n'),
                            Some('r') => decoded.push('\r'),
                            Some('t') => decoded.push('\t'),
                            Some(c @ ('\\' | '\'' | '"')) => decoded.push(c),
                            Some(c) => {
                                decoded.push('\\');
                                decoded.push(c);
                            }
                            None => decoded.push('\\'),
                        }
                    }
                    fixtures.push(decoded);
                    break;
                }
                escaped = c == '\\' && !escaped;
            }
        }
    }
    fixtures.sort();
    fixtures.dedup();
    let mut commands: Vec<_> = property.into_iter().chain(fixtures).collect();
    let corpus_count = commands.len();
    commands.extend(strings(&[
        "wine game.exe; cat <<E²\n'\nE²",
        "wine game.exe; cat <<E\u{301}\n'\nE",
        "cat <<\u{1c}END\nx\nEND",
        "\u{1c}input=",
        "echo\u{1c}; next",
    ]));
    let Some(reference_path) = std::env::args().nth(1) else {
        println!("{}", json!(commands));
        return;
    };
    let reference: serde_json::Value =
        serde_json::from_slice(&std::fs::read(reference_path).unwrap()).unwrap();
    let reference = reference.as_array().unwrap();
    assert_eq!(commands.len(), reference.len());
    let mut differences = std::collections::BTreeMap::<String, usize>::new();
    for (text, expected) in commands.iter().zip(reference) {
        assert_eq!(expected["command"], json!(text));
        let spans = match argv_spans(text) {
            Ok(spans) => json!({"ok": spans}),
            Err(error) => json!({"error": error}),
        };
        let actual = json!({
            "words": words(text),
            "strip_quotes": strip_quotes(text),
            "strip_heredocs": strip_heredocs(text),
            "argv_spans": spans,
            "tokenize": tokenize(text, ";&|\n`", true, true, " \t\r"),
            "shell_segments_sep": shell_segments_sep(text),
            "tripwire": tripwire_tokens(text).iter()
                .map(|token| (token.as_str(), tripwire_word(token))).collect::<Vec<_>>()
        });
        for (api, result) in actual.as_object().unwrap() {
            if *result != expected[api] {
                let count = differences.entry(api.clone()).or_default();
                *count += 1;
                if *count <= 3 {
                    eprintln!("{api}: {text:?}\nRust: {result}\nPython: {}", expected[api]);
                }
            }
        }
    }
    println!("{corpus_count} original commands + {} regressions; differences by API: {differences:?}", commands.len() - corpus_count);
    assert!(differences.is_empty());
}
