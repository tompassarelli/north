use super::{OVERRIDE, State, base, git, git_root, under};
use regex::Regex;
use std::path::Path;
use std::process::{Command, Stdio};

impl State<'_> {
    pub(super) fn lane(&self, p: &str) -> Option<String> {
        let (pre, rest) = p.split_once("/worktrees/")?;
        pre.starts_with(&format!("{}/code", self.home))
            .then(|| format!("{pre}/worktrees/{}", rest.split('/').next().unwrap_or("")))
    }
    pub(super) fn strong(&self, wt: &str) -> bool {
        if under(&self.cwd, wt) || self.created.iter().any(|p| p == wt) {
            return true;
        }
        let slug = base(wt);
        if slug.is_empty()
            || !slug
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
        {
            return false;
        }
        let re = Regex::new(&format!(
            r"worktrees/{}([^A-Za-z0-9._-]|$)",
            regex::escape(slug)
        ))
        .unwrap();
        ["agent_transcript_path", "transcript_path"]
            .iter()
            .filter_map(|k| self.input.data[k].as_str())
            .any(|p| {
                std::fs::read_to_string(p).ok().is_some_and(|t| {
                    t.lines()
                        .any(|l| l.contains("worktree add") && re.is_match(l))
                })
            })
    }
    pub(super) fn never(&self, p: &str) -> bool {
        [
            "/", "/bin", "/boot", "/dev", "/etc", "/home", "/lib", "/lib64", "/nix", "/opt",
            "/proc", "/root", "/run", "/sbin", "/srv", "/sys", "/tmp", "/usr", "/var",
        ]
        .contains(&p)
            || [
                "",
                "/code",
                "/Desktop",
                "/Documents",
                "/Downloads",
                "/Music",
                "/Pictures",
                "/Videos",
                "/.config",
                "/.gnupg",
                "/.local",
                "/.local/share",
                "/.local/state",
                "/.ssh",
            ]
            .iter()
            .any(|s| p == format!("{}{s}", self.home))
    }
    pub(super) fn scratch(&self, p: &str) -> bool {
        under(p, &self.cache)
            || under(p, "/tmp")
            || under(p, "/var/tmp")
            || p.starts_with("/run/user/")
            || (!self.tmp.is_empty() && under(p, self.tmp.trim_end_matches('/')))
    }
    pub(super) fn disposable(&self, p: &str) -> bool {
        under(p, &self.cache)
            || [
                "node_modules",
                "__pycache__",
                ".pytest_cache",
                ".mypy_cache",
                ".ruff_cache",
                ".direnv",
                ".gradle",
            ]
            .contains(&base(p))
            || (!p.starts_with(&format!("{}/", self.home)) && self.scratch(p))
    }
    pub(super) fn scratch_shape(&self, p: &str) -> bool {
        ["lan", "online"].iter().any(|s| {
            p.strip_prefix(&format!("{}/wisp/{s}/", self.data))
                .is_some_and(|s| !s.is_empty())
        })
    }
    pub(super) fn agent_scratch(&self, p: &str) -> bool {
        if !self.scratch_shape(p) {
            return false;
        }
        let output = Command::new("realpath")
            .args(["-m", "--", p])
            .stderr(Stdio::null())
            .output()
            .ok();
        output.is_some_and(|o| {
            o.status.success()
                && self.scratch_shape(String::from_utf8_lossy(&o.stdout).trim_end_matches('\n'))
        })
    }
    pub(super) fn owner_reason(&self, p: &str) -> Option<String> {
        let code = format!("{}/code/", self.home);
        self.store_reason(p, &code)
            .or_else(|| {
                p.starts_with(&code)
                    .then(|| Self::container_reason(p, &code))
                    .flatten()
            })
            .or_else(|| self.lane_reason(p))
    }
    pub(super) fn store_reason(&self, p: &str, code: &str) -> Option<String> {
        if p.strip_prefix(code)
            .is_some_and(|s| s.split('/').next().unwrap_or("").ends_with("-data"))
        {
            return Some(format!(
                "'{p}' is machine memory — a ~/code/*-data runtime store that live lanes read, and nothing regenerates it. Prune it with the tool that owns it, never with a recursive delete"
            ));
        }
        if under(p, &format!("{}/.local/state/north", self.home)) {
            return Some(format!(
                "'{p}' is North's own state (coordination graph, session ledger) and other sessions are reading it right now"
            ));
        }
        if under(p, &format!("{code}resources")) {
            return Some(format!(
                "'{p}' is ~/code/resources — read-only context; agents never edit or delete there"
            ));
        }
        None
    }
    pub(super) fn container_reason(p: &str, code: &str) -> Option<String> {
        if p.ends_with("/pins") {
            return Some(format!(
                "'{p}' is a container's pins/ root — every content-addressed checkout in that project at once, plus the .pin manifests that record who consumes them. Sweepers and raw recursive deletion never operate here; retire one verified orphan with pin-retire"
            ));
        }
        if let Some((pre, rest)) = p.split_once("/pins/") {
            let wt = format!("{pre}/pins/{}", rest.split('/').next().unwrap_or(""));
            return Some(format!(
                "'{p}' is in a content-addressed pin whose consumer state must be verified. Its contents, HEAD, and path are immutable while any consumer remains. Advance consumers with a new hash-named pin; after every real consumer moves, retire this pin and sidecar with: pin-retire --consumer-main CONSUMER/main -- '{wt}'. Raw deletion stays denied"
            ));
        }
        if p.ends_with("/worktrees") {
            return Some(format!(
                "'{p}' is a container's worktrees/ root — every concurrent lane in that project at once, including lanes this session cannot see. Name the ONE lane you mean: {p}/SLUG"
            ));
        }
        if p.ends_with("/main") || p.contains("/main/") {
            return Some(format!(
                "'{p}' is inside a 'main' checkout — production, and any dirty state in it is human work-in-progress. Work in a lane: git -C CONTAINER/main worktree add CONTAINER/worktrees/SLUG -b SLUG"
            ));
        }
        if !p[code.len()..].contains('/') {
            return Some(format!(
                "'{p}' is a project container — main/, every lane under worktrees/, and every pin under pins/ go together. Name one directory inside it instead"
            ));
        }
        None
    }
    pub(super) fn lane_reason(&self, p: &str) -> Option<String> {
        if let Some(wt) = self.lane(p)
            && !self.strong(&wt) && !self.cd_dirs.iter().any(|c| under(c, &wt)) {
                let why = if p == wt {
                    format!("'{p}' is another session's worktree")
                } else {
                    format!("'{p}' is inside {wt}, another session's worktree")
                };
                let pre = wt.split_once("/worktrees/").unwrap().0;
                return Some(format!(
                    "{why} — it may hold in-flight work this session cannot see (several lanes run concurrently here). If it is your lane, cd into it in the same command (cd {wt} && rm -r RELATIVE/PATH); if it has landed: git -C {pre}/main worktree remove {wt}"
                ));
            }
        None
    }
    pub(super) fn classify(&self, p: &str) -> Option<(bool, String)> {
        self.sacred(p).or_else(|| self.unrecoverable(p))
    }
    pub(super) fn sacred(&self, p: &str) -> Option<(bool, String)> {
        if self.never(p) {
            return Some((
                true,
                format!(
                    "recursive delete of '{p}' — never, not even by accident. Name the specific subdirectory you mean"
                ),
            ));
        }
        if let Some(why) = self.owner_reason(p) {
            return Some((true, why));
        }
        if p.ends_with("/.git") || p.contains("/.git/") {
            return Some((
                true,
                format!(
                    "'{p}' takes a repository's .git with it — every unpushed commit, every branch, and the reflog. Delete working files instead"
                ),
            ));
        }
        if Path::new(p).join(".git").exists() {
            return Some((
                true,
                format!(
                    "'{p}' is a git checkout root — a recursive delete takes .git with it (unpushed commits, reflog). Use: git -C {p} worktree remove '{p}', or name a subdirectory"
                ),
            ));
        }
        None
    }
    pub(super) fn unrecoverable(&self, p: &str) -> Option<(bool, String)> {
        let path = Path::new(p);
        if path.is_symlink() || !path.exists() || self.disposable(p) || self.agent_scratch(p) {
            return None;
        }
        if let Some(repo) = git_root(p) {
            if git(&repo, &["check-ignore", "-q", "--", p]).is_some() {
                return None;
            }
            let dirty = git(&repo, &["status", "--porcelain", "--", p]).unwrap_or_default();
            if dirty.is_empty() {
                return None;
            }
            if dirty.lines().all(|l| l.starts_with("?? "))
                && self.lane(p).is_some_and(|wt| self.strong(&wt))
            {
                return None;
            }
            let dirty = dirty.lines().take(3).collect::<Vec<_>>().join("; ");
            return Some((
                false,
                format!(
                    "'{p}' holds work git cannot restore ({dirty}) — commit it, or gitignore it if it is build output; {OVERRIDE}"
                ),
            ));
        }
        if p.starts_with(&format!("{}/", self.home)) {
            return Some((
                false,
                format!(
                    "'{p}' is personal data — no version control above it, no cache root, so nothing restores it; {OVERRIDE}"
                ),
            ));
        }
        Some((
            false,
            format!(
                "'{p}' cannot be classified as recoverable (no repository, no cache root, outside $HOME) — blocked by default. Narrow the target to the regenerable directory you mean, or {OVERRIDE}"
            ),
        ))
    }
}
