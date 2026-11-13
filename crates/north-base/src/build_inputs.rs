pub const ENQUEUE_PATTERN: &str = "@enqueue";
const ROOT_FILES: [&str; 8] = ["Cargo.toml", "Cargo.lock", "build.rs", "rust-toolchain.toml", "rust-toolchain", "flake.nix", "flake.lock", "bin/cargo"];
const ROOT_DIRS: [&str; 3] = ["crates/", "src/", ".cargo/"];
const EMBEDDED: &[&str] = &[
    ".safe-push",
    "harness/skills/machine-capacity/scripts/capacity-run-queue-overload.recorded.json",
];
pub const RUN_TIME_DATA: [&str; 19] = [
    "harness/disk-ledger.toml",
    "harness/defaults.toml",
    "harness/model-catalog.toml",
    "harness/loops.toml",
    "harness/routing-cells.toml",
    "harness/nix/cards.toml",
    "harness/code-health/review-brief.md",
    "harness/claude/agents/planner.md",
    "harness/claude/agents/worker.md",
    "harness/claude/agents/worker-haiku.md",
    "harness/claude/agents/worker-high.md",
    "harness/claude/agents/worker-sonnet.md",
    "harness/claude/agents/worker-xhigh.md",
    "harness/file-skills.toml",
    "north-data/private-checks.toml",
    "north-data/expectations.toml",
    "harness/merge-sets.toml",
    "harness/policy-owners.toml",
    "docs/feedback-loop.md",
];

pub mod separate_workspace;

pub fn rust_build_input_paths() -> impl Iterator<Item = &'static str> {
    ROOT_FILES.into_iter().chain(ROOT_DIRS).chain(EMBEDDED.iter().copied()).chain([".gitattributes"])
}

pub fn rust_build_input(path: &str) -> bool {
    ROOT_FILES.contains(&path) || ROOT_DIRS.iter().any(|dir| path.starts_with(dir)) || EMBEDDED.contains(&path) || path == ".gitattributes" || path.ends_with("/.gitattributes")
}

pub fn run_time_data(path: &str) -> bool {
    RUN_TIME_DATA.contains(&path)
}

const NOT_RUN_TIME_DIRS: [&str; 4] = ["docs/", "tests/", "examples/", ".github/"];
const NOT_RUN_TIME_FILES: [&str; 6] = ["LICENSE", "LICENSE-APACHE", "LICENSE-MIT", "NOTICE", "ambient-test-clocks.tsv", "ambient-test-inputs.tsv"];

/// Whether a change to `path` changes what a promoted runtime runs: its binaries, or a file of the exported tree it reads, publishes or links.
pub fn runtime_input(path: &str) -> bool {
    rust_build_input(path) || run_time_data(path) || !(NOT_RUN_TIME_FILES.contains(&path) || NOT_RUN_TIME_DIRS.iter().any(|dir| path.starts_with(dir)))
}

pub const LIVE_FIX: &str = "Live-fix";

pub fn live_fix(message: &str) -> Option<&str> {
    message.lines().find_map(|line| line.strip_prefix(LIVE_FIX)?.strip_prefix(':')).map(str::trim).filter(|reason| !reason.is_empty())
}

/// The start of the promotion slot that holds `now` (seconds since the epoch), slots being `spacing` seconds apart from 00:00 UTC.
pub fn promotion_slot(now: i64, spacing: i64) -> i64 {
    now - now.rem_euclid(spacing)
}

/// The seconds between promotion slots that `harness/defaults.toml` declares.
pub fn promotion_slot_sec() -> i64 {
    crate::platform::harness::default_number("runtime_promotion_slot_sec") as i64
}
