use std::cell::LazyCell;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use serde_json::json;

use crate::fleet::records::read_record;
use crate::fleet::runtime::SESSION_ENV;
use crate::foundation::clock::Timestamp;
use crate::hook_input::{HookInput, Verdict};

const RECORDS: &str = "agents/hooks/container-shell.jsonl";

/// The session of this hook when its launch declared `--container-shell`, so its shell tool runs in a task container and a command it runs there touches nothing of this host. The session's record says so, which its launch writes and no worker command can; nothing in the command or its environment does.
pub fn declared() -> Option<ContainerSession> {
    let session = std::env::var(SESSION_ENV).ok().filter(|session| !session.is_empty() && !session.contains('/'));
    session.zip(crate::fleet::registry::root_here().ok()).and_then(|(session, root)| declared_in(&root, &session))
}

pub(crate) fn declared_in(root: &Path, session: &str) -> Option<ContainerSession> {
    let record = read_record(&root.join(session)).ok().filter(|record| record.container_shell)?;
    Some(ContainerSession { container_dir: record.container_dir, own_dir: record.cwd })
}

const PATH_KEYS: [&str; 3] = ["file_path", "filePath", "notebook_path"];

/// A session launched with `--container-shell`: `container_dir` is where its own directory `own_dir` is mounted in the task's container, when the launch named it.
pub struct ContainerSession {
    container_dir: Option<PathBuf>,
    own_dir: PathBuf,
}

/// The rewrite of a pre-tool-use file tool call of a container session whose launch named the container directory, when a path of the call lies under it, else `Allow`; the session's record is read only for a file tool call.
pub fn moved_file_call<F: FnOnce() -> Option<ContainerSession>>(event: &str, input: &HookInput, session: &LazyCell<Option<ContainerSession>, F>) -> Verdict {
    let file_tool = event == "PreToolUse" && matches!(input.tool_name(), "Edit" | "Write" | "MultiEdit" | "NotebookEdit");
    file_tool.then(|| session.as_ref()).flatten().and_then(|session| session.rewrite(input)).unwrap_or(Verdict::Allow)
}

impl ContainerSession {
    /// The file tool call `input` with each path under the container directory moved to the same relative path under the session's own directory, since the file tools run on this host, where only the session's directory holds those files; `None` when nothing moves.
    pub fn rewrite(&self, input: &HookInput) -> Option<Verdict> {
        let container = self.container_dir.as_deref()?;
        let mut updated = input.tool_input.clone();
        let mut moved = Vec::new();
        for key in PATH_KEYS {
            let Some(own) = input.tool_input[key].as_str().and_then(|path| self.own_path(container, Path::new(path))) else { continue };
            moved.push(format!("`{}` to `{}`", input.tool_input[key].as_str().unwrap_or_default(), own.display()));
            updated[key] = own.display().to_string().into();
        }
        (!moved.is_empty()).then(|| Verdict::Rewrite(updated, format!("container-shell: the file tools run on this host, where the task's directory {} is this session's directory {}; this call went from {}.", container.display(), self.own_dir.display(), moved.join(", "))))
    }

    /// `path` under the session's own directory when it lies under `container` by its words alone and its place there stays inside that directory through every symlink.
    fn own_path(&self, container: &Path, path: &Path) -> Option<PathBuf> {
        let relative = path.strip_prefix(container).ok().filter(|relative| relative.components().all(|part| matches!(part, Component::Normal(_))))?;
        let own = self.own_dir.join(relative);
        let mut existing = own.as_path();
        while existing.symlink_metadata().is_err() {
            existing = existing.parent()?;
        }
        std::fs::canonicalize(existing).ok()?.starts_with(std::fs::canonicalize(&self.own_dir).ok()?).then_some(own)
    }
}

/// Appends what each guard that stood down for `input` would have said to the hooks' records under the state directory, where an eval's runner reads which guards did not apply to its session.
pub fn record(input: &HookInput, stood_down: &[String]) {
    let state = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).filter(|state| state.is_absolute()).or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")));
    let Some(file) = state.filter(|_| !stood_down.is_empty()).map(|state| state.join(RECORDS)) else { return };
    let at = Timestamp::now().iso();
    let lines: String = stood_down.iter().map(|said| format!("{}\n", json!({"at": at, "tool": input.tool_name(), "command": input.command(), "said": said}))).collect();
    let _ = file.parent().map(std::fs::create_dir_all);
    if let Ok(mut records) = std::fs::OpenOptions::new().create(true).append(true).open(file) {
        let _ = records.write_all(lines.as_bytes());
    }
}
