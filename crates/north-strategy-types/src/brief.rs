/// The brief facets, each a closed vocabulary, in the order the runs table holds them; the guard, the router and the ledger read these.
pub const FACETS: [(&str, &[&str]); 4] = [
    ("Spec", &["exact", "measured", "judged"]),
    ("Scope", &["one-file", "module", "cross-module", "cross-repo"]),
    ("Surface", &["ts", "rust", "lua", "nix", "shell", "workflow", "docs", "assets"]),
    ("Verify", &["none", "local-test", "farm", "native", "visual", "live-run"]),
];

pub const CATEGORIES: [&str; 13] =
    ["mechanical", "docs-policy", "tooling", "feature", "bug-known-cause", "debugging-unknown-cause", "netcode-determinism", "performance", "balance-tuning", "native-check", "research", "planning", "review"];
