use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::NorthResult;
use crate::platform::{git, harness};

use super::{home, operator_dir, repo_root};

const NOT_PUBLISHED: &str = "agents sync: generation not published";

pub fn operator_directory() -> NorthResult<PathBuf> {
    operator_dir(env::var_os("AGENTS_OPERATOR_DIR"), repo_root)
}

pub fn run(arguments: &[String]) -> NorthResult<u8> {
    let operator = operator_directory()?;
    let Some(south) = toplevel(&operator) else {
        eprintln!("agents sync: operator directory is not in a Git checkout");
        return Ok(1);
    };
    let north = north_checkout(
        env::var("NORTH_REPO_ROOTS").ok().as_deref(),
        env::var_os("NORTH_HOME"),
        &home()?,
    )?;
    let mut say = |line: String| eprintln!("{line}");
    let current = current_main("south", &south, &mut say)
        .and_then(|()| current_main("north", &north, &mut say));
    if let Err(line) = current {
        eprintln!("{line}");
        eprintln!("{NOT_PUBLISHED}");
        return Ok(1);
    }
    let harness = harness::harness_root().map_err(crate::error::NorthError::Configuration)?;
    if !policy_gate(&repo_root("nixos-config")?, &operator) {
        eprintln!("{NOT_PUBLISHED}");
        return Ok(1);
    }
    super::run(arguments)?;
    if !Command::new("python3")
        .arg("-I")
        .arg(harness.join("harness/lib/claude_profiles.py"))
        .status()
        .is_ok_and(|status| status.success())
    {
        return Ok(1);
    }
    let _ = Command::new(harness.join("bin/effect-kit"))
        .arg("scan")
        .status();
    Ok(0)
}

fn toplevel(directory: &Path) -> Option<PathBuf> {
    let output = git::run(directory, &["rev-parse", "--show-toplevel"]).ok()?;
    output
        .status
        .success()
        .then(|| PathBuf::from(String::from_utf8_lossy(&output.stdout).trim()))
}

fn policy_gate(nixos_config: &Path, operator: &Path) -> bool {
    crate::agent_config_check::policy_gate_session(nixos_config, operator)
}

pub(super) fn current_main(
    name: &str,
    directory: &Path,
    say: &mut impl FnMut(String),
) -> Result<(), String> {
    let git = |args: &[&str]| {
        git::run(directory, args)
            .ok()
            .filter(|output| output.status.success())
    };
    let count = |args: &[&str]| {
        git(args).and_then(|output| {
            String::from_utf8_lossy(&output.stdout)
                .trim()
                .parse::<u64>()
                .ok()
        })
    };
    let fetched = git(&["fetch", "-q", "origin", "main"]).is_some();
    let mut behind = 0;
    if fetched {
        behind = count(&["rev-list", "--count", "HEAD..origin/main"])
            .ok_or_else(|| format!("{name} main: cannot count commits behind origin"))?;
        if behind == 0 {
            return Ok(());
        }
    } else {
        say(format!(
            "agents sync: {name} fetch failed; using local main"
        ));
    }
    let ahead = count(&["rev-list", "--count", "origin/main..HEAD"]).unwrap_or(0);
    let branch = git(&["symbolic-ref", "-q", "--short", "HEAD"])
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned());
    let dirty = git(&["status", "--porcelain", "--untracked-files=no"])
        .is_some_and(|output| !output.stdout.is_empty());
    let reason = if branch.as_deref() != Some("main") {
        "checkout is not on main".to_owned()
    } else if ahead > 0 {
        format!("diverged with {ahead} local commits")
    } else if dirty {
        "uncommitted changes".to_owned()
    } else if !fetched || git(&["merge", "-q", "--ff-only", "origin/main"]).is_some() {
        return Ok(());
    } else {
        "git merge --ff-only failed".to_owned()
    };
    Err(match fetched {
        true => {
            format!("{name} main is behind origin by {behind} and not fast-forwardable: {reason}")
        }
        false => format!("{name} main refused offline: {reason}"),
    })
}

pub(super) fn north_checkout(
    roots: Option<&str>,
    north_home: Option<std::ffi::OsString>,
    home: &Path,
) -> NorthResult<PathBuf> {
    let configured = match roots {
        Some(roots) => serde_json::from_str::<serde_json::Value>(roots)?
            .get("north")
            .and_then(|path| path.as_str().map(PathBuf::from)),
        None => None,
    };
    Ok(configured
        .or_else(|| {
            north_home
                .filter(|path| !path.is_empty())
                .map(PathBuf::from)
        })
        .unwrap_or_else(|| home.join("code/north/main")))
}
