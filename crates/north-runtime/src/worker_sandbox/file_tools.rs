use std::path::{Path, PathBuf};

use super::{Context, confines_worker, removable, writable};

const CONFIG_DIR: &str = "CLAUDE_CONFIG_DIR";
const SETTINGS: [&str; 2] = ["settings.json", "settings.local.json"];

/// The first of `paths`, which a file tool of this session names, that it may not write, or `None` for a session whose shell is not confined. The boundary is the one the shell's domain applies ([`writable`]), since the Claude Code process that runs Edit and Write is never inside it; a lane the path itself names counts as the session's own, as a lane a command names does. A client's settings and hook files and its whole configuration directory are refused even where a writable tree covers them, because they decide which hooks run. A boundary that cannot be computed refuses every path.
pub fn unwritable_by_file_tool(paths: &[String], cwd: &Path) -> Option<String> {
    let profile = std::env::var_os(CONFIG_DIR).filter(|directory| !directory.is_empty()).map(PathBuf::from);
    confines_worker().then(|| outside(&Context::from_env_in(&paths.join(" "), cwd), paths, profile.as_deref())).flatten()
}

fn outside(context: &Context, paths: &[String], profile: Option<&Path>) -> Option<String> {
    let roots: Vec<PathBuf> = writable(context, &removable(context)).iter().filter_map(|root| root.canonicalize().ok()).collect();
    let profile = profile.and_then(resolved);
    let usable = context.home.is_absolute() && context.cwd.is_absolute() && !roots.is_empty();
    paths.iter().find(|path| !usable || !resolved(&context.cwd.join(path)).is_some_and(|real| roots.iter().any(|root| real.starts_with(root)) && !steers_hooks(&real, profile.as_deref()))).cloned()
}

/// Whether `real` is part of the client's configuration directory or a settings or hook file of any `.claude` directory.
fn steers_hooks(real: &Path, profile: Option<&Path>) -> bool {
    let parts: Vec<_> = real.components().map(|part| part.as_os_str().to_string_lossy().into_owned()).collect();
    let claude = parts.iter().position(|part| part == ".claude");
    profile.is_some_and(|profile| real.starts_with(profile)) || claude.is_some_and(|at| parts.get(at + 1).is_some_and(|next| SETTINGS.contains(&next.as_str()) || next == "hooks"))
}

/// `path` with its symbolic links and `..` resolved as far as it exists, the rest appended: a file the tool creates has no real path yet, and a Landlock rule holds the inode of its directory.
fn resolved(path: &Path) -> Option<PathBuf> {
    let mut tail = Vec::new();
    let mut head = path.to_path_buf();
    loop {
        if let Ok(real) = head.canonicalize() {
            return Some(tail.into_iter().rev().fold(real, |real, part| real.join(part)));
        }
        let name = head.file_name()?.to_owned();
        tail.push(name);
        head = head.parent()?.to_path_buf();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(home: &Path, lane: &Path) -> Context {
        Context::from_request("", lane, &[("HOME".into(), home.display().to_string()), ("NORTH_FLEET_SESSION".into(), "worker".into())])
    }

    #[test]
    fn a_file_tool_reaches_the_lane_and_not_the_fleets_records_queue_archive_or_a_settings_file() {
        let outside_tmp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        std::fs::create_dir_all(&outside_tmp).unwrap();
        let sandbox = tempfile::Builder::new().prefix("north-file-tools").tempdir_in(outside_tmp).unwrap();
        let home = sandbox.path().canonicalize().unwrap();
        let lane = home.join("code/north/worktrees/worker");
        let state = home.join(".local/state/north");
        for directory in [lane.join(".claude"), home.join("code/north/main"), state.join("fleet/worker"), state.join("fleet/.archive/worker-1"), state.join("land-queue/abc"), state.join("fleet/spool/sessions/other"), state.join("profiles/claude-c")] {
            std::fs::create_dir_all(directory).unwrap();
        }
        let record = state.join("fleet/worker/session.json");
        let queued = state.join("land-queue/abc/1-guards.json");
        let other_spool = state.join("fleet/spool/sessions/other/report.json");
        let own_spool = state.join("fleet/spool/sessions/worker/report.json");
        let archived = state.join("fleet/.archive/worker-1/session.json");
        let settings = state.join("profiles/claude-c/settings.json");
        let lane_settings = lane.join(".claude/settings.json");
        let lane_hook = lane.join(".claude/hooks/judge.sh");
        let profile = state.join("profiles/claude-c");
        let fine = lane.join("src/new.rs");
        let named = |path: &Path| path.display().to_string();
        let context = context(&home, &lane);
        for path in [&record, &queued, &archived, &settings, &other_spool, &lane_settings, &lane_hook] {
            assert_eq!(outside(&context, &[named(path)], Some(&profile)), Some(named(path)));
        }
        assert_eq!(outside(&context, &[named(&fine), named(&record)], Some(&profile)), Some(named(&record)));
        assert_eq!(outside(&context, &[named(&fine), "docs/readme.md".into(), named(&own_spool)], Some(&profile)), None);
        let unknown_home = Context::from_request("", &lane, &[]);
        assert_eq!(outside(&unknown_home, &[named(&fine)], None), Some(named(&fine)));
    }
}
