use super::*;
use std::cell::RefCell;
use std::process::Command;

const HASH: &str = "0123456789abcdfghijklmnpqrsvwxyz";
const WRAPPER_HASH: &str = "abcdfghijklmnpqrsvwxyz0123456789";

pub fn wrapper() -> String {
    format!("/nix/store/{WRAPPER_HASH}-rustup-1.29.0/nix-support/ld-wrapper.sh")
}

#[derive(Default)]
pub struct Fake {
    pub calls: RefCell<Vec<String>>,
    pub fail: Option<&'static str>,
}

impl Host for Fake {
    fn run(&self, program: &str, args: &[String]) -> Result<String, String> {
        self.calls.borrow_mut().push(format!("{program} {}", args.join(" ")));
        match self.fail {
            Some(word) if args.iter().any(|arg| arg == word) => Err(format!("{word} failed")),
            _ => Ok(String::new()),
        }
    }
}

fn git(directory: &Path, args: &[&str]) {
    let status = Command::new("git").arg("-C").arg(directory).args(["-c", "user.name=t", "-c", "user.email=t@t"]).args(args).status().unwrap();
    assert!(status.success(), "git {args:?}");
}

pub struct Home {
    pub dir: tempfile::TempDir,
    pub env: Env,
}

pub fn home() -> Home {
    let dir = tempfile::tempdir().unwrap();
    let env = Env { code: dir.path().join("code"), state: dir.path().join("state/north"), rustup_home: dir.path().join(".rustup") };
    let main = env.code.join("proj/main");
    fs::create_dir_all(&main).unwrap();
    git(&main, &["init", "-q", "-b", "main"]);
    fs::write(main.join("flake.nix"), "{ outputs = _: { devShells.x86_64-linux.default = 1; }; }").unwrap();
    git(&main, &["add", "."]);
    git(&main, &["commit", "-q", "-m", "x"]);
    git(&main, &["worktree", "add", "-q", "-b", "lane", env.code.join("proj/worktrees/lane").to_str().unwrap()]);
    let runtime = env.state.join("runtimes/37ab206be579b9a14f624df065ce5da2e18c08d9");
    fs::create_dir_all(&runtime).unwrap();
    fs::write(runtime.join("north"), format!("\0/nix/store/{HASH}-glibc-2.42-84/lib/ld.so\0")).unwrap();
    let wrappers = env.rustup_home.join("toolchains/1.96-x86_64-unknown-linux-gnu/lib/rustlib/x86_64-unknown-linux-gnu/bin/gcc-ld");
    fs::create_dir_all(&wrappers).unwrap();
    fs::write(wrappers.join("ld.lld"), format!("#!/bin/sh\nexec {} \"$@\"\n", wrapper())).unwrap();
    Home { dir, env }
}

#[test]
fn nix_gc_scan_finds_each_store_path_root_once_per_mention() {
    let bytes = format!("\0/nix/store/{HASH}-glibc-2.42-84/lib/ld.so\0/nix/store/short-x\0/nix/store/{HASH}-gcc-15.3.0-lib:");
    assert_eq!(store_roots(bytes.as_bytes()), [format!("/nix/store/{HASH}-glibc-2.42-84"), format!("/nix/store/{HASH}-gcc-15.3.0-lib")]);
    assert_eq!(store_paths(bytes.as_bytes())[0], format!("/nix/store/{HASH}-glibc-2.42-84/lib/ld.so"));
}

#[test]
fn nix_gc_dry_run_lists_every_live_dev_shell_runtime_and_the_rustup_ld_wrapper_as_roots() {
    let home = home();
    let roots = plan(&home.env);
    let text = render(&roots);
    assert!(text.contains(&format!("rustup    1.96-x86_64-unknown-linux-gnu {} (missing from the store)", wrapper())), "{text}");
    assert!(text.contains(&format!("runtime   37ab206b /nix/store/{HASH}-glibc-2.42-84")), "{text}");
    assert_eq!(text.matches("dev-shell").count(), 1, "{text}");
    assert!(text.contains("(+1 worktrees)") && text.contains("(to root)"), "{text}");
    let host = Fake::default();
    assert_eq!(run(&["--dry-run".into()], &home.env, &host), 0);
    assert!(host.calls.borrow().is_empty());
}

#[test]
fn nix_gc_roots_every_dev_shell_offline_before_it_collects_with_max_freed() {
    let home = home();
    let host = Fake::default();
    assert_eq!(run(&["--max-freed".into(), "5G".into()], &home.env, &host), 0);
    let calls = host.calls.borrow();
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert!(calls[0].starts_with("nix print-dev-env ") && calls[0].contains("--offline --no-update-lock-file --profile "), "{calls:?}");
    assert_eq!(calls[1], "nix-collect-garbage --max-freed 5G");
}

#[test]
fn nix_gc_collects_nothing_when_a_root_cannot_be_made() {
    let home = home();
    let host = Fake { fail: Some("print-dev-env"), ..Fake::default() };
    assert_eq!(run(&[], &home.env, &host), 1);
    assert!(host.calls.borrow().iter().all(|call| !call.starts_with("nix-collect-garbage")));
}

#[test]
fn nix_gc_roots_a_present_store_path_through_add_root() {
    let home = home();
    let store = home.dir.path().join("store-path");
    fs::create_dir_all(&store).unwrap();
    let root = Root { kind: Kind::Runtime, owner: "x".into(), source: store.display().to_string(), store: Some(store.display().to_string()), link: home.env.state.join("r/gcroot-x") };
    let host = Fake::default();
    root_one(&root, &host).unwrap();
    assert_eq!(host.calls.borrow()[0], format!("nix-store --add-root {} --indirect --realise {}", root.link.display(), store.display()));
}

#[test]
fn nix_gc_prunes_dev_shell_roots_of_removed_worktrees_only() {
    let home = home();
    let roots = home.env.dev_shell_roots();
    fs::create_dir_all(&roots).unwrap();
    let plan = plan(&home.env);
    let live = plan.iter().find(|root| root.kind == Kind::DevShell).unwrap().link.clone();
    let gone = roots.join("proj-0000000000000000");
    for link in [&live, &gone] {
        std::os::unix::fs::symlink("/nonexistent", link).unwrap();
        std::os::unix::fs::symlink("/nonexistent", format!("{}-1-link", link.display())).unwrap();
    }
    prune(&home.env, &plan);
    assert!(live.symlink_metadata().is_ok() && gone.symlink_metadata().is_err());
    assert!(PathBuf::from(format!("{}-1-link", live.display())).symlink_metadata().is_ok());
    assert!(PathBuf::from(format!("{}-1-link", gone.display())).symlink_metadata().is_err());
}
