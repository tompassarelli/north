use std::io::Write;
use std::path::Path;

use super::measure::Times;

pub(super) const HEADER: &str = "recorded_at\thost\trepo\tworkspace\tsha\trustc\tsamples\tcold_build_s\tleaf_build_s\tleaf_test_no_run_s\thub_build_s\thub_test_no_run_s\tthreshold_s\tverdict\tcritical_path";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Verdict {
    Within,
    Near,
    Over,
    Unset,
}

impl Verdict {
    pub(super) fn of(leaf_test: f64, threshold: Option<f64>) -> Self {
        match threshold {
            None => Self::Unset,
            Some(limit) if leaf_test > limit => Self::Over,
            Some(limit) if leaf_test >= limit * 0.75 => Self::Near,
            Some(_) => Self::Within,
        }
    }

    pub(super) fn word(self) -> &'static str {
        match self {
            Self::Within => "within",
            Self::Near => "near",
            Self::Over => "over",
            Self::Unset => "unset",
        }
    }
}

pub(super) struct Row<'a> {
    pub(super) recorded_at: &'a str,
    pub(super) host: &'a str,
    pub(super) repo: &'a str,
    pub(super) workspace: &'a str,
    pub(super) sha: &'a str,
    pub(super) rustc: &'a str,
    pub(super) samples: usize,
    pub(super) times: &'a Times,
    pub(super) threshold: Option<f64>,
    pub(super) verdict: Verdict,
}

impl Row<'_> {
    pub(super) fn line(&self) -> String {
        let t = self.times;
        let threshold = self.threshold.map_or_else(|| "-".to_owned(), |limit| format!("{limit:.0}"));
        let fields = [self.recorded_at.to_owned(), self.host.to_owned(), self.repo.to_owned(), self.workspace.to_owned(), self.sha.to_owned(), self.rustc.to_owned(), self.samples.to_string(), secs(t.cold_build), secs(t.leaf_build), secs(t.leaf_test), secs(t.hub_build), secs(t.hub_test), threshold, self.verdict.word().to_owned(), t.critical_path.clone()];
        fields.map(|field| field.replace(['\t', '\n'], " ")).join("\t")
    }
}

fn secs(value: f64) -> String {
    format!("{value:.1}")
}

pub(super) fn append(path: &Path, row: &Row<'_>) -> Result<(), String> {
    let fail = |error: std::io::Error| format!("{}: {error}", path.display());
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(fail)?;
    }
    let fresh = std::fs::metadata(path).ok().is_none_or(|meta| meta.len() == 0);
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path).map_err(fail)?;
    if fresh {
        writeln!(file, "{HEADER}").map_err(fail)?;
    }
    writeln!(file, "{}", row.line()).map_err(fail)
}

pub(super) fn suggested_threshold(leaf_test: f64) -> f64 {
    (leaf_test * 2.0).ceil().max(30.0)
}
