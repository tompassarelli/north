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
