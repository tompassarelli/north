use std::sync::mpsc;
use std::time::Duration;

use super::{path, requested, usage};
use crate::cli::tools::verbs;
use crate::cli::{parse_command, Command};

fn words(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_owned).collect()
}

#[test]
fn every_verb_in_the_table_answers_help_from_the_dispatcher_within_five_seconds() {
    for verb in verbs() {
        for ask in ["--help", "-h"] {
            let arguments = vec![verb.to_owned(), ask.to_owned()];
            let (sent, received) = mpsc::channel();
            let probe = arguments.clone();
            std::thread::spawn(move || {
                let answer = match parse_command(probe) {
                    Ok(Command::VerbHelp(path)) => usage(&path),
                    _ => None,
                };
                let _ = sent.send(answer);
            });
            let answer = received.recv_timeout(Duration::from_secs(5)).unwrap_or_else(|_| panic!("north {verb} {ask} ran past 5 s"));
            let text = answer.unwrap_or_else(|| panic!("north {verb} {ask} is not answered from a help.txt line"));
            assert!(text.contains(&format!("north {verb}")) || text.contains(&format!("Usage: {verb}")), "north {verb} {ask} printed {text}");
        }
    }
}

#[test]
fn a_subcommand_help_is_answered_before_the_subcommand_runs() {
    let arguments = words("capacity watchdog sample --interval 5 --help");
    assert_eq!(parse_command(arguments.clone()).unwrap(), Command::VerbHelp(words("capacity watchdog sample")));
    let text = usage(&path(&arguments)).unwrap();
    assert!(text.contains("north capacity watchdog sample"), "{text}");
}

#[test]
fn the_longest_listed_path_answers_and_alternatives_match() {
    let text = usage(&words("code-health gate extra")).unwrap();
    assert!(text.contains("north code-health report|gate|check"), "{text}");
    assert!(!text.contains("crate-lines"), "{text}");
    let land = usage(&words("land-actions")).unwrap();
    assert!(land.lines().count() == 3 && land.contains("Run land checks as a GitHub Actions workflow"), "{land}");
}

#[test]
fn help_after_a_command_separator_or_inside_a_wrapped_program_belongs_to_that_program() {
    assert!(!requested(&words("run-bounded 5m -- cargo --help")));
    assert!(!requested(&words("gh-session issue --help")));
    assert!(requested(&words("gh-session --help")));
    assert!(!requested(&words("capacity gate /run/shims/cargo build --help")));
    assert!(!requested(&words("capacity gate --class depbox --help")));
    assert!(requested(&words("capacity gate --help")));
    assert!(!requested(&words("resident-cap gate claude --help")));
    assert!(!matches!(parse_command(words("gh-session issue view --help")), Ok(Command::VerbHelp(_))));
    assert!(!matches!(parse_command(words("unknown-verb --help")), Ok(Command::VerbHelp(_))));
}

#[test]
fn a_verb_that_publishes_its_contract_prints_it_whole() {
    assert_eq!(usage(&words("safe-push")), Some(format!("{}\n", crate::landing::safe_push_plan::USAGE.trim_end())));
    assert_eq!(usage(&words("capacity watchdog sample")), Some(format!("{}\n", crate::capacity::WATCHDOG_USAGE)));
    assert!(usage(&words("lane-gc")).unwrap().contains("--idle-hours"));
    assert!(usage(&words("land serve")).unwrap().contains("--all"));
    assert!(usage(&words("land enqueue")).unwrap().contains("north land withdraw"));
}
