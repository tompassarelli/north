//! The variables a run carries across a boundary into a depbox sandbox or onto an offload host, as one list that depbox (through `north capacity carry --list`) and the offload ship both read; a lease's scope inherits the caller's whole environment, so it carries every one of them.

/// A boundary a run crosses, which decides the part of the list it carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Boundary {
    /// A depbox `run` sandbox, which also shares the compiler cache.
    Sandbox,
    /// A depbox `fetch` sandbox, which has no compiler cache.
    Fetch,
    /// An offload slot on another host, which keeps its own paths, toolchain and lease.
    Offload,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Reach {
    Landing,
    Host,
    Cache,
}

pub const LANDING: &str = "SAFE_PUSH_LANDING";
pub const LAND_BASE: &str = "SAFE_PUSH_LAND_BASE";
pub const LAND_HEAD: &str = "SAFE_PUSH_LAND_HEAD";

const CARRY: [(&str, Reach); 29] = [
    (LANDING, Reach::Landing),
    (LAND_BASE, Reach::Landing),
    (LAND_HEAD, Reach::Landing),
    ("SAFE_PUSH_LAND_RUNTIME_HEAD", Reach::Landing),
    ("SAFE_PUSH_LAND_REF", Reach::Landing),
    ("SAFE_PUSH_LAND_PACKAGES", Reach::Landing),
    ("SAFE_PUSH_LAND_CPUS", Reach::Landing),
    ("SAFE_PUSH_LAND_IT", Reach::Landing),
    ("PATH", Reach::Host),
    ("HOME", Reach::Host),
    ("USER", Reach::Host),
    ("LANG", Reach::Host),
    ("TERM", Reach::Host),
    ("CARGO_BUILD_JOBS", Reach::Host),
    ("RUST_TEST_THREADS", Reach::Host),
    ("AGENT_CAPACITY_LEASE", Reach::Host),
    ("NORTH_DEV_SHELL", Reach::Host),
    ("NIX_PATH", Reach::Host),
    ("SSL_CERT_FILE", Reach::Host),
    ("SSL_CERT_DIR", Reach::Host),
    ("NIX_SSL_CERT_FILE", Reach::Host),
    ("CURL_CA_BUNDLE", Reach::Host),
    ("GIT_SSL_CAINFO", Reach::Host),
    ("RUSTC_WRAPPER", Reach::Cache),
    ("SCCACHE_DIR", Reach::Cache),
    ("SCCACHE_CACHE_SIZE", Reach::Cache),
    ("SCCACHE_BASEDIRS", Reach::Cache),
    ("CMAKE_C_COMPILER_LAUNCHER", Reach::Cache),
    ("CMAKE_CXX_COMPILER_LAUNCHER", Reach::Cache),
];

const USAGE: &str = "usage: north capacity carry --list [--into sandbox|fetch|offload]";

impl Boundary {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "sandbox" => Some(Self::Sandbox),
            "fetch" => Some(Self::Fetch),
            "offload" => Some(Self::Offload),
            _ => None,
        }
    }

    fn carries(self, reach: Reach) -> bool {
        match self {
            Self::Sandbox => true,
            Self::Fetch => reach != Reach::Cache,
            Self::Offload => reach == Reach::Landing,
        }
    }
}

/// The names a run carries across `boundary`, in list order.
pub fn names(boundary: Boundary) -> impl Iterator<Item = &'static str> {
    CARRY.into_iter().filter(move |(_, reach)| boundary.carries(*reach)).map(|(name, _)| name)
}

/// `north capacity carry --list [--into BOUNDARY]` prints, one per line, the variables a run carries across BOUNDARY, or every carried variable.
pub fn run(arguments: &[String]) -> u8 {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let listed: Vec<&str> = match words.as_slice() {
        ["--list"] => CARRY.into_iter().map(|(name, _)| name).collect(),
        ["--list", "--into", boundary] => match Boundary::parse(boundary) {
            Some(boundary) => names(boundary).collect(),
            None => return usage(),
        },
        _ => return usage(),
    };
    println!("{}", listed.join("\n"));
    0
}

fn usage() -> u8 {
    eprintln!("{USAGE}");
    2
}

#[cfg(test)]
mod tests {
    use super::{Boundary, names};

    #[test]
    fn an_offload_carries_only_the_landing_context_and_a_fetch_sandbox_carries_everything_but_the_compiler_cache() {
        let offload: Vec<_> = names(Boundary::Offload).collect();
        assert!(offload.iter().all(|name| name.starts_with("SAFE_PUSH_LAND")), "{offload:?}");
        assert!(offload.contains(&"SAFE_PUSH_LAND_IT") && offload.contains(&"SAFE_PUSH_LAND_RUNTIME_HEAD"), "{offload:?}");
        let fetch: Vec<_> = names(Boundary::Fetch).collect();
        let sandbox: Vec<_> = names(Boundary::Sandbox).collect();
        assert!(offload.iter().all(|name| fetch.contains(name)) && fetch.iter().all(|name| sandbox.contains(name)));
        assert!(fetch.contains(&"PATH") && !fetch.contains(&"RUSTC_WRAPPER") && sandbox.contains(&"RUSTC_WRAPPER"));
    }
}
