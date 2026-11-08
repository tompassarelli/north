use north_coverage::map::{Map, Test, path_for};
use north_coverage::run_selected::{nextest_arguments, repository};
use std::collections::BTreeSet;
use std::path::Path;

const MOST_NAMED: usize = 3;

pub struct Reach(Option<Map>);

impl Reach {
    pub fn load(root: &Path) -> Self {
        Self(Map::load(&path_for(&repository(root))).ok())
    }

    pub fn line(&self, path: &str) -> String {
        self.0.as_ref().map_or_else(|| fallback(path, "no coverage map on this host"), |map| from_map(map, path))
    }
}

fn from_map(map: &Map, path: &str) -> String {
    let reaching = map.reaching(path);
    if reaching.is_empty() {
        return fallback(path, "no mapped test reaches it");
    }
    let command = if reaching.len() <= MOST_NAMED { named(&reaching) } else { by_binary(&reaching) };
    format!("{command} ({} tests reach it)", reaching.len())
}

fn named(tests: &BTreeSet<Test>) -> String {
    format!("cargo nextest run {}", nextest_arguments(tests, &[]).join(" "))
}

fn by_binary(tests: &BTreeSet<Test>) -> String {
    let binaries: BTreeSet<&str> = tests.iter().map(|test| test.binary.as_str()).collect();
    let packages: BTreeSet<&str> = binaries.iter().map(|binary| binary.split("::").next().unwrap_or(binary)).collect();
    let filter: Vec<String> = binaries.iter().map(|binary| format!("binary_id(={binary})")).collect();
    let packages: Vec<String> = packages.iter().map(|package| format!("-p {package}")).collect();
    format!("cargo nextest run {} -E '{}'", packages.join(" "), filter.join(" | "))
}

fn fallback(path: &str, why: &str) -> String {
    let package = path.strip_prefix("crates/").and_then(|rest| rest.split('/').next());
    let scope = package.map_or_else(String::new, |package| format!(" -p {package}"));
    format!("cargo nextest run{scope} ({why})")
}
