use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use sha2::{Digest, Sha256};

use super::{Consumer, ProofError, Target, consumer_proof, expand};
use crate::lanes::reference_search::{Scope, mentioning};
use crate::lanes::repo::{git_ok, git_out};
use crate::lanes::verdict::retires;
use crate::platform::git;

static FULL_OBJECT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9a-f]{40}$").unwrap());
static CONSUMER_RECORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^consumer-main:\s*(.*\S)\s*$").unwrap());

impl Target {
    pub(super) fn retire_consumed(&self, home: &Path, arguments: &[String]) -> Result<Vec<String>, String> {
        let oid = self.leaf.as_str();
        if !FULL_OBJECT.is_match(oid) {
            return Err(format!("pin leaf is not one lowercase full object ID: {oid}"));
        }
        self.require_main()?;
        if self.main.canonicalize().ok().as_deref() != Some(self.main.as_path()) {
            return Err(format!("main checkout path is not canonical: {}", self.main.display()));
        }
        let sidecar = self.sidecar();
        if !fs::symlink_metadata(&sidecar).is_ok_and(|meta| meta.is_file()) {
            return Err(format!("pin needs a regular same-name consumer sidecar: {}", sidecar.display()));
        }
        let text = fs::read_to_string(&sidecar).map_err(|error| format!("cannot read {}: {error}", sidecar.display()))?;
        if text.trim().is_empty() {
            return Err(format!("consumer sidecar is empty: {}", sidecar.display()));
        }
        let digest = Sha256::digest(text.as_bytes());
        let declared = self.declared_consumers(home, &text, &sidecar)?;
        self.require_pin_state(oid)?;
        let consumers = arguments.iter().map(|raw| self.published_consumer(home, raw)).collect::<Result<Vec<_>, _>>()?;
        let supplied = consumers.iter().map(|consumer| consumer.main.to_string_lossy().into_owned()).collect::<Vec<_>>();
        consumer_proof(&supplied, &declared).map_err(|error| match error {
            ProofError::DuplicateArguments => "duplicate --consumer-main arguments are not allowed".to_string(),
            ProofError::DuplicateSidecar => format!("duplicate consumer-main records in sidecar: {}", sidecar.display()),
            ProofError::Mismatch => format!("--consumer-main arguments do not exactly match the sidecar: {}", sidecar.display()),
        })?;
        for consumer in &consumers {
            self.refuse_consumer_worktrees(&consumer.main, oid)?;
        }
        if self.dry_run {
            let flat = text.replace(['\n', '\t'], " ");
            return Ok(vec!["pin-retire: DRY RUN — verified clean detached pin".into(), format!("pin: {}\nconsumers: {flat}", self.pin.display())]);
        }
        self.recheck(&consumers, &sidecar, &digest, oid)?;
        self.remove()?;
        if self.registered() {
            return Err(format!("worktree removal left the pin registered: {}", self.pin.display()));
        }
        if !fs::symlink_metadata(&sidecar).is_ok_and(|meta| meta.is_file()) {
            return Err(format!("sidecar changed during worktree removal: {}", sidecar.display()));
        }
        if Sha256::digest(fs::read(&sidecar).unwrap_or_default()) != digest {
            return Err(format!("consumer sidecar changed during worktree removal: {}", sidecar.display()));
        }
        fs::remove_file(&sidecar).map_err(|_| format!("sidecar removal failed: {}", sidecar.display()))?;
        Ok(vec![format!("retired pin {}", self.pin.display())])
    }

    fn declared_consumers(&self, home: &Path, text: &str, sidecar: &Path) -> Result<Vec<String>, String> {
        let records = text.lines().filter(|line| line.starts_with("consumer-main:")).collect::<Vec<_>>();
        if records.is_empty() {
            return Err(format!("sidecar has no machine-readable consumer-main records: {}", sidecar.display()));
        }
        let paths = records.iter().filter_map(|record| CONSUMER_RECORD.captures(record).map(|found| found[1].to_string())).collect::<Vec<_>>();
        if paths.len() != records.len() {
            return Err(format!("sidecar has an empty or malformed consumer-main record: {}", sidecar.display()));
        }
        paths
            .iter()
            .map(|raw| {
                let declared = expand(home, raw).canonicalize().map_err(|_| format!("sidecar consumer main does not exist: {raw}"))?;
                self.require_consumer_shape(&declared, "sidecar consumer is not a canonical ~/code/.../main checkout")?;
                Ok(declared.to_string_lossy().into_owned())
            })
            .collect()
    }

    fn require_consumer_shape(&self, path: &Path, complaint: &str) -> Result<(), String> {
        let depth = path.strip_prefix(&self.code_root).map_or(0, |rest| rest.components().count());
        if depth >= 2 && path.file_name().is_some_and(|name| name == "main") {
            Ok(())
        } else {
            Err(format!("{complaint}: {}", path.display()))
        }
    }

    fn require_pin_state(&self, oid: &str) -> Result<(), String> {
        if self.registered_head().as_deref() != Some(oid) {
            return Err(format!("pin is not a detached registered worktree at its named object: {}", self.pin.display()));
        }
        if self.pin_head().as_deref() != Some(oid) {
            return Err(format!("pin HEAD differs from its path: {}", self.pin.display()));
        }
        if self.attached() {
            return Err(format!("pin is attached to a branch: {}", self.pin.display()));
        }
        if !git_ok(&self.main, &["cat-file", "-e", &format!("{oid}^{{commit}}")]) {
            return Err(format!("pin object is unavailable from main: {oid}"));
        }
        if !retires(true, u32::from(self.dirty())) {
            return Err(format!("pin has tracked or untracked changes; refusing retirement: {}", self.pin.display()));
        }
        Ok(())
    }

    fn published_consumer(&self, home: &Path, raw: &str) -> Result<Consumer, String> {
        let main = expand(home, raw).canonicalize().map_err(|_| format!("consumer main does not exist: {raw}"))?;
        self.require_consumer_shape(&main, "consumer is not a canonical ~/code/.../main checkout")?;
        if !main.join(".git").exists() {
            return Err(format!("consumer is not a Git checkout: {}", main.display()));
        }
        if git_out(&main, &["symbolic-ref", "--quiet", "HEAD"]).as_deref() != Some("refs/heads/main") {
            return Err(format!("consumer is not its main branch checkout: {}", main.display()));
        }
        if git_out(&main, &["status", "--porcelain"]).is_none_or(|status| !status.is_empty()) {
            return Err(format!("consumer main is dirty, so its moved state is not durable: {}", main.display()));
        }
        let head = git_out(&main, &["rev-parse", "--verify", "HEAD"]).ok_or_else(|| format!("consumer has no HEAD: {}", main.display()))?;
        let published = git_out(&main, &["rev-parse", "--verify", "refs/remotes/origin/main"]).ok_or_else(|| format!("consumer has no published origin/main: {}", main.display()))?;
        if head != published {
            return Err(format!("consumer main does not match origin/main: {}", main.display()));
        }
        let pin = self.pin.to_string_lossy();
        match git::run(&main, &["grep", "-F", "-q", "-e", self.leaf.as_str(), "-e", &pin, &head, "--", "."]) {
            Ok(output) if output.status.code() == Some(0) => Err(format!("consumer still references the pin object or path: {}", main.display())),
            Ok(output) if output.status.code() == Some(1) => Ok(Consumer { main, head }),
            _ => Err(format!("consumer reference check failed: {}", main.display())),
        }
    }

    fn refuse_consumer_worktrees(&self, consumer: &Path, oid: &str) -> Result<(), String> {
        let mut worktrees = git_out(consumer, &["worktree", "list", "--porcelain"])
            .unwrap_or_default()
            .lines()
            .filter_map(|line| line.strip_prefix("worktree ").map(PathBuf::from))
            .collect::<Vec<_>>();
        let lanes = consumer.parent().map(|container| container.join("worktrees")).and_then(|dir| fs::read_dir(dir).ok());
        for entry in lanes.into_iter().flatten().flatten() {
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                worktrees.push(entry.path());
            }
        }
        let needle = regex::bytes::Regex::new(&format!("{}|{}", regex::escape(oid), regex::escape(&self.pin.to_string_lossy()))).map_err(|error| error.to_string())?;
        let scope = Scope { max_file_size: None, skip_build_output: false, limit: None };
        let mut checked = BTreeSet::new();
        for worktree in worktrees.into_iter().filter(|worktree| worktree.is_dir() && checked.insert(worktree.clone())) {
            let hits = mentioning(std::slice::from_ref(&worktree), &needle, &scope).map_err(|_| format!("consumer worktree reference check failed: {}", worktree.display()))?;
            if !hits.is_empty() {
                return Err(format!("consumer worktree still references the pin: {}", worktree.display()));
            }
        }
        Ok(())
    }

    fn recheck(&self, consumers: &[Consumer], sidecar: &Path, digest: &[u8], oid: &str) -> Result<(), String> {
        for consumer in consumers {
            let main = &consumer.main;
            if git_out(main, &["rev-parse", "--verify", "HEAD"]).as_deref() != Some(consumer.head.as_str()) {
                return Err(format!("consumer HEAD changed during retirement proof: {}", main.display()));
            }
            if git_out(main, &["status", "--porcelain"]).is_none_or(|status| !status.is_empty()) {
                return Err(format!("consumer main became dirty during retirement proof: {}", main.display()));
            }
            if git_out(main, &["rev-parse", "--verify", "refs/remotes/origin/main"]).as_deref() != Some(consumer.head.as_str()) {
                return Err(format!("consumer origin/main changed during retirement proof: {}", main.display()));
            }
        }
        if Sha256::digest(fs::read(sidecar).unwrap_or_default()).as_slice() != digest {
            return Err(format!("consumer sidecar changed during retirement proof: {}", sidecar.display()));
        }
        if self.pin_head().as_deref() != Some(oid) {
            return Err(format!("pin HEAD changed during retirement proof: {}", self.pin.display()));
        }
        if self.attached() {
            return Err(format!("pin became attached during retirement proof: {}", self.pin.display()));
        }
        if self.dirty() {
            return Err(format!("pin became dirty during retirement proof: {}", self.pin.display()));
        }
        if self.registered_head().as_deref() != Some(oid) {
            return Err(format!("pin registration changed during retirement proof: {}", self.pin.display()));
        }
        Ok(())
    }
}
