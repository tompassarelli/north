//! Verbs that folded under a domain noun: an unrecognized command naming one prints where it lives now.
const MOVED: &[(&[&str], &str)] = &[
    (&["scoreboard"], "ledger scoreboard"),
    (&["incident"], "ledger incident"),
    (&["land-abandon"], "land abandon"),
    (&["citation-check"], "check citations"),
    (&["issue-box-lint"], "check issue-boxes"),
    (&["lane-gc"], "lane gc"),
    (&["lane-layout"], "lane layout"),
    (&["lane-reap"], "lane reap"),
    (&["lane-reclaim"], "lane reclaim"),
    (&["lane-rescue"], "lane rescue"),
    (&["lane-seed"], "lane seed"),
    (&["lane-squash"], "lane squash"),
    (&["lane-sweep"], "lane sweep"),
];

pub(super) fn hint(arguments: &[String]) -> Option<String> {
    let (words, path) = MOVED.iter().find(|(words, _)| arguments.starts_with(&words.iter().map(|word| (*word).to_owned()).collect::<Vec<_>>()))?;
    let rest: Vec<&str> = arguments[words.len()..].iter().map(String::as_str).collect();
    Some(format!("did you mean: north {path}{}{}", if rest.is_empty() { "" } else { " " }, rest.join(" ")))
}

#[cfg(test)]
mod tests {
    use super::hint;

    fn words(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn a_moved_verb_names_its_new_path_and_keeps_its_arguments() {
        assert_eq!(hint(&words("scoreboard --weekly")).as_deref(), Some("did you mean: north ledger scoreboard --weekly"));
        assert_eq!(hint(&words("lane-gc --dry-run")).as_deref(), Some("did you mean: north lane gc --dry-run"));
        assert_eq!(hint(&words("flow")), None);
    }
}
