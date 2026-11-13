macro_rules! re {
    ($pattern:literal) => {{
        static RE: std::sync::LazyLock<regex::Regex> =
            std::sync::LazyLock::new(|| regex::Regex::new($pattern).unwrap());
        &*RE
    }};
}

mod bar;
mod check;
mod cli;
mod debt;
mod git;
mod new_bars;
mod new_rules;
mod record;

pub use bar::{Bar, bars};
pub use cli::run;
