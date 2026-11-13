use std::path::Path;

use super::*;

const OPERATOR: &str = "/home/tom";

#[test]
fn a_brief_for_an_agent_login_names_the_agents_code_directory() {
    let home = login_home("agent@cpx62", Path::new(OPERATOR)).unwrap();
    let brief = "Read /home/tom/code/north/main/AGENTS.md; lane /home/tom/code/north/worktrees/x; keep /home/tom/codex and /home/tom/.config.";
    assert_eq!(
        rebase(brief, Path::new(OPERATOR), &home),
        "Read /home/agent/code/north/main/AGENTS.md; lane /home/agent/code/north/worktrees/x; keep /home/tom/codex and /home/tom/.config."
    );
}

#[test]
fn a_host_with_no_agent_login_keeps_the_brief_as_it_is() {
    assert_eq!(login_home("cpx62", Path::new(OPERATOR)), None);
    assert_eq!(login_home("tom@cpx62", Path::new(OPERATOR)), None);
    assert_eq!(login_home("@cpx62", Path::new(OPERATOR)), None);
    assert_eq!(login_home("tom@cpx62", Path::new("")), None, "an unset HOME made the operator's own login count as an agent's");
    assert_eq!(login_home("../etc@cpx62", Path::new(OPERATOR)), None, "a login naming no plain user name gave a home outside /home");
}
