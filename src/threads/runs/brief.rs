//! The fields a worker brief names, read the way bin/threads reads them.
use std::sync::LazyLock;

use regex::Regex;

pub(crate) const CATEGORIES: [&str; 11] =
    ["mechanical", "docs-policy", "tooling", "feature", "bug-known-cause", "debugging-unknown-cause", "netcode-determinism", "performance", "balance-tuning", "native-check", "research"];

/// Orthogonal brief facets, each a closed vocabulary, in the order the runs table holds them.
pub(crate) const FACETS: [(&str, &[&str]); 4] = [
    ("Spec", &["exact", "measured", "judged"]),
    ("Scope", &["one-file", "module", "cross-module", "cross-repo"]),
    ("Surface", &["ts", "lua", "nix", "shell", "workflow", "docs", "assets"]),
    ("Verify", &["none", "local-test", "farm", "native", "visual"]),
];

fn field(name: &str) -> Regex {
    Regex::new(&format!(r"\b{name}:[ \t]*([^\s`<>]+)")).expect("brief field pattern")
}

static ITEM: LazyLock<Regex> = LazyLock::new(|| field("Item"));
static FOLLOWS: LazyLock<Regex> = LazyLock::new(|| field("Follows"));
static ARM: LazyLock<Regex> = LazyLock::new(|| field("Arm"));
static CATEGORY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bCategory:[ \t]*([A-Za-z-]+)").expect("category pattern"));
static LENS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bLens:[ \t]*([^.;,\n`<>]+)").expect("lens pattern"));
static FACET: LazyLock<Vec<Regex>> = LazyLock::new(|| FACETS.iter().map(|(name, _)| Regex::new(&format!(r"\b{name}:[ \t]*([A-Za-z-]+)")).expect("facet pattern")).collect());

/// What a brief names: None for a field it leaves out, empty for one it names as nothing.
#[derive(Default)]
pub(crate) struct Fields {
    pub item: Option<String>,
    pub follows: Option<String>,
    pub arm: Option<String>,
    pub category: Option<String>,
    pub facets: [Option<String>; 4],
    pub lens: Option<String>,
}

impl Fields {
    pub(crate) fn read(brief: &str) -> Self {
        let first = |pattern: &Regex| pattern.captures(brief).map(|found| found[1].to_owned());
        let named = |pattern: &Regex| first(pattern).map(|text| text.trim_end_matches(['.', ',', ';', ':', ')']).to_owned());
        let facets = std::array::from_fn(|index| first(&FACET[index]).map(|text| text.to_lowercase()).filter(|text| FACETS[index].1.contains(&text.as_str())));
        Self {
            item: named(&ITEM),
            follows: named(&FOLLOWS),
            arm: named(&ARM),
            category: first(&CATEGORY).map(|text| text.trim().to_lowercase()),
            facets,
            lens: first(&LENS).map(|text| text.trim().to_lowercase()),
        }
    }

    /// The category when it is one of CATEGORIES.
    pub(crate) fn known_category(&self) -> Option<&str> {
        self.category.as_deref().filter(|category| CATEGORIES.contains(category))
    }
}
