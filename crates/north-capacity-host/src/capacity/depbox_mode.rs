//! The two depbox sandboxes: `fetch` shares the network for installs and fetches, `run` has none. The gate picks one for a toolchain call and depbox enters it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Fetch,
    Run,
}

impl Mode {
    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "fetch" => Some(Self::Fetch),
            "run" => Some(Self::Run),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Fetch => "fetch",
            Self::Run => "run",
        }
    }
}

pub fn in_sandbox(marker: &str, pid_depth: usize) -> bool {
    !marker.is_empty() && pid_depth > 1
}

pub fn pid_namespace_depth() -> usize {
    std::fs::read_to_string("/proc/self/status").ok().and_then(|status| status.lines().find_map(|line| line.strip_prefix("NSpid:").map(|pids| pids.split_whitespace().count()))).unwrap_or(0)
}
