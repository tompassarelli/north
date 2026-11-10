#[derive(Debug, Default, PartialEq, Eq)]
pub struct Verdict {
    pub left_out: Vec<String>,
    pub environment: Vec<String>,
    pub selected: Vec<String>,
}

pub fn classify(failed: &[String], on_base: Option<&[String]>, selected: impl Fn(&str) -> bool) -> Verdict {
    let mut verdict = Verdict::default();
    for name in failed {
        if on_base.is_some_and(|base| base.contains(name)) {
            verdict.environment.push(name.clone());
        } else if selected(name) {
            verdict.selected.push(name.clone());
        } else {
            verdict.left_out.push(name.clone());
        }
    }
    verdict
}
