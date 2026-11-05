use super::super::search_words::{is_file, join, tilde};

const SAME: &str = "nilcvwxoqHFPab";
const DROPPED: &str = "rEGIsUT";
const VALUED: &str = "efmABCdD";
const LONG_FLAGS: &[(&str, Option<&str>)] = &[
    ("--line-number", Some("-n")),
    ("--ignore-case", Some("-i")),
    ("--files-with-matches", Some("-l")),
    ("--files-without-match", Some("--files-without-match")),
    ("--count", Some("-c")),
    ("--invert-match", Some("-v")),
    ("--word-regexp", Some("-w")),
    ("--line-regexp", Some("-x")),
    ("--only-matching", Some("-o")),
    ("--quiet", Some("-q")),
    ("--silent", Some("-q")),
    ("--with-filename", Some("-H")),
    ("--no-filename", Some("-I")),
    ("--fixed-strings", Some("-F")),
    ("--perl-regexp", Some("-P")),
    ("--text", Some("-a")),
    ("--null", Some("-0")),
    ("--byte-offset", Some("-b")),
    ("--dereference-recursive", Some("-L")),
    ("--recursive", None),
    ("--extended-regexp", None),
    ("--basic-regexp", None),
    ("--no-messages", None),
    ("--initial-tab", None),
    ("--binary", None),
    ("--color", None),
    ("--colour", None),
];
const LONG_VALUED: &[&str] = &[
    "--regexp",
    "--file",
    "--max-count",
    "--after-context",
    "--before-context",
    "--context",
    "--include",
    "--exclude",
    "--exclude-dir",
    "--binary-files",
    "--label",
    "--devices",
    "--directories",
    "--exclude-from",
    "--group-separator",
];

fn value_target(name: &str) -> Option<&'static str> {
    Some(match name {
        "e" | "--regexp" => "-e",
        "f" | "--file" => "-f",
        "m" | "--max-count" => "-m",
        "A" | "--after-context" => "-A",
        "B" | "--before-context" => "-B",
        "C" | "--context" => "-C",
        _ => return None,
    })
}

#[derive(Default)]
struct Translation {
    recursive: bool,
    explicit_pattern: bool,
    flags: Vec<char>,
    options: Vec<String>,
    operands: Vec<String>,
}

impl Translation {
    fn value_option(&mut self, name: &str, value: String) -> Option<()> {
        if matches!(name, "e" | "f" | "--regexp" | "--file") {
            self.explicit_pattern = true;
        }
        if let Some(target) = value_target(name) {
            self.options.extend([target.to_owned(), value]);
        } else if name == "--include" {
            self.options.extend(["-g".to_owned(), value]);
        } else if matches!(name, "--exclude" | "--exclude-dir") {
            self.options.extend(["-g".to_owned(), format!("!{value}")]);
        } else if matches!(name, "--label" | "--group-separator" | "--exclude-from") {
            return None;
        }
        Some(())
    }

    fn mapped_flag(&mut self, mapped: &str) {
        if mapped.len() == 2 {
            self.flags.push(mapped.chars().nth(1).unwrap());
        } else {
            self.options.push(mapped.to_owned());
        }
    }

    fn long(&mut self, argument: &str, rest: &mut std::slice::Iter<String>) -> Option<()> {
        let (name, value) = match argument.split_once('=') {
            Some((name, value)) => (name, Some(value.to_owned())),
            None => (argument, None),
        };
        if matches!(name, "--recursive" | "--dereference-recursive") {
            self.recursive = true;
        }
        if LONG_VALUED.contains(&name) {
            let value = value.or_else(|| rest.next().cloned())?;
            return self.value_option(name, value);
        }
        let (_, mapped) = LONG_FLAGS.iter().find(|(flag, _)| *flag == name)?;
        if let Some(mapped) = mapped {
            self.mapped_flag(mapped);
        }
        Some(())
    }

    fn short(&mut self, argument: &str, rest: &mut std::slice::Iter<String>) -> Option<()> {
        if argument[1..].bytes().all(|b| b.is_ascii_digit()) {
            self.options
                .extend(["-C".to_owned(), argument[1..].to_owned()]);
            return Some(());
        }
        let group: Vec<char> = argument[1..].chars().collect();
        let mut position = 0;
        while position < group.len() {
            let letter = group[position];
            position += 1;
            if matches!(letter, 'r' | 'R') {
                self.recursive = true;
            }
            if VALUED.contains(letter) {
                let attached: String = group[position..].iter().collect();
                let value = if attached.is_empty() {
                    rest.next().cloned()?
                } else {
                    attached
                };
                return self.value_option(&letter.to_string(), value);
            }
            if SAME.contains(letter) {
                self.flags.push(letter);
            } else if let Some(mapped) = short_map(letter) {
                self.mapped_flag(mapped);
            } else if !DROPPED.contains(letter) {
                return None;
            }
        }
        Some(())
    }
}

fn short_map(letter: char) -> Option<&'static str> {
    Some(match letter {
        'h' => "-I",
        'L' => "--files-without-match",
        'y' => "-i",
        'Z' => "-0",
        'R' => "-L",
        _ => return None,
    })
}

pub(super) fn to_rg(arguments: &[String], base: &str, home: &str) -> Option<Vec<String>> {
    translate(arguments, base, home, false)
}

impl Translation {
    fn flag_group(&self) -> Option<String> {
        let mut unique = String::new();
        for letter in &self.flags {
            if !unique.contains(*letter) {
                unique.push(*letter);
            }
        }
        (!unique.is_empty()).then(|| format!("-{unique}"))
    }
}

pub(super) fn translate(
    arguments: &[String],
    base: &str,
    home: &str,
    anchored: bool,
) -> Option<Vec<String>> {
    let mut state = Translation::default();
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        if argument == "--" {
            state.operands.extend(rest.by_ref().cloned());
        } else if argument.starts_with("--") {
            state.long(argument, &mut rest)?;
        } else if argument.starts_with('-') && argument.len() > 1 {
            state.short(argument, &mut rest)?;
        } else {
            state.operands.push(argument.clone());
        }
    }
    if !state.recursive {
        return None;
    }
    let (pattern, paths) = if state.explicit_pattern {
        (None, state.operands.as_slice())
    } else {
        let (pattern, paths) = state.operands.split_first()?;
        (Some(pattern), paths)
    };
    if !paths.is_empty()
        && paths
            .iter()
            .all(|path| is_file(&join(base, &tilde(path, home))))
    {
        return None;
    }
    let mut words = vec!["rg".to_owned()];
    words.extend(state.flag_group());
    words.extend(state.options.iter().cloned());
    if let Some(pattern) = pattern {
        if pattern.starts_with('-') {
            words.push("-e".to_owned());
        }
        words.push(pattern.clone());
    }
    words.extend(paths.iter().cloned());
    if anchored && paths.is_empty() {
        words.push(".".to_owned());
    }
    Some(words)
}
