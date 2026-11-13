use super::git::git;
use super::new_bars::new_bars;
use super::new_rules::new_rules;
use std::path::Path;

const FORMS: &str = "Provenance: <claim with the bar>; origin <tom|experiment|prior-art|incident|none>; pointer <quote with date | run file plus rerun command | link | issue>; rationale <repo path#anchor | not recorded>; revisit <YYYY-MM-DD | closed REPO#N | event text>, ending ; soft for a bar that does not gate (origin none only then)";
pub(super) fn check(repo: &Path, base: &str, head: &str) -> Result<u8, String> {
    let body = git(repo, &["log", "-1", "--format=%B", head])?;
    for m in re!(
        r"(?m)^This reverts commit ([0-9a-f]{7,40})\.?(?:\s*)$|^Sheriff-Reverts: ([0-9a-f]{40})$"
    )
    .captures_iter(&body)
    {
        if git(
            repo,
            &[
                "rev-parse",
                "--verify",
                &format!("{}^{{commit}}", m.get(1).or(m.get(2)).unwrap().as_str()),
            ],
        )
        .is_ok()
        {
            return Ok(0);
        }
    }
    let missing = new_bars(repo, base, head)?;
    let rules = new_rules(repo, base, head)?;
    for (path, bar, line) in &missing {
        println!("{path}\t{}\t{line}", bar.text);
    }
    for (path, kind, line) in &rules {
        println!("{path}\t{kind}\t{line}");
    }
    if !missing.is_empty() {
        eprintln!(
            "provenance: {} new gating bars have no Provenance record; add {FORMS}",
            missing.len()
        );
    }
    if !rules.is_empty() {
        eprintln!(
            "provenance: {} new hooks or policy lines have no valid Provenance record; name the rule or its source path in the claim; add {FORMS}",
            rules.len()
        );
    }
    Ok(u8::from(!missing.is_empty() || !rules.is_empty()))
}
