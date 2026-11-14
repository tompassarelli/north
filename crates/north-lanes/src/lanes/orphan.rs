use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use north_base::platform::land_queue;

use super::live_process;

pub(super) fn remove(main: &Path, lane: &Path, queues: &Path, proc_root: &Path) -> Result<String, String> {
    if std::fs::read_dir(lane).map_err(|error| error.to_string())?.next().is_none() {
        std::fs::remove_dir(lane).map_err(|error| format!("cannot delete it: {error}"))?;
        return Ok(format!("removed: {} (empty)", lane.display()));
    }
    match std::fs::read_to_string(lane.join(".git")) {
        Ok(gitfile) => {
            let gitdir = gitfile.trim().strip_prefix("gitdir: ").ok_or("its .git is not a gitfile")?;
            if Path::new(gitdir).exists() {
                return Err(format!("its git directory {gitdir} still exists, so another repository registers it"));
            }
        }
        Err(_) if lane.join(".git").exists() => return Err("its .git is a directory, so it is a clone, not a worktree".to_owned()),
        Err(_) => {}
    }
    if live_process::has_cwd_inside(proc_root, lane) {
        return Err("a live process has its cwd inside".to_owned());
    }
    if !land_queue::waiting_on(queues, &land_queue::local_host(), lane).is_empty() {
        return Err("a land ticket waits on it".to_owned());
    }
    if let Some(unstored) = unstored_file(&main.join(".git"), lane)? {
        return Err(format!("{unstored} holds content the repository does not store"));
    }
    std::fs::remove_dir_all(lane).map_err(|error| format!("cannot delete it: {error}"))?;
    Ok(format!("removed: {} (git no longer registered it and every file it held is stored in the repository)", lane.display()))
}

fn unstored_file(git_dir: &Path, lane: &Path) -> Result<Option<String>, String> {
    let scratch = tempfile::tempdir().map_err(|error| error.to_string())?;
    let git = |arguments: &[&str], input: &[u8]| -> Result<Vec<u8>, String> {
        let mut child = Command::new("git")
            .arg("--git-dir")
            .arg(git_dir)
            .arg("--work-tree")
            .arg(lane)
            .args(arguments)
            .current_dir(lane)
            .env("GIT_INDEX_FILE", scratch.path().join("index"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| error.to_string())?;
        let mut stdin = child.stdin.take().ok_or("no stdin")?;
        let input = input.to_vec();
        let writer = std::thread::spawn(move || stdin.write_all(&input));
        let output = child.wait_with_output().map_err(|error| error.to_string())?;
        let _ = writer.join();
        output.status.success().then_some(output.stdout).ok_or_else(|| format!("git {} failed", arguments.join(" ")))
    };
    let listed = git(&["ls-files", "-o", "--exclude-standard", "-z"], b"")?;
    let files: Vec<String> = listed.split(|byte| *byte == 0).filter(|name| !name.is_empty()).map(|name| String::from_utf8_lossy(name).into_owned()).collect();
    let regular: Vec<&String> = files.iter().filter(|name| std::fs::symlink_metadata(lane.join(name)).is_ok_and(|meta| meta.file_type().is_file())).collect();
    if regular.iter().any(|name| name.contains('\n')) {
        return Err("a file name holds a newline".to_owned());
    }
    let paths: String = regular.iter().map(|name| format!("{}\n", lane.join(name).display())).collect();
    let hashed = String::from_utf8_lossy(&git(&["hash-object", "--stdin-paths"], paths.as_bytes())?).into_owned();
    let object_ids: Vec<&str> = hashed.lines().collect();
    let checked = String::from_utf8_lossy(&git(&["cat-file", "--batch-check"], hashed.as_bytes())?).into_owned();
    Ok(checked.lines().position(|line| line.ends_with(" missing")).and_then(|at| regular.get(at)).map(|name| (*name).clone()).or_else(|| (object_ids.len() != regular.len()).then(|| "a file".to_owned())))
}
