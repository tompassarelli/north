use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Default)]
pub(super) struct Matrix {
    pub(super) baseline: BTreeMap<String, bool>,
    pub(super) mutants: BTreeMap<String, bool>,
    pub(super) kills: BTreeMap<String, BTreeSet<String>>,
}

impl Matrix {
    pub(super) fn parse(text: &str) -> Self {
        let mut matrix = Self::default();
        for line in text.lines() {
            match line.split('\t').collect::<Vec<_>>().as_slice() {
                ["baseline", test, verdict] => {
                    matrix.baseline.insert((*test).to_owned(), *verdict == "pass");
                }
                ["mutant", id, _, state] => {
                    matrix.mutants.insert((*id).to_owned(), *state == "ran");
                }
                ["kill", id, test] => {
                    matrix.kills.entry((*id).to_owned()).or_default().insert((*test).to_owned());
                }
                _ => {}
            }
        }
        matrix
    }

    pub(super) fn render(&self, labels: &BTreeMap<String, String>) -> String {
        let mut out = String::new();
        for (test, pass) in &self.baseline {
            out.push_str(&format!("baseline\t{test}\t{}\n", if *pass { "pass" } else { "fail" }));
        }
        for (id, ran) in &self.mutants {
            let label = labels.get(id).map(String::as_str).unwrap_or_default();
            out.push_str(&format!("mutant\t{id}\t{label}\t{}\n", if *ran { "ran" } else { "dead" }));
        }
        for (id, tests) in &self.kills {
            for test in tests {
                out.push_str(&format!("kill\t{id}\t{test}\n"));
            }
        }
        out
    }
}

pub(super) struct Catch {
    pub(super) catches: u32,
    pub(super) last_seen: String,
}

pub(super) fn evidence(tsv: &str) -> Result<BTreeMap<String, Catch>, String> {
    let mut lines = tsv.lines();
    let header: Vec<&str> = lines.next().unwrap_or_default().split('\t').collect();
    let column = |name: &str| header.iter().position(|field| *field == name).ok_or_else(|| format!("evidence has no {name} column"));
    let (test, catches, last_seen) = (column("test")?, column("catches")?, column("last_seen")?);
    Ok(lines
        .map(|line| line.split('\t').collect::<Vec<_>>())
        .filter(|fields| fields.len() > test.max(catches).max(last_seen))
        .map(|fields| {
            (fields[test].to_owned(), Catch { catches: fields[catches].parse().unwrap_or(0), last_seen: fields[last_seen].to_owned() })
        })
        .collect())
}

fn same_test(a: &str, b: &str) -> bool {
    a == b || a.ends_with(&format!("::{b}")) || b.ends_with(&format!("::{a}"))
}

pub(super) struct Verdict {
    pub(super) idle: Vec<String>,
    pub(super) unmeasured: usize,
    pub(super) caught: usize,
    pub(super) unique: usize,
    pub(super) contracts: usize,
    pub(super) live_mutants: usize,
}

pub(super) fn select(matrices: &[Matrix], evidence: &BTreeMap<String, Catch>, contracts: &[String], since: &str) -> Verdict {
    let measured: BTreeMap<&str, bool> = matrices.last().map(|latest| latest.baseline.iter().map(|(test, pass)| (test.as_str(), *pass)).collect()).unwrap_or_default();
    let killers: BTreeSet<&str> = matrices
        .iter()
        .flat_map(|matrix| matrix.kills.values())
        .filter(|tests| tests.len() == 1)
        .flat_map(|tests| tests.iter().map(String::as_str))
        .collect();
    let recent: Vec<&str> = evidence.iter().filter(|(_, row)| row.catches > 0 && row.last_seen.as_str() >= since).map(|(test, _)| test.as_str()).collect();
    let mut verdict = Verdict {
        idle: Vec::new(),
        unmeasured: measured.values().filter(|pass| !**pass).count(),
        caught: 0,
        unique: 0,
        contracts: 0,
        live_mutants: matrices.iter().flat_map(|matrix| matrix.mutants.values()).filter(|ran| **ran).count(),
    };
    for test in measured.iter().filter(|(_, pass)| **pass).map(|(test, _)| *test) {
        if contracts.iter().any(|contract| same_test(test, contract)) {
            verdict.contracts += 1;
        } else if recent.iter().any(|caught| same_test(test, caught)) {
            verdict.caught += 1;
        } else if killers.contains(test) {
            verdict.unique += 1;
        } else {
            verdict.idle.push(test.to_owned());
        }
    }
    verdict
}

fn grouped(tests: &[String]) -> String {
    let mut groups = BTreeMap::<&str, Vec<&str>>::new();
    for test in tests {
        let (module, name) = test.rsplit_once("::").unwrap_or(("", test));
        groups.entry(module).or_default().push(name);
    }
    groups
        .into_iter()
        .map(|(module, names)| match (module, names.as_slice()) {
            ("", _) => names.iter().map(|name| format!("`{name}`")).collect::<Vec<_>>().join(", "),
            (_, [one]) => format!("`{module}::{one}`"),
            _ => format!("`{module}::{{{}}}`", names.join(", ")),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

pub(super) fn render_box(repo: &str, date: &str, since: &str, verdict: &Verdict) -> String {
    format!(
        "- [ ] {repo} retirement sweep {date}: delete or justify the {} tests with no catch since {since}, no unique kill among {} live mutants and no `[mutant] contract` ({} caught, {} unique killers, {} contracts kept; {} failed on the untouched tree and were not judged): {}",
        verdict.idle.len(),
        verdict.live_mutants,
        verdict.caught,
        verdict.unique,
        verdict.contracts,
        verdict.unmeasured,
        grouped(&verdict.idle),
    )
}
