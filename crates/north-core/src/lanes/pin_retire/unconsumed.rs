use std::fs;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;

use super::Target;
use crate::lanes::reference_search::{Scope, mentioning};
use crate::lanes::repo::{git_ok, git_out};
use crate::lanes::verdict::retires;

const SEARCH_LIMIT: Duration = Duration::from_secs(300);
const SEARCH_FILE_LIMIT: u64 = 4 << 20;
const REPORTED_HITS: usize = 20;

static FULL_OBJECT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9a-f]{40}$").unwrap());
static SHORT_OBJECT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9a-f]{7,40}$").unwrap());

impl Target {
    pub(super) fn retire_unconsumed(&self) -> Result<Vec<String>, String> {
        let leaf = self.leaf.as_str();
        if !SHORT_OBJECT.is_match(leaf) {
            return Err(format!("pin leaf is not a lowercase hex object name: {leaf}"));
        }
        self.require_main()?;
        let sidecar = self.sidecar();
        if sidecar.symlink_metadata().is_ok() {
            return Err(format!("pin has a consumer sidecar; retire it with --consumer-main: {}", sidecar.display()));
        }
        let head = self
            .registered_head()
            .filter(|head| FULL_OBJECT.is_match(head))
            .ok_or_else(|| format!("pin is not a detached registered worktree of {}: {}", self.main.display(), self.pin.display()))?;
        if self.pin_head().as_deref() != Some(head.as_str()) {
            return Err(format!("pin HEAD differs from its registration: {}", self.pin.display()));
        }
        let naming = self.naming(&head)?;
        let reach = self.reachable_from(&head);
        if !retires(!reach.is_empty(), 0) {
            return Err(format!("HEAD {head} is not reachable from origin/main, refs/archive/* or a local branch"));
        }
        let roots = self.reference_roots();
        self.refuse_references(&roots)?;
        if !retires(true, u32::from(self.dirty())) {
            return Err(format!("pin has tracked or untracked changes; refusing retirement: {}", self.pin.display()));
        }
        let mut lines = vec![
            format!("pin-retire --unconsumed proof for {}", self.pin.display()),
            format!("  (a) naming: {naming}"),
            format!("  (b) reachable from: {}", reach.iter().take(5).map(|name| format!("{name} ")).collect::<String>()),
            format!("  (c) references: none in {} mains/lanes under {}", roots.len(), self.code_root.display()),
            "  (d) worktree: clean apart from ignored output".to_string(),
        ];
        if self.dry_run {
            lines.push("pin-retire: DRY RUN — not removed".into());
            return Ok(lines);
        }
        if self.pin_head().as_deref() != Some(head.as_str()) {
            return Err(format!("pin HEAD changed during retirement proof: {}", self.pin.display()));
        }
        if sidecar.symlink_metadata().is_ok() {
            return Err(format!("a sidecar appeared during retirement proof: {}", sidecar.display()));
        }
        self.remove()?;
        lines.push(format!("retired pin {}", self.pin.display()));
        Ok(lines)
    }

    fn naming(&self, head: &str) -> Result<String, String> {
        let leaf = self.leaf.as_str();
        if head == leaf {
            return Ok(format!("HEAD {head} equals the pin name"));
        }
        if leaf.len() < 40 && head.starts_with(leaf) {
            return Ok(format!("short-named pin; HEAD {head} extends the name {leaf}"));
        }
        let other = Target { pin: self.pins_dir.join(head), ..self.clone() };
        let sidecar = self.pins_dir.join(format!("{head}.pin"));
        let valid = other.registered_head().as_deref() == Some(head) && fs::symlink_metadata(&sidecar).is_ok_and(|meta| meta.is_file());
        if !valid {
            return Err(format!("pin HEAD {head} differs from its name and has no valid pin at {}", other.pin.display()));
        }
        Ok(format!("misnamed pin; HEAD {head} already has valid pin {}", other.pin.display()))
    }

    fn reachable_from(&self, head: &str) -> Vec<String> {
        let mut command = vec!["for-each-ref", "--format=%(refname)", "--contains", head];
        if git_ok(&self.main, &["rev-parse", "--verify", "-q", "refs/remotes/origin/main"]) {
            command.push("refs/remotes/origin/main");
        }
        command.extend(["refs/archive/", "refs/heads/"]);
        git_out(&self.main, &command).unwrap_or_default().lines().map(str::to_string).collect()
    }

    fn reference_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        let mut containers = fs::read_dir(&self.code_root).map(|entries| entries.flatten().map(|entry| entry.path()).collect::<Vec<_>>()).unwrap_or_default();
        containers.sort();
        for container in containers {
            roots.push(container.join("main"));
            let mut lanes = fs::read_dir(container.join("worktrees")).map(|entries| entries.flatten().map(|entry| entry.path()).collect::<Vec<_>>()).unwrap_or_default();
            lanes.sort();
            roots.extend(lanes);
        }
        roots.retain(|root| fs::symlink_metadata(root).is_ok_and(|meta| meta.is_dir()));
        roots
    }

    fn refuse_references(&self, roots: &[PathBuf]) -> Result<(), String> {
        let leaf = self.leaf.as_str();
        let needle = regex::bytes::Regex::new(&format!("(?m)(?:^|[^0-9A-Fa-f]){leaf}(?:[^0-9A-Fa-f]|$)")).map_err(|error| error.to_string())?;
        let scope = Scope { max_file_size: Some(SEARCH_FILE_LIMIT), skip_build_output: true, limit: Some(SEARCH_LIMIT) };
        let hits = mentioning(roots, &needle, &scope).map_err(|error| format!("reference search over {} failed ({error})", self.code_root.display()))?;
        if hits.is_empty() {
            return Ok(());
        }
        let listed = hits.iter().take(REPORTED_HITS).map(|hit| format!("\n  {}", hit.display())).collect::<String>();
        Err(format!("mains or lanes under {} still reference pins/{leaf} or the bare name {leaf}{listed}", self.code_root.display()))
    }

}
