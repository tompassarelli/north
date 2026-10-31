use super::git_invocation::words;
use crate::hook::{HookInput, Verdict};
use crate::shell::{ShellToken, basename, command, operator_tokens, strip_heredocs};

pub(super) const DENIAL: &str = "Native evidence runs go through `bun wisp accept`, which takes its own quiet native lease. For a run that can't use one, prefix NORTH_NATIVE_SOLO='<reason>'; the reason is recorded in the run ledger.";
pub(super) const QUIET_DENIAL: &str = "The quiet native lease is for timing checks only; `bun wisp accept` takes it per batch. Prefix NORTH_NATIVE_SOLO='<reason>' for a recorded exception.";

pub fn evaluate(input: &HookInput) -> Verdict {
    if crate::guard::activation::authoring_guards_off("north-hook") {
        return Verdict::Allow;
    }
    decide_with(input, |reason, text| {
        let session = std::env::var("NORTH_FLEET_SESSION")
            .ok()
            .or_else(|| input.data["session_id"].as_str().map(str::to_owned))
            .unwrap_or_else(|| "unknown".into());
        crate::ledger::Ledger::open_default()?.record_native_solo(&session, reason, text)
    })
}

#[cfg(test)]
pub(super) fn decide(input: &HookInput) -> Verdict {
    decide_with(input, |_, _| Ok(()))
}

pub(super) fn decide_with(
    input: &HookInput,
    mut record: impl FnMut(&str, &str) -> crate::error::NorthResult<()>,
) -> Verdict {
    let Some(text) = input.command() else {
        return Verdict::Allow;
    };
    let text = text.lossy();
    let Ok(tokens) = operator_tokens(&strip_heredocs(&text)) else {
        return Verdict::Allow;
    };
    let mut segment = Vec::new();
    let mut exceptions = Vec::new();
    for token in tokens.into_iter().chain([ShellToken::Operator(";".into())]) {
        match token {
            ShellToken::Word(word) => segment.push(word),
            ShellToken::Operator(_) => {
                let index = command(&segment, &["env", "exec", "command", "nohup"]);
                if let Some(reason) = refusal(&segment[index..]) {
                    let solo = segment[..index]
                        .iter()
                        .filter_map(|word| word.strip_prefix("NORTH_NATIVE_SOLO="))
                        .next_back()
                        .filter(|reason| !reason.trim().is_empty());
                    match solo {
                        Some(reason) => exceptions.push(reason.to_owned()),
                        None => return Verdict::Deny(reason.into()),
                    }
                }
                segment.clear();
            }
        }
    }
    if let Some(reason) = exceptions.first() {
        let _ = record(reason, &text);
    }
    Verdict::Allow
}

fn refusal(args: &[String]) -> Option<&'static str> {
    let first = args.first()?;
    let name = basename(first);
    if matches!(name, "machine-capacity" | "machine-capacity.mjs") && quiet_capture(args) {
        return Some(QUIET_DENIAL);
    }
    if matches!(name, "bun" | "bunx") {
        return refusal(bun_command(&args[1..]));
    }
    let safe = args.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "--headless" | "--dry-run" | "--plan" | "--observe"
        )
    });
    if safe {
        return None;
    }
    let tail: Vec<_> = args[1..].iter().flat_map(|arg| words(arg)).collect();
    let denied = match name {
        "wisp" | "wisp.ts" => matches!(
            tail.as_slice(),
            ["pad", ..]
                | ["native-capture", "run", ..]
                | ["integrity", "capture", ..]
                | ["lan", "soak", ..]
        ),
        "nativeCapture.ts" => matches!(tail.as_slice(), ["run" | "batch", ..]),
        "nativeCaptureBatch.ts"
        | "nativeKeyboardPad.ts"
        | "analogNative.ts"
        | "nativePadCut233.ts" => true,
        _ => false,
    };
    denied.then_some(DENIAL)
}

fn quiet_capture(args: &[String]) -> bool {
    let boundary = args
        .iter()
        .position(|arg| arg == "--")
        .unwrap_or(args.len());
    let options = &args[..boundary];
    let child = args.get(boundary + 1..).unwrap_or(&[]);
    options
        .iter()
        .any(|arg| matches!(arg.as_str(), "run" | "session"))
        && options.iter().any(|arg| arg == "--quiet")
        && (options.windows(2).any(|pair| pair == ["--class", "native"])
            || options.iter().any(|arg| arg == "--class=native"))
        && !planner_or_timing(child)
}

const QUIET_LEASE_COMMANDS: &[&[&str]] = &[&["native", "remote"], &["accept"]];

fn planner_or_timing(args: &[String]) -> bool {
    let index = command(args, &["env", "exec", "command"]);
    let mut args = &args[index..];
    if args
        .first()
        .is_some_and(|arg| matches!(basename(arg), "bun" | "bunx"))
    {
        args = bun_command(&args[1..]);
    }
    let Some(first) = args.first() else {
        return false;
    };
    if matches!(
        basename(first),
        "nativeKeyboardPad.ts" | "analogNative.ts" | "nativePadCut233.ts"
    ) {
        return true;
    }
    if !matches!(basename(first), "wisp" | "wisp.ts") {
        return false;
    }
    QUIET_LEASE_COMMANDS
        .iter()
        .any(|command| args[1..].iter().map(String::as_str).take(command.len()).eq(command.iter().copied()))
}

fn bun_command(mut args: &[String]) -> &[String] {
    while let Some(first) = args.first() {
        match first.as_str() {
            "--cwd" | "--directory" => args = args.get(2..).unwrap_or(&[]),
            "run" | "x" | "--" => args = &args[1..],
            arg if arg.starts_with('-') => args = &args[1..],
            _ => break,
        }
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn denials_name_only_runners_the_quiet_lease_accepts() {
        for text in [DENIAL, QUIET_DENIAL] {
            let named: Vec<&str> = text
                .split('`')
                .skip(1)
                .step_by(2)
                .filter(|span| span.starts_with("bun wisp "))
                .collect();
            assert!(!named.is_empty());
            for span in named {
                let mut args = ["machine-capacity", "session", "--class", "native", "--quiet", "--"]
                    .map(str::to_owned)
                    .to_vec();
                args.extend(span.split(' ').map(str::to_owned));
                assert!(!quiet_capture(&args), "{span} is not a runner the guard accepts");
            }
        }
    }

    #[test]
    fn quiet_lease_commands_exist_in_wisp() {
        let wisp: Vec<String> = serde_json::from_str(include_str!("fixtures/wisp_commands.json")).unwrap();
        for command in QUIET_LEASE_COMMANDS {
            let name = command.join(" ");
            assert!(wisp.contains(&name), "{name} is not a wisp command");
        }
    }
}
