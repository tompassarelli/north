const ROOT_FILES: [&str; 8] = ["Cargo.toml", "Cargo.lock", "build.rs", "rust-toolchain.toml", "rust-toolchain", "flake.nix", "flake.lock", "bin/cargo"];
const ROOT_DIRS: [&str; 4] = ["crates/", "src/", ".cargo/", "harness/claude/agents/"];
const EMBEDDED: [&str; 14] = [
    "harness/code-health/review-brief.md",
    "harness/defaults.toml",
    "harness/model-catalog.toml",
    "harness/model-watch-sources.toml",
    "agent-machinery/file-skills.toml",
    "north-data/private-checks.toml",
    "north-data/expectations.toml",
    "bin/threads",
    "docs/feedback-loop.md",
    ".safe-push",
    "harness/merge-sets.toml",
    "harness/policy-owners.toml",
    "tests/fixtures/capacity-unseen-pressure.recorded.json",
    "harness/skills/machine-capacity/scripts/capacity-run-queue-overload.recorded.json",
];

pub fn rust_build_input(path: &str) -> bool {
    ROOT_FILES.contains(&path) || ROOT_DIRS.iter().any(|dir| path.starts_with(dir)) || EMBEDDED.contains(&path) || path == ".gitattributes" || path.ends_with("/.gitattributes")
}
