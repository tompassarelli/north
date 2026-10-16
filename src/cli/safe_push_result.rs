fn line(status: &str, sha: &str, reason: &str, minutes: &str, url: &str) -> String {
    match status {
        "landed" => format!("safe-push: landed {sha}"),
        "timeout" => format!("safe-push: timed out after {minutes}m (run {url})"),
        _ => format!("safe-push: refused {reason}"),
    }
}

pub fn run(arguments: &[String]) -> u8 {
    let [status, sha, reason, minutes, url] = arguments else {
        eprintln!("Usage: north safe-push-result STATUS SHA REASON MINUTES URL");
        return 2;
    };
    if status == "landed" && sha.is_empty() {
        eprintln!("north safe-push-result: landed requires a commit sha");
        return 2;
    }
    println!("{}", line(status, sha, reason, minutes, url));
    0
}

#[cfg(test)]
mod tests {
    use super::line;

    #[test]
    fn landed_terminal_line_contains_the_published_sha() {
        let sha = "a".repeat(40);
        assert_eq!(
            line("landed", &sha, "", "0", ""),
            format!("safe-push: landed {sha}")
        );
    }
}
