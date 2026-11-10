mod neutralize;

use self::neutralize::{neutralize, restore};
use crate::hook_input::{HookInput, Verdict};
use regex::Regex;
use std::sync::LazyLock;
use crate::shell::{ShellToken, basename, heredoc_bodies, operator_tokens, strip_heredocs};

const DENIAL: &str = "No. `sops` decrypt output must not reach the terminal: a multi-line value such as a PEM key slips past a line-by-line redaction. Pull one value with `--extract '[\"key\"]'` into a pipe to its consumer, or send the output to a file.";
const INERT: &[&str] = &[
    "echo", "printf", "rg", "grep", "fd", "man", "which", "type", "ls", "sd", "git", "gh",
];
const DECLARATIONS: &[&str] = &["export", "local", "declare", "readonly", "typeset"];
const SHELLS: &[&str] = &["bash", "sh", "zsh", "dash"];
const TERMINALS: &[&str] = &["/dev/tty", "/dev/stdout", "/dev/stderr", "/dev/console"];
const NESTING: usize = 4;
static QUOTED_HEREDOC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"<<-?\s*['"\\]"#).unwrap());

pub fn evaluate(input: &HookInput) -> Verdict {
    let Some(text) = input.command() else {
        return Verdict::Allow;
    };
    if !input.is_shell() || !input.raw.windows(4).any(|bytes| bytes == b"sops") {
        return Verdict::Allow;
    }
    if prints_decrypted(&text.lossy(), 0, false) {
        Verdict::Deny(DENIAL.into())
    } else {
        Verdict::Allow
    }
}

#[derive(Default)]
struct Stage {
    words: Vec<String>,
    to_file: bool,
    to_terminal: bool,
}

struct Pipeline {
    stages: Vec<Stage>,
    captured: bool,
}

fn unquoted_command_text(body: &str) -> String {
    let mut open = false;
    body.chars()
        .map(|ch| match ch {
            '`' => {
                open = !open;
                if open { "$(".to_owned() } else { ")".to_owned() }
            }
            _ => ch.to_string(),
        })
        .collect()
}

fn heredoc_scripts(text: &str) -> Vec<String> {
    heredoc_bodies(text)
        .into_iter()
        .flat_map(|body| {
            let opener = text[..body.start].lines().next_back().unwrap_or_default();
            if opener.split_whitespace().any(|word| SHELLS.contains(&basename(word)) || word == "ssh") {
                vec![text[body].to_owned()]
            } else if QUOTED_HEREDOC.is_match(opener) {
                Vec::new()
            } else {
                substitutions(&unquoted_command_text(&text[body]))
            }
        })
        .collect()
}

fn prints_decrypted(text: &str, depth: usize, captured: bool) -> bool {
    if depth < NESTING
        && heredoc_scripts(text)
            .iter()
            .any(|inner| prints_decrypted(inner, depth + 1, false))
    {
        return true;
    }
    let text = neutralize(&strip_heredocs(text));
    let Ok(tokens) = operator_tokens(&text) else {
        return false;
    };
    pipelines(&tokens, captured).iter().any(|pipeline| {
        pipeline.stages.iter().enumerate().any(|(index, stage)| {
            if depth < NESTING && nested_prints(stage, depth) {
                return true;
            }
            decrypts(&stage.words) && reaches_terminal(pipeline, index)
        })
    })
}

fn nested_prints(stage: &Stage, depth: usize) -> bool {
    if let Some(script) = shell_script(&stage.words) {
        return prints_decrypted(&restore(&script), depth + 1, false);
    }
    let captured = assignment_only(&stage.words) || stage.to_file;
    if remote_command(&stage.words).iter().any(|script| prints_decrypted(&restore(script), depth + 1, false)) {
        return true;
    }
    stage
        .words
        .iter()
        .flat_map(|word| substitutions(word))
        .any(|inner| prints_decrypted(&inner, depth + 1, captured))
}

fn remote_command(words: &[String]) -> Vec<&String> {
    match program(words) {
        Some(start) if basename(&words[start]) == "ssh" => {
            words[start + 1..].iter().filter(|word| word.contains(char::is_whitespace)).collect()
        }
        _ => Vec::new(),
    }
}

fn substitutions(word: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = word;
    while let Some(start) = rest.find("$(") {
        let body = &rest[start + 2..];
        let mut level = 1;
        let end = body.char_indices().find_map(|(index, ch)| {
            level += i32::from(ch == '(') - i32::from(ch == ')');
            (level == 0).then_some(index)
        });
        let end = end.unwrap_or(body.len());
        out.push(body[..end].to_owned());
        rest = &body[end..];
    }
    out
}

fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

fn assignment_only(words: &[String]) -> bool {
    let skip = usize::from(words.first().is_some_and(|word| DECLARATIONS.contains(&word.as_str())));
    words[skip..].iter().all(|word| is_assignment(word))
}

fn program(words: &[String]) -> Option<usize> {
    words.iter().position(|word| !is_assignment(word))
}

fn shell_script(words: &[String]) -> Option<String> {
    let start = words.iter().position(|word| SHELLS.contains(&basename(word)) || word == "eval")?;
    let rest = &words[start + 1..];
    if words[start] == "eval" {
        return Some(rest.join(" "));
    }
    let flag = rest
        .iter()
        .position(|arg| arg.starts_with('-') && !arg.starts_with("--") && arg.contains('c'))?;
    rest.get(flag + 1).cloned()
}

fn sops_words(words: &[String]) -> Option<&[String]> {
    let first = program(words)?;
    if INERT.contains(&basename(&words[first])) {
        return None;
    }
    let sops = words[first..].iter().position(|word| basename(word) == "sops")?;
    Some(&words[first + sops..])
}

fn decrypts(words: &[String]) -> bool {
    sops_words(words).is_some_and(|sops| {
        sops[1..]
            .iter()
            .any(|arg| matches!(arg.as_str(), "decrypt" | "-d" | "--decrypt"))
    })
}

const NAME_FILTERS: &[&str] = &["keys", "keys[]", "keys_unsorted", "keys_unsorted[]", "length", "type"];

fn prints_no_values(stage: &Stage) -> bool {
    let Some(start) = program(&stage.words) else {
        return false;
    };
    let args: Vec<&str> = stage.words[start + 1..].iter().map(String::as_str).filter(|arg| !arg.starts_with('-')).collect();
    match basename(&stage.words[start]) {
        "jq" => args.first().is_some_and(|filter| NAME_FILTERS.contains(&filter.trim())),
        "awk" => args.first().is_some_and(|program| awk_prints_no_field(program)),
        _ => false,
    }
}

fn awk_prints_no_field(program: &str) -> bool {
    let program = restore(program);
    let program = program.as_str();
    static LENGTH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"length\([^)]*\)").unwrap());
    static MATCH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\$[0-9A-Za-z_]+\s*!?~\s*/[^/]*/").unwrap());
    let without_length = LENGTH.replace_all(program, "");
    let rest = MATCH.replace_all(&without_length, "");
    program.contains("print") && !rest.contains('$')
}

fn quiet_comparison(words: &[String]) -> bool {
    let Some(start) = program(words) else {
        return false;
    };
    match basename(&words[start]) {
        "cmp" => true,
        "diff" => words[start + 1..].iter().any(|arg| matches!(arg.as_str(), "-q" | "-s" | "--brief" | "--quiet")),
        _ => false,
    }
}

fn writes_elsewhere(args: &[String]) -> bool {
    args.iter().any(|arg| {
        matches!(arg.as_str(), "-i" | "--in-place" | "--output") || arg.starts_with("--output=")
    })
}

fn extracts(args: &[String]) -> bool {
    args.iter()
        .any(|arg| arg == "--extract" || arg.starts_with("--extract="))
}

fn reaches_terminal(pipeline: &Pipeline, index: usize) -> bool {
    let words = sops_words(&pipeline.stages[index].words).unwrap_or_default();
    let consumers = &pipeline.stages[index + 1..];
    let tees_to_terminal = consumers.iter().any(|stage| stage.words.iter().any(|word| terminal(word)));
    if pipeline.stages[index].to_terminal || tees_to_terminal {
        return true;
    }
    if pipeline.stages[index].to_file || pipeline.captured || writes_elsewhere(words) {
        return false;
    }
    match consumers.last() {
        Some(last) => !extracts(words) && !last.to_file && !prints_no_values(last),
        None => true,
    }
}

#[derive(Clone, Copy)]
enum Target {
    Out,
    Duplicate,
    Other,
}

#[derive(Default)]
struct Builder {
    out: Vec<Pipeline>,
    captured: Vec<bool>,
    stages: Vec<Stage>,
    stage: Stage,
    target: Option<Target>,
    comparing: bool,
}

impl Builder {
    fn end(&mut self) {
        self.target = None;
        self.stages.push(std::mem::take(&mut self.stage));
        let stages = std::mem::take(&mut self.stages);
        if stages.iter().any(|stage| !stage.words.is_empty()) {
            let captured = self.captured.last().copied().unwrap_or(false);
            self.out.push(Pipeline { stages, captured });
        }
    }

    fn word(&mut self, word: &str) {
        match self.target.take() {
            None => self.stage.words.push(word.to_owned()),
            Some(Target::Out) => self.sent_to(word),
            Some(Target::Duplicate) if word == "2" => self.stage.to_terminal = true,
            Some(Target::Duplicate) if word.chars().all(|c| c.is_ascii_digit() || c == '-') => {}
            Some(Target::Duplicate) => self.sent_to(word),
            Some(Target::Other) if word.contains("$(") => self.stage.words.push(word.to_owned()),
            Some(Target::Other) => {}
        }
    }

    fn sent_to(&mut self, path: &str) {
        if terminal(path) {
            self.stage.to_terminal = true;
        } else {
            self.stage.to_file = true;
        }
    }

    fn redirect(&mut self, duplicates: bool) {
        let descriptor = self
            .stage
            .words
            .last()
            .filter(|word| word.chars().all(|c| c.is_ascii_digit()))
            .cloned();
        if descriptor.is_some() {
            self.stage.words.pop();
        }
        self.target = Some(match (descriptor.as_deref(), duplicates) {
            (Some(fd), _) if fd != "1" => Target::Other,
            (_, true) => Target::Duplicate,
            _ => Target::Out,
        });
    }

    fn operator(&mut self, op: &str) {
        match op {
            "|" => {
                self.target = None;
                self.comparing = false;
                self.stages.push(std::mem::take(&mut self.stage));
            }
            "(" => {
                self.comparing |= self.target.is_some() && quiet_comparison(&self.stage.words);
                let assigned = self.comparing
                    || assignment_only(&self.stage.words)
                        && self.stage.words.last().is_some_and(|word| word.ends_with("=$"));
                self.end();
                self.captured.push(assigned);
            }
            ")" => {
                self.end();
                if self.captured.len() > 1 {
                    self.captured.pop();
                }
            }
            "<" => self.target = Some(Target::Other),
            _ => {
                self.comparing = false;
                self.end();
            }
        }
    }
}

fn terminal(path: &str) -> bool {
    TERMINALS.contains(&path) || path.starts_with("/dev/fd/") || path.starts_with("/proc/self/fd/")
}

fn is_operator(token: Option<&ShellToken>, wanted: &[&str]) -> bool {
    matches!(token, Some(ShellToken::Operator(op)) if wanted.contains(&op.as_str()))
}

fn pipelines(tokens: &[ShellToken], captured: bool) -> Vec<Pipeline> {
    let mut builder = Builder { captured: vec![captured], ..Builder::default() };
    let mut index = 0;
    while index < tokens.len() {
        match &tokens[index] {
            ShellToken::Word(word) => builder.word(word),
            ShellToken::Operator(op) if matches!(op.as_str(), ">" | ">>") => {
                let duplicates = is_operator(tokens.get(index + 1), &["&"]);
                let clobber = !duplicates && is_operator(tokens.get(index + 1), &["|"]);
                index += usize::from(duplicates || clobber);
                builder.redirect(duplicates);
            }
            ShellToken::Operator(op)
                if op == "&" && is_operator(tokens.get(index + 1), &[">", ">>"]) =>
            {
                builder.target = Some(Target::Out);
                index += 1;
            }
            ShellToken::Operator(op) => builder.operator(op),
        }
        index += 1;
    }
    builder.end();
    builder.out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused(command: &str) -> bool {
        prints_decrypted(command, 0, false)
    }

    #[test]
    fn refuses_decrypt_that_reaches_the_terminal() {
        for command in [
            "sops decrypt secrets.yaml | sed 's/key: .*/key: ***/'",
            "sops -d secrets.yaml | sed -e 's/a/b/'",
            "sops --decrypt secrets.yaml",
            "sops decrypt secrets.yaml",
            "sops decrypt --extract '[\"key\"]' secrets.yaml",
            "sudo sops -d secrets.yaml | grep token",
            "SOPS_AGE_KEY_FILE=/k sops -d f | head",
            "cd /x && sops decrypt f | cat",
            "sops decrypt f >&2",
            "sops decrypt f 2>/dev/null",
            "sops decrypt f > /dev/tty",
            "sops decrypt f >/dev/stderr",
            "sops decrypt f 3>/tmp/x",
            "bash -c 'sops -d f | sed s/a/b/'",
            "cat <(sops -d f)",
            "diff <(sops -d a) <(sops -d b)",
            "{ sops -d f; }",
            "if true; then sops -d f; fi",
            "for f in a b; do sops -d $f; done",
            "! sops -d f",
            "timeout 5 sops -d f",
            "nice sops -d f",
            "stdbuf -o0 sops -d f | cat",
            "echo \"$(sops -d f)\"",
            "echo $(sops -d f)",
            "echo `sops -d f`",
            "echo x=$(sops -d f)",
            "printf %s k=$(sops -d f)",
            "x=$(sops -d f >/dev/tty)",
            "timeout 30s sops -d f",
            "timeout -s KILL 5 sops -d f",
            "sudo -u root sops -d f",
            "env -u VAR sops -d f",
            "sudo -i sops -d f",
            "env -i sops -d f",
            "nix shell nixpkgs#sops -c sops -d f",
            "ssh host sops -d f",
            "eval \"sops -d f\"",
            "bash -c 'echo $(sops -d f)'",
            "cat <<EOF\n$(sops -d f)\nEOF",
            "x=\"$(sops -d f >&2)\"",
            "x=\"$(sops -d f | tee /dev/tty)\"",
            "cat <<< \"$(sops -d f)\"",
            "sops -d f | tee /dev/tty >/dev/null",
            "ssh host 'sops -d f'",
            "ssh host \"sops -d f | sed s/a/b/\"",
            "bash <<'EOF'\nsops -d f\nEOF",
            "ssh host <<EOF\nsops -d f\nEOF",
            "cat <<EOF\nIt's: $(sops -d f)\nEOF",
            "sops -d f | jq .pem",
            "sops -d f | jq 'keys' | tee /dev/tty",
            "sops -d f | awk '{print $2}'",
            "cat <(sops -d f)",
            "printf %s \"$(sops -d f)\"",
        ] {
            assert!(refused(command), "{command}");
        }
    }

    #[test]
    fn allows_decrypt_whose_output_never_prints_a_value() {
        for command in [
            "sops -d --output-type json f | jq 'keys'",
            "sudo env SOPS_AGE_KEY_FILE=/k sops -d --output-type json f | jq 'keys'; echo done",
            "sops -d f | jq -r 'keys[]'",
            "sops -d f | awk -F': ' '/api_key/ {print \"length\", length($2), \"ok:\", ($2 ~ /^sk-/)}'",
            "cmp <(sops -d --extract '[\"a\"]' f) <(sops -d --extract '[\"a\"]' g) && echo same",
            "diff -q <(sops -d a) <(sops -d b)",
            "printf 'header = \"Bearer %s\"\\n' \"$(sops -d --extract '[\"k\"]' f)\" > \"$t\"",
        ] {
            assert!(!refused(command), "{command}");
        }
    }

}
