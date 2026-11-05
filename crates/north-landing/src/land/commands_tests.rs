use std::collections::BTreeSet;

use super::{COMMANDS, usage};

const TABLE: &str = include_str!("commands.rs");

#[test]
fn the_command_table_is_one_sorted_line_per_subcommand_so_the_merge_driver_keeps_it_sorted() {
    let lines = TABLE.lines().collect::<Vec<_>>();
    assert_eq!(lines.first(), Some(&"&["));
    assert_eq!(lines.last(), Some(&"]"));
    assert!(lines.windows(2).all(|pair| pair[0] < pair[1]), "commands.rs lines must stay strictly sorted");
    assert_eq!(lines.len(), COMMANDS.len() + 2);
    for (line, command) in lines[1..lines.len() - 1].iter().zip(COMMANDS) {
        assert!(line.starts_with(&format!("Command {{ name: \"{}\", ", command.name)), "{line}");
    }
}

#[test]
fn help_keeps_every_command_of_the_usage_line_it_replaced_and_one_line_per_listed_command() {
    let replaced = "usage: north land checks --base BASE --head HEAD [--dry 0|1] [--safe-push PATH] [--red-fix ISSUE] [--outcome FILE]\n       north land serve [--once] [--all] [--repo DIR] [--checker PROGRAM] [--depth D] [--app-token PROGRAM]\n       north land enqueue [--repo DIR] [--message FILE]\n       north land urgent TICKET --reason TEXT [--repo DIR]\n       north land withdraw TICKET --reason TEXT [--repo DIR] [--noticed-by WHO] [--class CLASS]\n       north land scan dep-gate|tool-removal|test-integrity BASE..HEAD\n       north land watchdog --once [--queue-root DIR]\n       north land promote [--dry-run] [--shadow-minutes N] [--repo DIR]... [--checker PROGRAM]\n       north land replay --base REV [--repo DIR] [--checker PROGRAM] TICKET...\n       north land reuse hash|record|lookup";
    let lines = |text: &str| text.trim_start_matches("usage: ").lines().map(|line| line.trim().to_owned()).collect::<BTreeSet<_>>();
    let help = usage();
    assert!(help.starts_with("usage: north land "));
    assert!(help.contains("north land model [--repo DIR] [--era batch|precheck] [--base REF] [--json]"), "{help}");
    assert!(lines(replaced).is_subset(&lines(&help)), "{help}");
    assert_eq!(help.lines().count(), COMMANDS.iter().filter(|command| !command.usage.is_empty()).count());
}

#[test]
fn every_subcommand_name_is_unique_and_follow_stays_unlisted() {
    let names = COMMANDS.iter().map(|command| command.name).collect::<BTreeSet<_>>();
    assert_eq!(names.len(), COMMANDS.len());
    assert!(names.contains("follow"));
    assert!(!usage().contains("land follow"));
}
