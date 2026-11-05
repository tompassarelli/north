use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

fn repository_file(path: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path)).unwrap()
}

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(directory)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .args(arguments)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn mkshell(hook: &str) -> String {
    format!("{{\n  outputs = {{ nixpkgs }}: {{ devShells.x.default = pkgs.mkShell {{\n    packages = [ pkgs.shellcheck pkgs.bun ];\n    shellHook = ''\n{hook}\n    '';\n  }}; }};\n}}\n")
}

struct Scan {
    status: i32,
    output: String,
}

fn scan_changed(files: &[(&str, &str)], unchanged: &[(&str, &str)]) -> Scan {
    let root = tempfile::tempdir().unwrap();
    let path = root.path();
    git(path, &["init", "-q", "-b", "main"]);
    let write = |name: &str, text: &str| {
        let file = path.join(name);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    };
    for (name, text) in unchanged {
        write(name, text);
    }
    git(path, &["add", "-A", "."]);
    git(path, &["commit", "-q", "--allow-empty", "-m", "base"]);
    let base = git(path, &["rev-parse", "HEAD"]);
    for (name, text) in files {
        write(name, text);
    }
    git(path, &["add", "-A", "."]);
    git(path, &["commit", "-q", "--allow-empty", "-m", "change"]);
    let head = git(path, &["rev-parse", "HEAD"]);
    let output = Command::new(env!("CARGO_BIN_EXE_north"))
        .current_dir(path)
        .args(["land", "scan", "shell-hook", &format!("{base}..{head}")])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    Scan { status: output.status.code().unwrap_or(-1), output: format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr)) }
}

fn scan_flake(flake: &str) -> Scan {
    scan_changed(&[("flake.nix", flake)], &[])
}

#[test]
fn shell_hook_refuses_the_flake_that_named_target_seed_or_true() {
    let flake = repository_file("tests/it/fixtures/shell_hook/f1745de2e-flake.nix");
    assert!(flake.contains("north target-seed || true"));
    let scan = scan_flake(&flake);
    assert_eq!(scan.status, 1, "{}", scan.output);
    assert!(scan.output.contains("flake.nix") && scan.output.contains("|| true"), "{}", scan.output);
    assert!(scan.output.contains("`north` in `north target-seed || true` is not provided"), "{}", scan.output);
    assert!(scan.output.contains("`git` in"), "{}", scan.output);
}

#[test]
fn shell_hook_refuses_exit_in_every_position() {
    for hook in ["exit 1", "[ -n \"$X\" ] || exit 1", "if [ -z \"$X\" ]; then exit 0; fi", "true && exit"] {
        let scan = scan_flake(&mkshell(hook));
        assert_eq!(scan.status, 1, "{hook}: {}", scan.output);
        assert!(scan.output.contains("exits the shell"), "{hook}: {}", scan.output);
    }
}

#[test]
fn shell_hook_refuses_a_program_the_shell_does_not_provide() {
    for (hook, program) in [("north target-seed", "north"), ("x=$(git rev-parse HEAD)", "git"), ("if command -v north; then machine-capacity run; fi", "machine-capacity"), ("convo list | head -1", "convo")] {
        let scan = scan_flake(&mkshell(hook));
        assert_eq!(scan.status, 1, "{hook}: {}", scan.output);
        assert!(scan.output.contains(&format!("`{program}` in")), "{hook}: {}", scan.output);
    }
}

#[test]
fn shell_hook_accepts_builtins_stdenv_tools_and_the_shells_own_packages() {
    let hook = "checkout=$PWD\nwhile [ ! -e \"$checkout/.git\" ] && [ \"$checkout\" != / ]; do checkout=$(dirname \"$checkout\"); done\nexport PATH=\"$checkout/bin:$PATH\"\nshellcheck --version >/dev/null\nbun --version 2>&1 | head -1\n# north target-seed || true\necho \"exit\"";
    let scan = scan_flake(&mkshell(hook));
    assert_eq!(scan.status, 0, "{}", scan.output);
}

#[test]
fn shell_hook_accepts_this_repositorys_flake_and_a_flake_without_a_hook() {
    let scan = scan_flake(&repository_file("flake.nix"));
    assert_eq!(scan.status, 0, "{}", scan.output);
    let scan = scan_flake("{ outputs = { }: { devShells.x.default = pkgs.mkShell { packages = [ pkgs.bun ]; }; }; }\n");
    assert_eq!(scan.status, 0, "{}", scan.output);
}

#[test]
fn shell_hook_scans_only_the_flakes_a_range_changes() {
    let bad = mkshell("north target-seed || true");
    let untouched = scan_changed(&[("README.md", "notes\n")], &[("flake.nix", &bad)]);
    assert_eq!(untouched.status, 0, "{}", untouched.output);
    let nested = scan_changed(&[("window/flake.nix", &bad)], &[]);
    assert_eq!(nested.status, 1, "{}", nested.output);
    assert!(nested.output.contains("window/flake.nix"), "{}", nested.output);
}
