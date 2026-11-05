//! `north doctor nix`: what a Nix footgun would break on this host, before an agent finds it.
mod lock;

use crate::nix_gc::{Env, Host, live_shells, missing_rustup, offline_args, rustup_references};
use std::fs;
const HASH: usize = 32;
const USAGE: &str = "usage: north doctor nix [--lock FILE [--allow FILE]]";

pub use lock::{Allow, Second, allowlist, check as lock_refusals, refusals, second_nixpkgs};

fn lock_command(file: &str, allow: Option<&str>) -> u8 {
    let read = |path: &str| fs::read_to_string(path).map_err(|error| format!("{path}: {error}"));
    let allow_text = match allow.map(read).transpose() {
        Ok(text) => text,
        Err(error) => return fail(&error),
    };
    match read(file).and_then(|text| lock::check(&text, allow_text.as_deref())) {
        Ok(refused) if refused.is_empty() => {
            println!("doctor nix: {file} brings no second nixpkgs");
            0
        }
        Ok(refused) => {
            for line in refused {
                eprintln!("doctor nix: {file}: {line}; make the input follow the flake's nixpkgs, or add an [[allow]] entry with its reason");
            }
            1
        }
        Err(error) => fail(&error),
    }
}

fn fail(message: &str) -> u8 {
    eprintln!("doctor nix: {message}");
    2
}

pub fn run(arguments: &[String], env: &Env, host: &dyn Host) -> u8 {
    let mut lock_file = None;
    let mut allow = None;
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        match (word.as_str(), words.next()) {
            ("--lock", Some(file)) => lock_file = Some(file.as_str()),
            ("--allow", Some(file)) => allow = Some(file.as_str()),
            _ => {
                eprintln!("{USAGE}");
                return 2;
            }
        }
    }
    match lock_file {
        Some(file) => lock_command(file, allow),
        None => print_probe(env, host),
    }
}

pub struct Finding {
    pub ok: bool,
    pub text: String,
}

pub fn probe(env: &Env, host: &dyn Host) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut report = |ok: bool, text: String| findings.push(Finding { ok, text });
    for shell in live_shells(&env.code) {
        let rooted = env.dev_shell_root(&shell).exists();
        let more = if shell.also > 0 { format!(" (+{} worktrees)", shell.also) } else { String::new() };
        report(rooted, format!("dev shell {} {}{more}: {}", shell.name, shell.worktree.display(), if rooted { "rooted" } else { "not rooted, `north nix gc` roots it" }));
        let entered = host.run("nix", &offline_args(&["develop", &shell.worktree.display().to_string()], &["-c", "true"]));
        report(entered.is_ok(), format!("dev shell {} enters offline{}", shell.name, entered.err().map(|error| format!(": {error}")).unwrap_or_default()));
        let lock = shell.worktree.join("flake.lock");
        if let Ok(text) = fs::read_to_string(&lock) {
            match lock::check(&text, None) {
                Ok(second) => second.iter().for_each(|line| report(false, format!("{}: {line}", lock.display()))),
                Err(error) => report(false, error),
            }
        }
    }
    let references = rustup_references(&env.rustup_home);
    for reference in missing_rustup(&references) {
        let what = reference.path.strip_prefix("/nix/store/").and_then(|rest| rest.get(HASH + 1..)).unwrap_or(&reference.path);
        report(false, format!("rustup toolchain {} needs {what}: {} names {}, which is not in the store", reference.toolchain, reference.file.display(), reference.path));
    }
    if references.is_empty() {
        report(true, "no rustup toolchain wrapper names a store path".into());
    }
    findings
}

fn print_probe(env: &Env, host: &dyn Host) -> u8 {
    let findings = probe(env, host);
    for finding in &findings {
        println!("{} {}", if finding.ok { "ok  " } else { "FAIL" }, finding.text);
    }
    let problems = findings.iter().filter(|finding| !finding.ok).count();
    println!("doctor nix: {problems} problems");
    u8::from(problems > 0)
}

#[cfg(test)]
mod tests;
