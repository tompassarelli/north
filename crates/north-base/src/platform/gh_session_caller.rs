use std::path::Path;

const DEPTH: usize = 24;
const OWN_VERB: &str = "gh-session";

pub fn of_process(pid: u32) -> String {
    let proc = Path::new("/proc");
    let mut next = parent_of(proc, pid);
    for _ in 0..DEPTH {
        let Some(process) = next.filter(|process| *process > 1) else {
            break;
        };
        if let Some(caller) = std::fs::read(proc.join(process.to_string()).join("cmdline")).ok().and_then(|raw| named(&raw)) {
            return caller;
        }
        next = parent_of(proc, process);
    }
    "shell".to_owned()
}

fn parent_of(proc: &Path, pid: u32) -> Option<u32> {
    let stat = std::fs::read_to_string(proc.join(pid.to_string()).join("stat")).ok()?;
    stat.rsplit_once(')')?.1.split_whitespace().nth(1)?.parse().ok()
}

fn named(cmdline: &[u8]) -> Option<String> {
    let mut words = cmdline.split(|byte| *byte == 0).map(String::from_utf8_lossy);
    let program = words.next()?;
    let base = program.rsplit('/').next().unwrap_or("");
    if base != "north" && !base.starts_with("north-") {
        return None;
    }
    let verb = words.next().filter(|word| !word.starts_with('-'))?;
    if verb == OWN_VERB {
        return None;
    }
    let subverb = words.next().filter(|word| !word.is_empty() && word.chars().all(|character| character.is_ascii_lowercase() || character == '-') && !word.starts_with('-'));
    Some(subverb.map_or_else(|| verb.to_string(), |sub| format!("{verb}-{sub}")))
}

pub fn endpoint(arguments: &[String]) -> String {
    let mut words = arguments.iter().filter(|word| !word.starts_with('-')).map(String::as_str);
    let Some(first) = words.next() else {
        return String::new();
    };
    if first == "api" {
        let path = words.next().unwrap_or("").split('?').next().unwrap_or("").trim_start_matches('/');
        let generic: Vec<&str> = path.split('/').map(|part| if !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()) { ":n" } else { part }).collect();
        return format!("api {}", generic.join("/"));
    }
    match words.next() {
        Some(second) if second.bytes().all(|byte| byte.is_ascii_lowercase() || byte == b'-') => format!("{first} {second}"),
        _ => first.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::{endpoint, named};

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|word| (*word).to_owned()).collect()
    }

    #[test]
    fn a_north_command_names_its_verb_and_subverb_and_gh_session_itself_never_counts() {
        assert_eq!(named(b"/nix/store/x/bin/north\0hook\0public-text\0").as_deref(), Some("hook-public-text"));
        assert_eq!(named(b"north\0fleet\0tick\0--once\0").as_deref(), Some("fleet-tick"));
        assert_eq!(named(b"north\0gh-session\0gh\0issue\0").as_deref(), None);
        assert_eq!(named(b"bash\0-c\0north hook\0").as_deref(), None);
    }

    #[test]
    fn an_endpoint_keeps_the_gh_command_or_the_api_path_with_numbers_generalised() {
        assert_eq!(endpoint(&words(&["issue", "view", "638", "-R", "o/r"])), "issue view");
        assert_eq!(endpoint(&words(&["api", "/repos/o/r/issues/638/comments?per_page=5"])), "api repos/o/r/issues/:n/comments");
        assert_eq!(endpoint(&words(&["run", "list", "--limit", "9"])), "run list");
    }
}
