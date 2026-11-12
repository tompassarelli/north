use regex::Regex;
use std::sync::LazyLock;

const REPOSITORIES: [&str; 3] = ["tompassarelli/north", "tompassarelli/makgora", "tompassarelli/wisp"];
/// Labels on issues a workflow opens, rewrites and closes itself, so no person boxes them.
pub const WORKFLOW_OWNED: [&str; 1] = [MAIN_RED];
/// The label the Main is red workflow puts on its one tracking issue.
pub const MAIN_RED: &str = "main-red";
pub const MAIN_RED_BODY: &str = "Main CI failures and confirmed culprit decisions.\n\n## Done when\n- [ ] The newest Harness and Cargo runs on main are green on a commit after the newest culprit range below. Done when: `for w in Harness Cargo; do gh run list --branch main --workflow $w --limit 1 --json conclusion -q '.[0].conclusion' | rg -qx success || exit 1; done` exits 0; close this issue with that run's link.";

static BOX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[ \t>]*[-*][ \t]+\[([ xX])\][ \t]+Clean-up:").expect("clean-up box pattern"));

pub fn covers(repo: &str) -> bool {
    REPOSITORIES.iter().any(|covered| covered.eq_ignore_ascii_case(repo))
}

pub fn is_box(line: &str) -> bool {
    BOX.is_match(line)
}
