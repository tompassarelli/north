use std::io::Write;
use std::path::Path;

use super::measure::Times;

pub(super) const HEADER: &str = "recorded_at\thost\trepo\tworkspace\tsha\trustc\tsamples\tcold_build_s\tleaf_build_s\tleaf_test_no_run_s\thub_build_s\thub_test_no_run_s\tthreshold_s\tverdict\tcritical_path\tcontention";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Verdict {
    Within,
    Near,
    Over,
    Contended,
}

impl Verdict {
    pub(super) fn of(leaf_test: f64, limit: f64) -> Self {
        if leaf_test > limit {
            Self::Over
        } else if leaf_test >= limit * 0.75 {
            Self::Near
        } else {
            Self::Within
        }
    }

    pub(super) fn judge(leaf_test: f64, limit: f64, contention: Option<f64>, ceiling: Option<f64>) -> Self {
        if contention.zip(ceiling).is_some_and(|(load, ceiling)| load > ceiling) {
            Self::Contended
        } else {
            Self::of(leaf_test, limit)
        }
    }

    pub(super) fn word(self) -> &'static str {
        match self {
            Self::Within => "within",
            Self::Near => "near",
            Self::Over => "over",
            Self::Contended => "contended",
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
    pub(super) threshold: f64,
    pub(super) verdict: Verdict,
    pub(super) contention: Option<f64>,
}

impl Row<'_> {
    pub(super) fn line(&self) -> String {
        let t = self.times;
        let threshold = self.threshold.to_string();
        let fields = [self.recorded_at.to_owned(), self.host.to_owned(), self.repo.to_owned(), self.workspace.to_owned(), self.sha.to_owned(), self.rustc.to_owned(), self.samples.to_string(), secs(t.cold_build), secs(t.leaf_build), secs(t.leaf_test), secs(t.hub_build), secs(t.hub_test), threshold, self.verdict.word().to_owned(), t.critical_path.clone(), self.contention.map_or_else(|| "-".to_owned(), |load| format!("{load:.2}"))];
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
