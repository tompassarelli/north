#[derive(Debug, Eq, PartialEq)]
pub enum Terminal {
    Published(String),
    Queued(String),
    Clean(String),
    Refused(String),
    Cancelled,
}

pub fn receipt(terminal: &Terminal) -> (String, u8) {
    match terminal {
        Terminal::Published(line) | Terminal::Queued(line) | Terminal::Clean(line) => {
            (format!("safe-push: {line}"), 0)
        }
        Terminal::Refused(reason) => (format!("safe-push: refused {reason}"), 1),
        Terminal::Cancelled => ("safe-push: cancelled".to_owned(), 130),
    }
}

pub fn run(arguments: &[String]) -> u8 {
    let [status, _sha, reason, _minutes, _url] = arguments else {
        eprintln!("Usage: north safe-push-result refused SHA REASON MINUTES URL");
        return 2;
    };
    if status != "refused" {
        eprintln!("north safe-push-result: only refused is a terminal result bash reports");
        return 2;
    }
    println!("{}", receipt(&Terminal::Refused(reason.clone())).0);
    0
}

#[cfg(test)]
mod tests {
    use super::{Terminal, receipt};

    #[test]
    fn each_terminal_has_its_line_and_exit_code() {
        assert_eq!(
            receipt(&Terminal::Refused("not on a branch".to_owned())),
            ("safe-push: refused not on a branch".to_owned(), 1)
        );
        assert_eq!(
            receipt(&Terminal::Cancelled),
            ("safe-push: cancelled".to_owned(), 130)
        );
        assert_eq!(
            receipt(&Terminal::Clean("✓ clean".to_owned())),
            ("safe-push: ✓ clean".to_owned(), 0)
        );
        assert_eq!(
            receipt(&Terminal::Queued("queued".to_owned())),
            ("safe-push: queued".to_owned(), 0)
        );
        assert_eq!(
            receipt(&Terminal::Published("pushed".to_owned())),
            ("safe-push: pushed".to_owned(), 0)
        );
    }
}
