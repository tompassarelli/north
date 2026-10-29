use super::shell::{resolve, shell_c_script, tokens};
use super::{Env, Policy};
use crate::guard::paths;
use crate::shell::{self, basename, shell_substitutions, whitespace};
use std::path::Path;

pub(super) fn git_output(args: &[&str]) -> Option<std::process::Output> {
    crate::platform::git::run_within(args, std::time::Duration::from_secs(5))
}
pub(super) fn tracks_nothing(path: &str) -> bool {
    let parent = if Path::new(path).is_dir() {
        Path::new(path)
    } else {
        Path::new(path).parent().unwrap_or(Path::new(""))
    };
    let parent = parent.to_string_lossy();
    let Some(first) = git_output(&["-C", &parent, "ls-files", "--error-unmatch", "--", path])
    else {
        return false;
    };
    if first.status.success() {
        return false;
    }
    git_output(&["-C", &parent, "ls-files", "--", path]).is_some_and(|out| {
        out.status.success()
            && String::from_utf8_lossy(&out.stdout)
                .trim_matches(whitespace)
                .is_empty()
    })
}
pub(super) struct GitCall {
    pub(super) env: Env,
    pub(super) target: String,
    pub(super) verb: String,
    pub(super) args: Vec<String>,
}
impl Policy {
    pub(super) fn git_invocations(&self, text: &str, cwd: &str, env: &Env) -> Vec<GitCall> {
        let mut found = Vec::new();
        for segment in self.segments(text, env, cwd) {
            let words = tokens(&segment.text);
            let executable = shell::command_index(&words);
            for here in &segment.cwds {
                for (i, word) in words.iter().enumerate() {
                    let name = basename(word).trim_start_matches('(');
                    if ["sh", "bash", "zsh"].contains(&name)
                        && i == executable
                        && let Some(script) = shell_c_script(&words, i).filter(|s| !s.is_empty())
                    {
                        found.extend(self.git_invocations(script, here, &segment.env));
                    }
                    if name != "git" {
                        continue;
                    }
                    let mut target = Some(here.clone());
                    let mut j = i + 1;
                    while j < words.len() {
                        let word = &words[j];
                        if word == "-C" && j + 1 < words.len() {
                            target = target.and_then(|target| {
                                self.target(&words[j + 1], &segment.env, &target)
                                    .map(|named| named.0)
                            });
                            j += 2;
                            continue;
                        }
                        if word.starts_with('-') {
                            j += 1;
                            continue;
                        }
                        if let Some(target) = target {
                            found.push(GitCall {
                                env: segment.env.clone(),
                                target,
                                verb: word.clone(),
                                args: words[j + 1..].to_vec(),
                            });
                        }
                        break;
                    }
                }
                for body in shell_substitutions(&segment.text) {
                    found.extend(self.git_invocations(&body, here, &segment.env));
                }
            }
        }
        found
    }
    pub(super) fn worktree_decision(&self, call: &GitCall) -> Option<(String, String)> {
        let start = call.args.iter().position(|s| !s.starts_with('-'))?;
        let sub = call.args[start].as_str();
        let mut paths = Vec::new();
        let mut skip = false;
        let mut literal = false;
        for arg in &call.args[start + 1..] {
            if skip {
                skip = false;
                continue;
            }
            if literal {
                paths.push(arg);
                continue;
            }
            if arg == "--" {
                literal = true;
                continue;
            }
            if ["-b", "-B", "--reason"].contains(&arg.as_str()) {
                skip = true;
                continue;
            }
            if arg.starts_with('-') {
                continue;
            }
            paths.push(arg);
        }
        if sub == "add" {
            if self
                .protected(&call.target)
                .is_some_and(|hit| hit.pin.is_some())
            {
                return Some((
                    call.target.clone(),
                    "worktree add from an immutable pin".into(),
                ));
            }
            if let Some(raw) = paths.first() {
                let path = resolve(&self.substitute(raw, &call.env), &call.target)?;
                if let Some(hit) = self.protected(&path)
                    && !(hit.pin.is_some()
                        && Path::new(&path)
                            .parent()
                            .and_then(Path::file_name)
                            .is_some_and(|s| s == "pins"))
                {
                    return Some((path, "worktree add".into()));
                }
            }
        } else if sub == "remove" || sub == "move" {
            for raw in paths.iter().take(if sub == "remove" { 1 } else { 2 }) {
                let path = resolve(&self.substitute(raw, &call.env), &call.target)?;
                if let Some(hit) = self.protected(&path) {
                    let mut stray = false;
                    if sub == "remove"
                        && hit.pin.is_none()
                        && !call.args.iter().any(|s| s == "-f" || s == "--force")
                        && let Some(out) =
                            git_output(&["-C", &call.target, "worktree", "list", "--porcelain"])
                    {
                        let listed: Vec<_> = String::from_utf8_lossy(&out.stdout)
                            .lines()
                            .filter_map(|line| {
                                line.strip_prefix("worktree ")
                                    .and_then(|s| paths::realpath(Path::new(s)))
                            })
                            .collect();
                        stray = listed.first().is_some_and(|s| s != &path)
                            && listed.iter().skip(1).any(|s| s == &path);
                    }
                    if !stray {
                        return Some((path, format!("worktree {sub}")));
                    }
                }
            }
        }
        None
    }
}
fn short_letters(args: &[String]) -> Vec<char> {
    args.iter()
        .filter(|s| {
            s.starts_with('-') && s.len() > 1 && s[1..].chars().all(|c| c.is_ascii_alphabetic())
        })
        .flat_map(|s| s[1..].chars())
        .collect()
}
pub(super) fn git_read_form(verb: &str, args: &[String]) -> bool {
    match verb {
        "stash" => args
            .iter()
            .find(|s| !s.starts_with('-'))
            .is_some_and(|s| ["list", "show"].contains(&s.as_str())),
        "clean" => args.iter().any(|s| s == "--dry-run") || short_letters(args).contains(&'n'),
        _ => false,
    }
}
pub(super) fn wip_destroying(verb: &str, args: &[String]) -> Option<String> {
    let has = |flag: &str| args.iter().any(|s| s == flag);
    match verb {
        "reset" => ["--hard", "--merge", "--keep"]
            .into_iter()
            .find(|s| has(s))
            .map(|s| format!("git reset {s}")),
        "stash" if !git_read_form(verb, args) => Some(format!(
            "git stash {}",
            args.iter()
                .find(|s| !s.starts_with('-'))
                .map_or("push", String::as_str)
        )),
        "checkout" if has("--") || has(".") || has("-f") || has("--force") => {
            Some("git checkout of working-tree paths".into())
        }
        "restore" if !has("--staged") || has("--worktree") => Some("git restore".into()),
        "clean"
            if !git_read_form(verb, args)
                && (has("--force") || short_letters(args).contains(&'f')) =>
        {
            Some("git clean -f".into())
        }
        _ => None,
    }
}
