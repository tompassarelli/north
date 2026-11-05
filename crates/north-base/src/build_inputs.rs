const ROOT_FILES: [&str; 8] = ["Cargo.toml", "Cargo.lock", "build.rs", "rust-toolchain.toml", "rust-toolchain", "flake.nix", "flake.lock", "bin/cargo"];
const ROOT_DIRS: [&str; 3] = ["crates/", "src/", ".cargo/"];
const EMBEDDED: &[&str] = &[
    ".safe-push",
    "harness/skills/machine-capacity/scripts/capacity-run-queue-overload.recorded.json",
];
pub const RUN_TIME_DATA: [&str; 18] = [
    "harness/defaults.toml",
    "harness/model-catalog.toml",
    "harness/loops.toml",
    "harness/model-watch-sources.toml",
    "harness/nix/cards.toml",
    "harness/code-health/review-brief.md",
    "harness/claude/agents/planner.md",
    "harness/claude/agents/worker.md",
    "harness/claude/agents/worker-haiku.md",
    "harness/claude/agents/worker-high.md",
    "harness/claude/agents/worker-sonnet.md",
    "harness/claude/agents/worker-xhigh.md",
    "agent-machinery/file-skills.toml",
    "north-data/private-checks.toml",
    "north-data/expectations.toml",
    "harness/merge-sets.toml",
    "harness/policy-owners.toml",
    "docs/feedback-loop.md",
];

pub fn rust_build_input(path: &str) -> bool {
    ROOT_FILES.contains(&path) || ROOT_DIRS.iter().any(|dir| path.starts_with(dir)) || EMBEDDED.contains(&path) || path == ".gitattributes" || path.ends_with("/.gitattributes")
}

pub fn run_time_data(path: &str) -> bool {
    RUN_TIME_DATA.contains(&path)
}
