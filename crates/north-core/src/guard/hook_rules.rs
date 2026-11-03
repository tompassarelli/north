#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Mechanical,
    NeedsInput,
    TrueBlock,
    Outside,
    Advice,
}

impl Class {
    pub fn label(self) -> &'static str {
        match self {
            Self::Mechanical => "mechanical",
            Self::NeedsInput => "needs-input",
            Self::TrueBlock => "true-block",
            Self::Outside => "outside",
            Self::Advice => "advice",
        }
    }
}

pub struct Rule {
    pub id: &'static str,
    pub class: Class,
    markers: &'static [&'static str],
    pub skill: &'static str,
}

const fn rule(id: &'static str, class: Class, markers: &'static [&'static str], skill: &'static str) -> Rule {
    Rule { id, class, markers, skill }
}

use Class::{Advice, Mechanical, NeedsInput, Outside, TrueBlock};

const WAIT: &str = "workers: Run anything that may take over about 45 seconds in the background, because a foreground wait delays every message to you.";
const LANE: &str = "repo-safety: edit, build and commit only in a lane worktree (git worktree add ~/code/<project>/worktrees/NAME); main is never touched.";
const BOUNDED: &str = "repo-safety: start a detached or background child only as `run-bounded <duration> -- <command>`.";
const FACETS: &str = "workers: a brief declares Category, Spec, Scope, Surface and Verify with the ledger's exact values.";
const CAPACITY: &str = "machine-capacity: run builds, full suites and renders through `machine-capacity run`.";

pub const RULES: &[Rule] = &[
    rule("claude-sleep", Outside, &["<tool_use_error>Blocked: sleep"], "workers: wait for a condition with Monitor's until-loop, or for a command with run_in_background; never chain sleeps."),
    rule("hook-crash", Outside, &["Failed with non-blocking status code"], ""),
    rule("modern-search", Mechanical, &["modern-search-guard:", "; use the modern tool instead:"], "repo-safety: search with rg and fd, never grep -r or find."),
    rule("resource-safe-search", TrueBlock, &["would perform recursive search", "would perform content search", "traversing main and every worktree"], "repo-safety: scope a search to one checkout or one named directory."),
    rule("corpus-scan", TrueBlock, &["is an unscoped sweep of the agent-transcri"], "todo: recall past work through convo, never a sweep of the transcript store."),
    rule("worker-wait", NeedsInput, &["Workers never block in the foreground"], WAIT),
    rule("poll-loop", NeedsInput, &["in a sleep or poll loop"], WAIT),
    rule("blind-stage", NeedsInput, &["blind staging refused", "north-hook: rewrote `git commit -a`"], "repo-safety: stage enumerated paths; never git add -A, ., -u or commit -a."),
    rule("session-kill", Mechanical, &["would detach an agent child from its owner", "session-kill-guard:"], BOUNDED),
    rule("launch-critical", TrueBlock, &["PRIMARY checkout", "the MAIN checkout of", "`main` is the clean checkout"], LANE),
    rule("tripwire.push", Mechanical, &["raw 'git push'", "tripwire: rewrote a raw `git push`"], "repo-safety: publish only with safe-push, never a raw git push."),
    rule("tripwire.delete", TrueBlock, &["tripwire: whole-tree recursive delete", "recursive-delete target"], "repo-safety: never recursively delete a home, system, checkout or state tree."),
    rule("tripwire", TrueBlock, &["tripwire:"], "repo-safety: history, secrets and shared state stay behind safe-push and the lane tools."),
    rule("git-stash", TrueBlock, &["The stash is shared by every worktree"], "repo-safety: park work as a WIP commit in the lane, never git stash."),
    rule("heavy-command.farm", TrueBlock, &["belongs on the farm"], "verification: full suites run on the farm."),
    rule("heavy-command", Mechanical, &["requires capacity admission", "Heavy command routed through machine-capacity", "is heavy and would run in your terminal scope"], CAPACITY),
    rule("native-item", NeedsInput, &["native NorthThread item", "native threads item"], "todo: native items live in threads, never gh."),
    rule("issue-filing", NeedsInput, &["A worker files an issue only", "is a split or follow-up of", "Could not link"], "workers: a worker files an issue only for what it cannot finish in its lane."),
    rule("issue-labels", NeedsInput, &["`gh issue create` in"], "github: gh issue create names its type, priority and evidence labels."),
    rule("worker-tier.missing-facet", NeedsInput, &["declare Category and Spec/Scope/Surface/Verify"], FACETS),
    rule("worker-tier.facet-value", Mechanical, &["is not a facet value"], FACETS),
    rule("worker-tier.payback", TrueBlock, &["needs a Payback: line"], "planning: a tooling brief carries a priced Payback line."),
    rule("worker-tier.lands-before", NeedsInput, &["needs a `Lands-before:"], FACETS),
    rule("worker-tier.background", Mechanical, &["require run_in_background: true", "worker-tier-guard: repaired"], "workers: spawn every Claude worker with run_in_background: true."),
    rule("spawn-capacity.confirm", Advice, &["spawn recording boundary miss"], ""),
    rule("spawn-capacity", TrueBlock, &["Hold: ", "CPU pressure", "warning only, nothing blocked"], "workers: staff within the pace line `north hook spawn-gate` prints."),
    rule("desktop-action", TrueBlock, &["Tom is using the desktop", "desktop action guard:"], "private-desktop-development: drive GUI work on the private desktop."),
    rule("clipboard", TrueBlock, &["Never read, write or clear Tom's clipboard"], "private-desktop-development: capture only with agent-capture."),
    rule("sops-decrypt", TrueBlock, &["`sops` decrypt output"], "Keep secrets out of output: pull one value with sops --extract."),
    rule("public-text", NeedsInput, &["public deny-list rule"], "github: public issue text names no private project facts."),
    rule("frozen-topics", TrueBlock, &["is frozen"], ""),
    rule("reviewer-cli", TrueBlock, &["standing order is Claude only"], ""),
    rule("native-evidence", NeedsInput, &["Native evidence runs go through", "The quiet native lease is for timing"], "warcraft-modding: native evidence runs go through bun wisp accept."),
    rule("native-launch", NeedsInput, &["A Wine/Proton/Warcraft client started here"], "warcraft-modding: start clients through the capacity wrapper."),
    rule("generated-policy", TrueBlock, &["Never hand-edit generated agent policy", "Never hand-edit a .nix generated"], "agent-policy: edit policy sources, never generated output."),
    rule("model-identity", TrueBlock, &["a todo model field must name an exact"], ""),
    rule("lead-timer", TrueBlock, &["lead-timer-guard:"], "workers: a lead wakes on messages, never timers."),
    rule("handoff-record", Advice, &["handoff-record:"], ""),
    rule("file-skill", Advice, &["You are editing the Firn"], ""),
    rule("code-health", Advice, &["code-health:"], ""),
    rule("worker-handoff", Advice, &["Land anything that passes now", "handoff point"], ""),
];

pub const OTHER: &str = "other";

pub fn classify(text: &str) -> &'static str {
    RULES.iter().find(|rule| rule.markers.iter().any(|marker| text.contains(marker))).map_or(OTHER, |rule| rule.id)
}

pub fn find(id: &str) -> Option<&'static Rule> {
    RULES.iter().find(|rule| rule.id == id)
}
