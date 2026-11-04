use super::pattern;

pattern!(JUSTIFY_PREFIX, r#"^\s*CASE=([A-Za-z])\s+FACT=("([^"]*)"|'([^']*)')\s*"#);

pub(super) struct Case<'a> {
    pub letter: Option<String>,
    pub fact: String,
    pub bare: &'a str,
}

pub(super) fn parse_case(command: &str) -> Case<'_> {
    match JUSTIFY_PREFIX.captures(command) {
        Some(m) => Case {
            letter: Some(m[1].to_uppercase()),
            fact: m.get(3).or(m.get(4)).map_or("", |f| f.as_str()).trim().to_owned(),
            bare: &command[m.get(0).map_or(0, |all| all.end())..],
        },
        None => Case { letter: None, fact: String::new(), bare: command },
    }
}
