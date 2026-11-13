use regex::Regex;
use std::sync::LazyLock;

pub(super) const UNIT: &str = r"(seconds|minutes|instructions|allocations|launches|frames|tokens|bytes|lines|units|sec|min|fps|MiB|GiB|KiB|MB|KB|GB|ns|µs|us|ms|s|h|%)";
pub(super) const NUMBER: &str = r"(\d[\d,]*(?:\.\d+)?)";
static BAR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"(?:(?i:(at least))\s+{NUMBER}\s*[×x]\s+(?i:fewer)\s+{UNIT})|(?:(?i:(at or under|at most|no more than|less than|faster than|under|below|within))\s+|(<=|≤|<)\s*){NUMBER}\s*{UNIT}")).unwrap()
});

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Bar {
    pub(super) comparator: String,
    pub(super) number: String,
    pub(super) unit: String,
    pub(super) text: String,
}
pub(super) fn word(c: char) -> bool {
    c == '_' || c.is_alphanumeric()
}
pub fn bars(text: &str) -> Vec<Bar> {
    BAR.captures_iter(text)
        .filter_map(|m| {
            let whole = m.get(0).unwrap();
            let symbolic = m.get(5).is_some();
            if !symbolic && text[..whole.start()].chars().next_back().is_some_and(word) {
                return None;
            }
            let (cmp, number, unit) = if m.get(1).is_some() {
                (&m[1], &m[2], &m[3])
            } else {
                (m.get(4).or(m.get(5)).unwrap().as_str(), &m[6], &m[7])
            };
            let end = m.get(if m.get(1).is_some() { 3 } else { 7 }).unwrap().end();
            if text[end..].chars().next().is_some_and(word) {
                return None;
            }
            let cmp = cmp.to_lowercase();
            let comparator = match cmp.as_str() {
                "<=" | "≤" | "at or under" | "no more than" => "at most",
                "<" | "less than" | "below" => "under",
                "at least" => "at least x fewer",
                _ => &cmp,
            }
            .into();
            let (number, unit) = quantity(number, unit);
            Some(Bar {
                comparator,
                number,
                unit,
                text: whole.as_str().into(),
            })
        })
        .collect()
}

pub(super) fn quantity(number: &str, unit: &str) -> (String, String) {
    let number: String = number
        .chars()
        .filter(|c| *c != ',')
        .map(|c| {
            if c.is_ascii() {
                return c;
            }
            let mut first = c as u32;
            while first > 0
                && char::from_u32(first - 1).is_some_and(|p| re!(r"^\d$").is_match(&p.to_string()))
            {
                first -= 1;
            }
            char::from_digit((c as u32 - first) % 10, 10).unwrap()
        })
        .collect();
    let number = number.parse::<f64>().unwrap_or(f64::NAN);
    let raw = format!("{number:.5e}");
    let mut number = if !number.is_finite() {
        number.to_string()
    } else {
        let (mantissa, exp) = raw.split_once('e').unwrap();
        let exp: i32 = exp.parse().unwrap();
        if !(-4..6).contains(&exp) {
            format!(
                "{}e{exp:+03}",
                mantissa.trim_end_matches('0').trim_end_matches('.')
            )
        } else {
            format!(
                "{:.*}",
                (5 - exp).max(0) as usize,
                raw.parse::<f64>().unwrap()
            )
        }
    };
    if number.contains('.') && !number.contains('e') {
        number = number.trim_end_matches('0').trim_end_matches('.').into();
    }
    (
        number,
        match unit {
            "sec" | "seconds" => "s",
            "minutes" => "min",
            "µs" => "us",
            _ => unit,
        }
        .into(),
    )
}
