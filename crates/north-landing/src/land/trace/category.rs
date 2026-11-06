use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Landing,
    Queue,
    HostWait,
    Offload,
    Compile,
    Test,
    Lint,
    Docs,
    Scans,
    Check,
    Git,
    Lander,
    Follow,
    Ci,
}

impl Category {
    pub const ORDER: [Category; 13] = [
        Category::Queue,
        Category::HostWait,
        Category::Offload,
        Category::Compile,
        Category::Test,
        Category::Lint,
        Category::Docs,
        Category::Scans,
        Category::Check,
        Category::Git,
        Category::Lander,
        Category::Follow,
        Category::Ci,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Category::Landing => "landing",
            Category::Queue => "queue wait",
            Category::HostWait => "host wait",
            Category::Offload => "sync/offload",
            Category::Compile => "compile",
            Category::Test => "test run",
            Category::Lint => "lint",
            Category::Docs => "docs/index checks",
            Category::Scans => "scans",
            Category::Check => "other checks",
            Category::Git => "git rebase/push",
            Category::Lander => "safe-push and lander",
            Category::Follow => "post-push follow",
            Category::Ci => "CI",
        }
    }

    pub fn waiting(self) -> bool {
        matches!(self, Category::Queue | Category::HostWait)
    }

    pub fn after_push(self) -> bool {
        matches!(self, Category::Follow | Category::Ci)
    }

    pub fn of_command(command: &str) -> Self {
        let has = |word: &str| command.contains(word);
        if has("clippy") || has("code-health") || has("check-owners") || has("fmt --check") {
            Category::Lint
        } else if has("cargo test") || has("nextest") || has(".test.sh") || has("bun test") || has("land-tests") {
            Category::Test
        } else if has("cargo check") || has("cargo build") || command.trim_end().ends_with(" --build") {
            Category::Compile
        } else if has("docs ") || has("-index") || has("citation") {
            Category::Docs
        } else if has("betterleaks") || has("scan") || has("denylist") {
            Category::Scans
        } else {
            Category::Check
        }
    }
}
