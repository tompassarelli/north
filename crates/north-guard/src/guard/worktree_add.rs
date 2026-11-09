use super::git_invocation::{bash_command, invocation, invocations, words};
use super::worktree_remove::{directory_options, home};
use crate::hook::{HookInput, Verdict};
use regex::Regex;
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;

static WORKTREE: LazyLock<Regex> = LazyLock::new(|| invocation("worktree", true));

const VALUE_OPTIONS: [&str; 4] = ["-b", "-B", "--reason", "--orphan"];

pub fn decide(input: &HookInput) -> Verdict {
    let Some(command) = bash_command(input) else {
        return Verdict::Allow;
    };
    let cwd = PathBuf::from(input.cwd().unwrap_or_default());
    for (call, arguments) in invocations(&command, &WORKTREE) {
        let arguments: Vec<&str> = words(arguments).collect();
        if arguments.first() != Some(&"add") {
            continue;
        }
        let directory = directory_options(call).into_iter().fold(cwd.clone(), |directory, option| normalized(&directory.join(home(option))));
        let Some(lane) = lane_word(&arguments[1..]) else {
            continue;
        };
        let lane = normalized(&directory.join(home(lane)));
        if nested_in_main(&lane) {
            return Verdict::Deny(format!(
                "No. {0} is a lane inside a main checkout: git resolves a relative lane path against the -C directory, so `git -C X/main worktree add X/worktrees/NAME` lands at X/main/X/worktrees/NAME, which the worktree layout check reports as a stray. Create it with `north lane new --issue REPO#N /home/tom/code/PROJECT/worktrees/NAME`, or name the lane by its absolute path.",
                lane.display()
            ));
        }
    }
    Verdict::Allow
}

fn lane_word<'a>(arguments: &[&'a str]) -> Option<&'a str> {
    let mut skip = false;
    for word in arguments {
        if std::mem::take(&mut skip) {
            continue;
        }
        if VALUE_OPTIONS.contains(word) {
            skip = true;
        } else if !word.starts_with('-') {
            return Some(word);
        }
    }
    None
}

fn nested_in_main(lane: &Path) -> bool {
    let name_of = |path: Option<&Path>| path.and_then(Path::file_name).map(|name| name.to_owned());
    let worktrees = lane.parent();
    name_of(worktrees).is_some_and(|name| name == "worktrees") && name_of(worktrees.and_then(Path::parent).and_then(Path::parent)).is_some_and(|name| name == "main")
}

fn normalized(path: &Path) -> PathBuf {
    path.components().fold(PathBuf::new(), |mut kept, component| {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                kept.pop();
            }
            other => kept.push(other),
        }
        kept
    })
}
