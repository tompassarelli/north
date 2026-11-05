//! The fields a worker brief names, read the way bin/threads reads them.
use std::sync::LazyLock;

use regex::Regex;

use crate::router::brief::CATEGORIES;

pub use crate::router::brief::FACETS;

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
pub struct Fields {
    pub item: Option<String>,
    pub follows: Option<String>,
    pub arm: Option<String>,
    pub category: Option<String>,
    pub facets: [Option<String>; 4],
    pub lens: Option<String>,
    pub estimates: Estimates,
}

impl Fields {
    pub fn read(brief: &str) -> Self {
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
            estimates: Estimates::read(brief),
        }
    }

    /// The category when it is one of CATEGORIES.
    pub fn known_category(&self) -> Option<&str> {
        self.category.as_deref().filter(|category| CATEGORIES.contains(category))
    }
}

/// Where a launch estimate came from: north tune's cell, the ledger's closed boxes, else the brief's own line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Source {
    Brief,
    Tune,
    Ledger,
}

impl Source {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Brief => "brief",
            Self::Tune => "tune",
            Self::Ledger => "ledger",
        }
    }

    fn of(line: &str, measured: &[&str]) -> Self {
        if line.contains("tune cell") {
            Self::Tune
        } else if measured.iter().any(|marker| line.contains(marker)) {
            Self::Ledger
        } else {
            Self::Brief
        }
    }
}

/// The estimates a brief launched its run with: the ETA line's source, and the Budget line's expected spend E (the
/// API-equivalent spend at which the worker lands what passes and splits the rest, else its p90) with its source.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Estimates {
    pub eta: Option<Source>,
    pub budget: Option<(f64, Source)>,
}

static AT_THE_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bat \$([0-9]+(?:\.[0-9]+)?) API-equivalent spend").expect("budget line pattern"));
static P90: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bp90 \$([0-9]+(?:\.[0-9]+)?)").expect("budget p90 pattern"));

impl Estimates {
    pub fn read(brief: &str) -> Self {
        let line = |name: &str| brief.lines().find_map(|line| line.trim_start().strip_prefix(name));
        let eta = line("ETA:").filter(|rest| rest.trim_start().starts_with(|c: char| c.is_ascii_digit())).map(|rest| Source::of(rest, &["p75 of n="]));
        let budget = line("Budget:").and_then(|rest| {
            let stated = AT_THE_LINE.captures(rest).or_else(|| P90.captures(rest))?;
            Some((stated[1].parse().ok()?, Source::of(rest, &["closed boxes"])))
        });
        Self { eta, budget }
    }
}
