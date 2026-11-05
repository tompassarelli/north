use std::fs;
use std::path::Path;

use super::layout_check::{Report, conclude, scan};
use super::test_git::{git_in, init_repo};

struct Tree {
    root: tempfile::TempDir,
}

impl Tree {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let tree = Self { root };
        for name in ["alpha", "charlie", "delta", "echo"] {
            tree.container(name, None);
        }
        tree.container("bravo", Some("sha256"));
        tree
    }

    fn code(&self) -> std::path::PathBuf {
        self.root.path().canonicalize().unwrap().join("code")
    }

    fn container(&self, name: &str, object_format: Option<&str>) {
        init_repo(&self.code().join(name).join("main"), object_format);
    }

    fn main(&self, name: &str) -> std::path::PathBuf {
        self.code().join(name).join("main")
    }

    fn scan(&self) -> Report {
        scan(&self.code(), self.root.path())
    }

    fn head(&self, name: &str) -> String {
        git_in(&self.main(name), &["rev-parse", "HEAD"])
    }

    fn pin(&self, name: &str, oid: &str) -> std::path::PathBuf {
        let pin = self.code().join(name).join("pins").join(oid);
        git_in(&self.main(name), &["worktree", "add", "-q", "--detach", pin.to_str().unwrap(), oid]);
        pin
    }

    fn sidecar(&self, name: &str, oid: &str, text: &str) {
        fs::write(self.code().join(name).join("pins").join(format!("{oid}.pin")), text).unwrap();
    }
}

fn output(report: &Report) -> String {
    report.lines.join("\n")
}

#[test]
fn legal_layouts_are_clean_and_pins_are_named_as_pins() {
    let tree = Tree::new();
    let report = tree.scan();
    assert_eq!((report.strays, report.repositories, report.checked), (0, 5, 5));
    assert_eq!(conclude(&report, &tree.code()), 0);
    let lane = tree.code().join("alpha/worktrees/lane");
    git_in(&tree.main("alpha"), &["worktree", "add", "-q", "-b", "lane", lane.to_str().unwrap(), "main"]);
    let oid = tree.head("bravo");
    tree.pin("bravo", &oid);
    tree.sidecar("bravo", &oid, "Consumers: this layout test.\n");
    let report = tree.scan();
    assert_eq!((report.strays, report.pins), (0, 1), "{}", output(&report));
    assert!(output(&report).starts_with("PIN    bravo"), "{}", output(&report));
    assert!(output(&report).contains(&oid) && !output(&report).contains("STRAY"));
}

#[test]
fn out_of_format_trees_are_reported() {
    let tree = Tree::new();
    let semantic = tree.code().join("delta/pins/site");
    git_in(&tree.main("delta"), &["worktree", "add", "-q", "--detach", semantic.to_str().unwrap(), "main"]);
    assert!(output(&tree.scan()).contains("leaf is not the checkout's full object ID"));
    git_in(&tree.main("delta"), &["worktree", "remove", semantic.to_str().unwrap()]);

    let charlie = tree.head("charlie");
    let flipped = if charlie.ends_with('0') { format!("{}1", &charlie[..charlie.len() - 1]) } else { format!("{}0", &charlie[..charlie.len() - 1]) };
    let wrong = tree.code().join("charlie/pins").join(&flipped);
    git_in(&tree.main("charlie"), &["worktree", "add", "-q", "--detach", wrong.to_str().unwrap(), "main"]);
    tree.sidecar("charlie", &flipped, "Consumers: this layout test.\n");
    assert_eq!(tree.scan().strays, 1);
    git_in(&tree.main("charlie"), &["worktree", "remove", wrong.to_str().unwrap()]);
    fs::remove_file(tree.code().join("charlie/pins").join(format!("{flipped}.pin"))).unwrap();

    let delta = tree.head("delta");
    tree.pin("delta", &delta);
    assert!(output(&tree.scan()).contains("same-name consumer sidecar is missing"));
    tree.sidecar("delta", &delta, "   \n\t\n");
    assert!(output(&tree.scan()).contains("same-name consumer sidecar is empty"));
    tree.sidecar("delta", &delta, "Consumers: this layout test.\n");
    assert_eq!(tree.scan().strays, 0);

    let echo = tree.head("echo");
    let attached = tree.code().join("echo/pins").join(&echo);
    git_in(&tree.main("echo"), &["worktree", "add", "-q", "-b", "attached-pin", attached.to_str().unwrap(), &echo]);
    tree.sidecar("echo", &echo, "Consumers: this layout test.\n");
    assert!(output(&tree.scan()).contains("HEAD is attached to a branch"));

    let legacy = tree.code().join("charlie/wt-legacy");
    git_in(&tree.main("charlie"), &["worktree", "add", "-q", "-b", "legacy", legacy.to_str().unwrap(), "main"]);
    let report = tree.scan();
    assert!(output(&report).contains("STRAY  charlie        ") && output(&report).contains("charlie/wt-legacy"), "{}", output(&report));
    assert_eq!(conclude(&report, &tree.code()), 1);
}

#[test]
fn too_few_repositories_is_not_a_clean_result() {
    let small = tempfile::tempdir().unwrap();
    let report = scan(small.path(), Path::new("/nonexistent"));
    assert_eq!((report.repositories, report.strays), (0, 0));
    assert!(report.repositories < super::layout_check::MINIMUM_REPOSITORIES);
    assert_eq!(conclude(&report, small.path()), 1);
}

#[test]
fn worktrees_under_tool_owned_cache_and_state_are_not_lanes() {
    let tree = Tree::new();
    for owned in [".cache/land-offload/code-alpha-main", ".local/state/delta/capture-queue/batch/src-0"] {
        let path = tree.root.path().canonicalize().unwrap().join(owned);
        git_in(&tree.main("alpha"), &["worktree", "add", "-q", "--detach", path.to_str().unwrap(), "main"]);
    }
    assert_eq!(tree.scan().strays, 0, "{}", output(&tree.scan()));
    let tmp = tree.root.path().canonicalize().unwrap().join("tmp/lane");
    git_in(&tree.main("alpha"), &["worktree", "add", "-q", "--detach", tmp.to_str().unwrap(), "main"]);
    assert_eq!(tree.scan().strays, 1);
}
