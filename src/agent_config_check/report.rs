pub(super) struct Report {
    verbose: bool,
    pub(super) fail: usize,
    pub(super) warn: usize,
    details: Vec<String>,
}

impl Report {
    pub(super) fn new(verbose: bool) -> Self {
        Self { verbose, fail: 0, warn: 0, details: Vec::new() }
    }

    pub(super) fn ok(&mut self, message: impl std::fmt::Display) {
        self.details.push(format!("ok: {message}"));
    }

    pub(super) fn note(&self, message: impl std::fmt::Display) {
        if self.verbose {
            println!("  note: {message}");
        }
    }

    pub(super) fn bad(&mut self, message: impl std::fmt::Display) {
        eprintln!("  FAIL: {message}");
        self.fail += 1;
    }

    pub(super) fn soft(&mut self, message: impl std::fmt::Display) {
        eprintln!("  warn: {message}");
        self.warn += 1;
    }

    pub(super) fn group(&mut self, name: &str, summary: &str, before: usize) {
        if self.fail == before {
            println!("✓ {name:<13} {summary}");
        } else {
            eprintln!("✗ {name:<13} {summary}");
        }
        if self.verbose {
            for line in &self.details {
                println!("  {line}");
            }
        }
        self.details.clear();
    }

    pub(super) fn provider_group(&mut self, name: &str, before: usize, lines: &[String]) {
        if self.fail == before {
            println!("✓ {name}");
        } else {
            eprintln!("✗ {name}");
        }
        for line in lines {
            println!("  {line}");
        }
        if self.verbose {
            for line in &self.details {
                println!("    {line}");
            }
        }
        self.details.clear();
    }
}
