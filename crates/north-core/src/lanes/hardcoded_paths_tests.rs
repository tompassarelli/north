use std::fs;
use std::path::{Path, PathBuf};

use super::hardcoded_paths::{Environment, check};
use super::test_git::{git_in, init_checkout};

const REFERENCE: &str = concat!("ref=\"$HOME/co", "de/north/main\"\n");

struct Checkouts {
    root: tempfile::TempDir,
}

impl Checkouts {
    fn new() -> Self {
        let checkouts = Self { root: tempfile::tempdir().unwrap() };
        for name in ["alpha", "beta"] {
            checkouts.make(name);
        }
        checkouts
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.path().canonicalize().unwrap().join(name).join("main")
    }

    fn make(&self, name: &str) {
        let root = self.path(name);
        fs::create_dir_all(root.join("config")).unwrap();
        fs::create_dir_all(root.join("dotfiles/bin")).unwrap();
        fs::create_dir_all(root.join("filler")).unwrap();
        fs::write(root.join("dotfiles/bin/sample"), REFERENCE).unwrap();
        for index in 0..120 {
            fs::write(root.join(format!("filler/f{index}.txt")), "x\n").unwrap();
        }
        fs::write(root.join("config/hardcoded-repo-paths.tsv"), format!("# fixture allowlist\n{name}/dotfiles/bin/sample\t1\n")).unwrap();
        init_checkout(&root);
    }

    fn allowlist(&self, name: &str) -> PathBuf {
        self.path(name).join("config/hardcoded-repo-paths.tsv")
    }

    fn environment(&self, cwd: &Path, scan: &Path) -> Environment {
        Environment {
            cwd: cwd.to_path_buf(),
            home: self.root.path().to_path_buf(),
            root_override: None,
            roots_override: Some(scan.to_string_lossy().into_owned()),
        }
    }

    fn run(&self, caller: &str, write_allow: bool) -> u8 {
        let caller = self.path(caller);
        check(&self.environment(&caller, &caller), write_allow).unwrap_or_else(|failure| panic!("{}", failure.0))
    }
}

fn set_count(path: &Path, from: &str, to: &str) {
    let text = fs::read_to_string(path).unwrap();
    fs::write(path, text.replace(&format!("\t{from}\n"), &format!("\t{to}\n"))).unwrap();
}

#[test]
fn the_callers_checkout_decides_the_result_not_the_first_one() {
    let checkouts = Checkouts::new();
    set_count(&checkouts.allowlist("beta"), "1", "999");
    assert_eq!(checkouts.run("beta", false), 2);
    assert_eq!(checkouts.run("alpha", false), 0);

    let outside = checkouts.root.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    let alpha = checkouts.path("alpha");
    let failure = check(&checkouts.environment(&outside, &alpha), false).err().unwrap();
    assert!(failure.0.starts_with("missing allowlist"), "{}", failure.0);

    let mut environment = checkouts.environment(&checkouts.path("beta"), &alpha);
    environment.root_override = Some(alpha.clone());
    assert_eq!(check(&environment, false).ok(), Some(0));
}

#[test]
fn write_allow_refreshes_and_prunes_the_callers_allowlist_only() {
    let checkouts = Checkouts::new();
    let beta = checkouts.path("beta");
    let mut sample = fs::read_to_string(beta.join("dotfiles/bin/sample")).unwrap();
    sample.push_str(REFERENCE);
    fs::write(beta.join("dotfiles/bin/sample"), sample).unwrap();
    fs::write(beta.join("filler/f1.txt"), REFERENCE).unwrap();
    let list = checkouts.allowlist("beta");
    fs::write(&list, format!("{}beta/filler/gone.txt\t3\n", fs::read_to_string(&list).unwrap())).unwrap();
    git_in(&beta, &["commit", "-qam", "drift"]);
    let alpha_before = fs::read_to_string(checkouts.allowlist("alpha")).unwrap();
    assert_eq!(checkouts.run("beta", true), 1);
    let rewritten = fs::read_to_string(&list).unwrap();
    assert!(rewritten.ends_with("beta/dotfiles/bin/sample\t2\n"), "{rewritten}");
    assert!(!rewritten.contains("gone.txt") && !rewritten.contains("f1.txt"));
    assert_eq!(fs::read_to_string(checkouts.allowlist("alpha")).unwrap(), alpha_before);
    fs::remove_file(beta.join("filler/f1.txt")).unwrap();
    git_in(&beta, &["commit", "-qam", "undo"]);
    assert_eq!(checkouts.run("beta", false), 0);
}

#[test]
fn the_scan_counts_unmarked_references_and_refuses_a_thin_tree() {
    use std::os::unix::fs::PermissionsExt;
    let checkouts = Checkouts::new();
    let alpha = checkouts.path("alpha");
    fs::write(alpha.join("filler/blob.bin"), [b"\0 ", REFERENCE.as_bytes()].concat()).unwrap();
    git_in(&alpha, &["add", "filler"]);
    git_in(&alpha, &["commit", "-qm", "binary"]);
    assert_eq!(checkouts.run("alpha", false), 0);
    let marked = format!("{}# hardcoded-repo-path:allow\n", REFERENCE.trim_end());
    fs::write(alpha.join("filler/marked.txt"), marked).unwrap();
    let tilde = concat!("~/co", "de/north/x\n");
    fs::write(alpha.join("filler/plain.txt"), tilde).unwrap();
    let script = alpha.join("filler/script.sh");
    fs::write(&script, tilde).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    git_in(&alpha, &["add", "filler"]);
    git_in(&alpha, &["commit", "-qm", "more"]);
    assert_eq!(checkouts.run("alpha", false), 1);
    for index in 0..40 {
        git_in(&alpha, &["rm", "-q", &format!("filler/f{index}.txt")]);
    }
    let failure = check(&checkouts.environment(&alpha, &alpha), false).err().unwrap();
    assert!(failure.0.contains("scanned only"), "{}", failure.0);
}

#[test]
fn allowlist_lines_are_trimmed_and_malformed_entries_refuse_to_report() {
    let checkouts = Checkouts::new();
    let list = checkouts.allowlist("alpha");
    fs::write(&list, "  # padded comment\n\nalpha/dotfiles/bin/sample\t1 \n").unwrap();
    assert_eq!(checkouts.run("alpha", false), 0);
    for malformed in ["alpha/dotfiles/bin/sample\t0\n", "alpha/dotfiles/bin/sample\n", "\t3\n", "alpha/dotfiles/bin/sample\t1\r\n"] {
        fs::write(&list, malformed).unwrap();
        let failure = check(&checkouts.environment(&checkouts.path("alpha"), &checkouts.path("alpha")), false).err().unwrap();
        assert!(failure.0.starts_with("malformed allowlist entry"), "{malformed:?}: {}", failure.0);
    }
    fs::write(&list, "alpha/dotfiles/bin/sample\t1\nalpha/dotfiles/bin/sample\t1\n").unwrap();
    let failure = check(&checkouts.environment(&checkouts.path("alpha"), &checkouts.path("alpha")), false).err().unwrap();
    assert!(failure.0.starts_with("duplicate allowlist entry"), "{}", failure.0);
}
