use super::git::{git_read_form, wip_destroying};
use super::patch::{excise_sanctioned, find_envelope};
use super::shell::{record_assignments, redirect_targets, resolve, tokens, union};
use super::writes::cargo_target_paths;
use super::{INTERPRETERS, MUTATING_GIT, Policy};
use crate::hook::HookInput;
use crate::shell::{basename, shell_substitutions, strip_heredoc_bodies};
use regex::Regex;
use std::collections::BTreeMap;
use std::sync::LazyLock;

impl Policy {
    pub(super) fn decide(&self, input: &HookInput) -> Option<String> {
        let tool = input.data["tool_name"].as_str().unwrap_or("");
        let ti = &input.data["tool_input"];
        let current = std::env::current_dir().ok()?.to_string_lossy().into_owned();
        let cwd = input.data["cwd"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(&current);
        if tool.ends_with("apply_patch") {
            let fields: BTreeMap<String, Box<serde_json::value::RawValue>> =
                serde_json::from_slice(&input.raw).ok()?;
            let envelope = fields.get("tool_input").and_then(|raw| find_envelope(raw));
            return self.patch_tool(ti, cwd, envelope.as_deref());
        }
        if !input.is_shell() {
            let path = ti["file_path"].as_str()?;
            let hit = self.protected(path)?;
            return Some(format!(
                "{path} is inside {}. {}\n\n{}",
                hit.noun(),
                hit.why,
                self.advice(&hit)
            ));
        }
        let command = &ti["command"];
        if command.as_array().is_some()
            && let Some(reason) = self.patch_command(command, cwd, None)
        {
            return Some(reason);
        }
        let command_text = input.command()?.as_str().filter(|s| !s.is_empty())?;
        static CD: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r#"(?:^|[;&|]|&&)[\s\x1c-\x1f]*cd[\s\x1c-\x1f]+("[^"]+"|'[^']+'|[^\s\x1c-\x1f;&|]+)"#).unwrap()
        });
        let effective = CD
            .captures(command_text)
            .and_then(|m| resolve(&self.expanduser(m[1].trim_matches(['"', '\''])), cwd))
            .unwrap_or_else(|| cwd.into());
        let scan = excise_sanctioned(&strip_heredoc_bodies(command_text));
        if let Some(reason) = self.patch_command(command, &effective, Some(&scan)) {
            return Some(reason);
        }
        let words = tokens(&scan);
        let calls = self.git_invocations(&scan, cwd, &self.base_env());
        let segments = self.segments(&scan, &self.base_env(), cwd);
        let mut final_env = segments
            .last()
            .map_or_else(|| self.base_env(), |s| s.env.clone());
        if let Some(last) = segments.last() {
            record_assignments(&tokens(&last.text), &mut final_env, self);
        }
        let mut visited = Vec::new();
        for segment in &segments {
            visited = union(&visited, &segment.cwds);
        }
        if visited.is_empty() {
            visited.push(cwd.into());
        }
        let mut nested = Vec::new();
        for body in shell_substitutions(&scan) {
            for here in &visited {
                nested.extend(self.segments(&body, &final_env, here));
            }
        }
        for raw in cargo_target_paths(&scan) {
            let path = resolve(&self.expanduser(&raw), &effective)?;
            if let Some(hit) = self.protected(&path) {
                return Some(self.deny(&path, &hit, "write Cargo target output"));
            }
            if let Some(project) = self.spill(&path) {
                let container = format!("{}/{project}", self.root);
                return Some(format!(
                    "This Bash command would put Cargo target output directly in the repository container {container} ({path}). Build output belongs to the exact lane that produced it. Use the lane-local default, set `CARGO_TARGET_DIR={container}/worktrees/SLUG/target`, or use a deliberate /tmp target outside repository containers."
                ));
            }
        }
        for call in &calls {
            if let Some(what) = wip_destroying(&call.verb, &call.args)
                && let Some(hit) = self.protected(&call.target)
            {
                let target = &call.target;
                if hit.pin.is_some() {
                    return Some(format!(
                        "`{what}` targets {target}, {}. {}\n\n{}",
                        hit.noun(),
                        hit.why,
                        self.advice(&hit)
                    ));
                }
                return Some(format!(
                    "`{what}` targets {target}, the MAIN checkout of {}. Uncommitted state in a main checkout is the human's work-in-progress; an agent never discards it. {}\n\nCompliant moves:\n  git -C {target} status --porcelain   # inspect, do not discard\n  wt-rescue {target}\n  # dirty main? run `wt-rescue` (relocates intact, restores clean)\n  # rare surgery only: `north config agents off launch-critical-worktree-guard` — deliberate\n  # bypass, state why, re-enable after\n{}",
                    hit.project,
                    hit.why,
                    self.worktree_advice(&hit.project)
                ));
            }
        }
        for call in calls {
            let decision = if call.verb == "worktree" {
                self.worktree_decision(&call)
            } else if call.verb == "fetch" || git_read_form(&call.verb, &call.args) {
                None
            } else if ["merge", "pull"].contains(&call.verb.as_str()) {
                (!call.args.iter().any(|s| s == "--ff-only")).then(|| {
                    (
                        call.target.clone(),
                        format!("{} (without --ff-only)", call.verb),
                    )
                })
            } else {
                MUTATING_GIT
                    .contains(&call.verb.as_str())
                    .then(|| (call.target.clone(), call.verb.clone()))
            };
            if let Some((path, verb)) = decision
                && let Some(hit) = self.protected(&path)
            {
                return Some(self.deny(&path, &hit, &format!("run `git {verb}`")));
            }
        }
        for segment in &segments {
            for raw in redirect_targets(&segment.text) {
                for here in &segment.cwds {
                    if let Some((path, _)) = self.target(&raw, &segment.env, here)
                        && let Some(hit) = self.protected(&path)
                    {
                        return Some(self.deny(&path, &hit, "write"));
                    }
                }
            }
        }
        for segment in segments.iter().chain(&nested) {
            for here in &segment.cwds {
                if let Some(reason) =
                    self.scan_write_commands(&tokens(&segment.text), here, &segment.env)
                {
                    return Some(reason);
                }
            }
        }
        if let Some(hit) = self.protected(&effective)
            && scan.contains("<<")
            && words.iter().any(|s| INTERPRETERS.contains(&basename(s)))
        {
            return Some(self.deny(
                &effective,
                &hit,
                "run an interpreter script with its working directory",
            ));
        }
        None
    }
}
