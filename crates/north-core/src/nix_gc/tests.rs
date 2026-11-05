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
    pub _dir: tempfile::TempDir,
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
    Home { _dir: dir, env }
}

#[test]
fn nix_gc_collects_nothing_when_a_root_cannot_be_made() {
    let home = home();
    let host = Fake { fail: Some("print-dev-env"), ..Fake::default() };
    assert_eq!(run(&[], &home.env, &host), 1);
    assert!(host.calls.borrow().iter().all(|call| !call.starts_with("nix-collect-garbage")));
}

