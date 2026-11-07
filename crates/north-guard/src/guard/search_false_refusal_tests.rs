#![cfg(test)]

use super::{corpus_scan, resource_safe_search};

const HOME: &str = "/home/tester";
const CWD: &str = "/home/tester/code/project/main";

#[test]
fn resource_safe_search_judges_operands_against_the_cd_target_and_not_redirects_or_a_pipe() {
    let container = tempfile::tempdir().unwrap();
    let root = container.path().to_str().unwrap();
    std::fs::create_dir(container.path().join("main")).unwrap();
    std::fs::create_dir(container.path().join("worktrees")).unwrap();
    let allowed = [
        "cd /home/tester/code/makgora/sc-1117/ts && rg -n \"gate-panel\" --glob '*.ts' .. 2>/dev/null | head".to_owned(),
        "cd /home/tester/code/north/worktrees/fast-lane/crates/north-landing/src/land && grep -rn \"post_land\" --include=*.rs .. | head".to_owned(),
        "cd ~/.local/state/north/profiles; fd -e jsonl --changed-within 31d . claude-b/projects > /dev/null".to_owned(),
        format!("cd {root}/main && cd $L && rg -n foo .."),
        format!("cd {root}/main/.. && cd {root}/worktrees && rg -n foo ."),
        "git log --oneline | rg fix".to_owned(),
    ];
    let refused = [
        format!("cd {root}/main && rg -n foo .."),
        format!("cd {root} && rg -n foo"),
        format!("rg -l foo {root}"),
        format!("cd {root}/worktrees && grep -rn post_land {root} 2>/dev/null"),
        "cd /proc && rg foo .".to_owned(),
        "rg foo /dev/null".to_owned(),
    ];
    for command in allowed {
        assert_eq!(resource_safe_search::refusal(&command, CWD, HOME), None, "{command}");
    }
    for command in refused {
        assert!(resource_safe_search::refusal(&command, CWD, HOME).is_some(), "{command}");
    }
}

#[test]
fn corpus_scan_matches_a_glob_only_against_the_names_it_can_match_and_reads_a_pipe_and_fd_depth() {
    let allowed = [
        "cd ~/.local/state/north && rg -l placement land-queue* collected-land 2>/dev/null | head",
        "cd ~/.local/state/north && ls -t | rg -i fleet | head",
        "fd -d3 -e db . ~/.local/state/north ~/.local/state/agents 2>/dev/null | head",
        "cd ~/.local/state/north && fd -d2 -t f foo",
    ];
    let refused = [
        "cd ~/.local/state/north && rg -l foo .",
        "rg -l foo ~/.local/state/north/a*",
        "rg -l foo ~/.local/state/north/acc?unts",
        "fd -d4 -e db . ~/.local/state/north",
        "cd ~/.local/state/north && rg -l foo | head",
    ];
    for command in allowed {
        assert_eq!(corpus_scan::refusal(command, CWD, HOME), None, "{command}");
    }
    for command in refused {
        assert!(corpus_scan::refusal(command, CWD, HOME).is_some(), "{command}");
    }
}
