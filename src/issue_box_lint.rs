use regex::Regex;
use std::sync::LazyLock;

static BOX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\s*[-*] \[ \]\s*(.+)$").unwrap());
static VERIFIER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\bTom(?:'s)?\s+(?:tries|tests?|checks?|confirms?|verifies|plays|playtests?|launches|(?:next\s+)?(?:launch|playtest|play session))\b|\bon (?:his|Tom's) (?:pad|controller)\b|\bin (?:his|Tom's) (?:next )?playtest\b").unwrap()
});
static PROOF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:proof|check|checked|verified|tested|confirmed)\s*\(").unwrap()
});
static TOM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bTom\b").unwrap());
static MECHANICAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:pad|input|controller|button|keys?|cursor|binds?|hitbox(?:es)?|sizes?|scale|frames?|timing|fps|sounds?|audio|menus?|lists?|appears?|loads?|launch(?:es)?|starts?|prints|builds on|installs?|works?|crash(?:es)?)\b").unwrap()
});
static JUDGMENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:feel|taste|fun|looks?|reads? well|direction|prefers?|likes?)\b").unwrap()
});
static SOURCED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\borigin (?:tom|experiment|prior-art|incident)\b|\bsoft\b").unwrap()
});
fn flagged(body: &str) -> Vec<&str> {
    BOX.captures_iter(body)
        .map(|m| m.get(1).unwrap().as_str())
        .filter(|box_text| {
            let verifier = VERIFIER.is_match(box_text)
                || PROOF.find_iter(box_text).any(|proof| {
                    let rest = &box_text[proof.end()..];
                    let inside = rest.split(')').next().unwrap();
                    TOM.find_iter(inside).any(|tom| {
                        !["'s machine", "'s laptop"]
                            .iter()
                            .any(|s| rest[tom.end()..].to_lowercase().starts_with(s))
                    })
                });
            verifier && MECHANICAL.is_match(&JUDGMENT.replace_all(box_text, ""))
                || !crate::provenance::bars(box_text).is_empty() && !SOURCED.is_match(box_text)
        })
        .collect()
}
fn gh(args: &[&str]) -> Result<serde_json::Value, String> {
    crate::platform::github::json(args)
}
fn lint(args: &[String]) -> Result<u8, String> {
    let mut items = Vec::new();
    let mut repos = Vec::new();
    let mut i = 0;
    let mut positional = false;
    let mut positional_done = false;
    let mut unknown = Vec::new();
    while i < args.len() {
        let (key, inline) = args[i]
            .split_once('=')
            .map_or((args[i].as_str(), None), |(k, v)| (k, Some(v)));
        if !positional && (key == "-h" || (key.len() > 2 && "--help".starts_with(key))) {
            if let Some(value) = inline {
                return Ok(usage_error(&format!(
                    "argument -h/--help: ignored explicit argument '{value}'"
                )));
            }
            print!("{HELP}");
            return Ok(0);
        }
        if !positional && key.starts_with("--") && key.len() > 2 && "--repo".starts_with(key) {
            positional_done |= !items.is_empty();
            if let Some(value) = inline {
                repos.push(value);
                i += 1;
                continue;
            }
            i += 1;
            if i >= args.len()
                || (args[i].starts_with('-')
                    && !Regex::new(r"\A-\d+$|\A-\d*\.\d+$")
                        .unwrap()
                        .is_match(&args[i]))
            {
                return Ok(usage_error("argument --repo: expected one argument"));
            }
            repos.push(args[i].as_str());
        } else if !positional && args[i] == "--" {
            positional = true;
            if positional_done {
                unknown.push(args[i].as_str());
            }
        } else if !positional && args[i].starts_with('-') {
            positional_done |= !items.is_empty();
            unknown.push(args[i].as_str());
        } else if positional_done {
            unknown.push(args[i].as_str());
        } else {
            items.push(&args[i]);
        }
        i += 1;
    }
    if !unknown.is_empty() {
        return Ok(usage_error(&format!(
            "unrecognized arguments: {}",
            unknown.join(" ")
        )));
    }
    let mut issues = Vec::new();
    for item in items {
        let (repo, number) = item.split_once('#').ok_or("expected REPO#N")?;
        let value = gh(&[
            "issue",
            "view",
            number,
            "-R",
            &format!("tompassarelli/{repo}"),
            "--json",
            "body",
        ])?;
        issues.push((
            item.to_owned(),
            value["body"].as_str().unwrap_or("").to_owned(),
        ));
    }
    for repo in repos {
        let values = gh(&[
            "issue",
            "list",
            "-R",
            &format!("tompassarelli/{repo}"),
            "--state",
            "open",
            "--limit",
            "500",
            "--json",
            "number,body",
        ])?;
        for value in values.as_array().ok_or("expected issue list")? {
            issues.push((
                format!("{repo}#{}", value["number"]),
                value["body"].as_str().unwrap_or("").to_owned(),
            ));
        }
    }
    let mut count = 0;
    for (item, body) in issues {
        for box_text in flagged(&body) {
            count += 1;
            println!("{item}\t{box_text}");
        }
    }
    eprintln!(
        "issue-box-lint: {count} boxes hand Tom a mechanical check (agents verify them on VM pairs and native capture) or name a bar without an origin or soft"
    );
    Ok(u8::from(count > 0))
}
fn usage_error(message: &str) -> u8 {
    eprintln!(
        "{}\nissue-box-lint: error: {message}",
        HELP.lines().next().unwrap()
    );
    2
}
const HELP: &str = r#"usage: issue-box-lint [-h] [--repo REPO] [issues ...]

issue-box-lint: flag unchecked Done when boxes that hand Tom a mechanical
check or a bar without provenance.

positional arguments:
  issues       REPO#N

options:
  -h, --help   show this help message and exit
  --repo REPO  lint every open issue in REPO
"#;

pub(crate) fn run(args: &[String]) -> u8 {
    match lint(args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_smashcraft_12_oct_boxes_flag_only_mechanical_checks_routed_to_tom() {
        let body=[
            "- [ ] Tom's play session on the current patch build (findings go into the next patch). Product proof (laptop, Tom).",
            "- [ ] Product proof (laptop, Tom's next launch): the controller helper builds on Tom's laptop for the current build (`bun wisp play` prints a helper path, not \"keyboard\").",
            "- [ ] Tom (note 7): \"the cursor should start on \"start match\" on the screen selection screen\". Entering stage select puts the pad focus and the mouse cursor on Start Match.",
            "- [ ] Tom tries it on his pad in his next playtest.",
            "- [ ] Tom's feel check in his next playtest.",
            "- [x] Tom tries the old pad build.",
            "- [ ] Frame native time is measured once per release candidate on Tom's machine, by one quiet laptop run under an exclusive lease.",
        ].join("\n");
        assert_eq!(
            flagged(&body)
                .iter()
                .map(|b| b.chars().take(40).collect::<String>())
                .collect::<Vec<_>>(),
            vec![
                "Product proof (laptop, Tom's next launch",
                "Tom tries it on his pad in his next play"
            ]
        );
        assert_eq!(
            flagged("- [ ] proof (Tom and Tom's laptop): input works"),
            vec!["proof (Tom and Tom's laptop): input works"]
        );
        assert_eq!(
            flagged("- [ ] proof (Tom's laptop and Tom): input works"),
            vec!["proof (Tom's laptop and Tom): input works"]
        );
    }
    #[test]
    fn test_galileo_box_with_a_bar_flags_only_without_an_origin() {
        let body=["- [ ] 9.11p: the 1,000-row query keystroke stays under 50 ms.","- [ ] 9.11p: the 1,000-row query keystroke stays under 50 ms (origin experiment; pointer galileo bench/keystroke.json plus `just bench keystroke`).","- [ ] 9.12p: export finishes under 2 s (soft)."].join("\n");
        assert_eq!(
            flagged(&body),
            vec!["9.11p: the 1,000-row query keystroke stays under 50 ms."]
        );
    }
}
