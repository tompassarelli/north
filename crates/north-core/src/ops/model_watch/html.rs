#[derive(Default)]
pub(super) struct PublicPage {
    pub(super) text: Vec<String>,
    pub(super) links: Vec<(String, String)>,
    hidden: usize,
    href: String,
    anchor: Vec<String>,
}

impl PublicPage {
    pub(super) fn parse(source: &str) -> Self {
        let mut page = Self::default();
        let mut rest = source;
        let mut data = String::new();
        while !rest.is_empty() {
            let Some(open) = rest.find('<') else {
                data.push_str(rest);
                break;
            };
            data.push_str(&rest[..open]);
            let tail = &rest[open..];
            if tail.starts_with("<!--") {
                page.flush(&mut data);
                rest = tail.find("-->").map_or("", |end| &tail[end + 3..]);
            } else if tail.starts_with("<!") || tail.starts_with("<?") {
                page.flush(&mut data);
                rest = tail.find('>').map_or("", |end| &tail[end + 1..]);
            } else if let Some(closing) = tail.strip_prefix("</").filter(|after| after.starts_with(|c: char| c.is_ascii_alphabetic())) {
                page.flush(&mut data);
                let end = closing.find('>').map_or(closing.len(), |end| end);
                page.end(&closing[..end].trim().split(|c: char| c.is_whitespace()).next().unwrap_or("").to_ascii_lowercase());
                rest = closing.get(end + 1..).unwrap_or("");
            } else if tail[1..].starts_with(|c: char| c.is_ascii_alphabetic()) {
                page.flush(&mut data);
                let (name, attributes, self_closing, after) = start_tag(&tail[1..]);
                page.start(&name, &attributes);
                if self_closing {
                    page.end(&name);
                } else if matches!(name.as_str(), "script" | "style") {
                    let close = format!("</{name}");
                    let lower = after.to_ascii_lowercase();
                    let end = lower.find(&close).unwrap_or(after.len());
                    rest = &after[end..];
                    continue;
                }
                rest = after;
            } else {
                data.push('<');
                page.flush(&mut data);
                rest = &tail[1..];
            }
        }
        page.flush(&mut data);
        page
    }

    fn flush(&mut self, data: &mut String) {
        if data.is_empty() {
            return;
        }
        let text = unescape(data);
        data.clear();
        if self.hidden == 0 {
            if !self.href.is_empty() {
                self.anchor.push(text.clone());
            }
            self.text.push(text);
        }
    }

    fn start(&mut self, tag: &str, attributes: &[(String, String)]) {
        if matches!(tag, "script" | "style") {
            self.hidden += 1;
        }
        if tag == "a" && self.hidden == 0 {
            self.href = attributes.iter().rev().find(|(key, _)| key == "href").map(|(_, value)| value.clone()).unwrap_or_default();
            self.anchor.clear();
        }
    }

    fn end(&mut self, tag: &str) {
        if matches!(tag, "script" | "style") {
            self.hidden = self.hidden.saturating_sub(1);
        }
        if tag == "a" && !self.href.is_empty() {
            self.links.push((std::mem::take(&mut self.href), self.anchor.join(" ")));
        }
    }
}

fn start_tag(input: &str) -> (String, Vec<(String, String)>, bool, &str) {
    let name_end = input.find(|c: char| c.is_whitespace() || matches!(c, '/' | '>')).unwrap_or(input.len());
    let name = input[..name_end].to_ascii_lowercase();
    let mut at = name_end;
    let mut attributes = Vec::new();
    let mut self_closing = false;
    loop {
        let rest = &input[at..];
        let trimmed = rest.trim_start_matches(|c: char| c.is_whitespace() || c == '/');
        let skipped = rest.len() - trimmed.len();
        if rest[..skipped].contains('/') && trimmed.starts_with('>') {
            self_closing = true;
        }
        at += skipped;
        let rest = &input[at..];
        if rest.is_empty() {
            return (name, attributes, self_closing, "");
        }
        if let Some(after) = rest.strip_prefix('>') {
            return (name, attributes, self_closing, after);
        }
        let key_end = rest.find(|c: char| c.is_whitespace() || matches!(c, '=' | '>' | '/')).unwrap_or(rest.len()).max(1);
        let key = rest[..key_end].to_ascii_lowercase();
        at += key_end;
        let rest = input[at..].trim_start();
        at = input.len() - rest.len();
        let value = if let Some(after) = rest.strip_prefix('=') {
            let after = after.trim_start();
            let (value, used) = match after.chars().next() {
                Some(quote @ ('"' | '\'')) => {
                    let end = after[1..].find(quote).map_or(after.len() - 1, |end| end);
                    (after[1..1 + end].to_owned(), (1 + end + 1).min(after.len()))
                }
                _ => {
                    let end = after.find(|c: char| c.is_whitespace() || c == '>').unwrap_or(after.len());
                    (after[..end].to_owned(), end)
                }
            };
            at = input.len() - after.len() + used;
            unescape(&value)
        } else {
            String::new()
        };
        attributes.push((key, value));
    }
}

fn unescape(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let tail = &rest[start + 1..];
        let decoded = tail.find(';').filter(|end| *end <= 32).and_then(|end| entity(&tail[..end]).map(|character| (character, end + 1)));
        match decoded {
            Some((character, used)) => {
                out.push(character);
                rest = &tail[used..];
            }
            None => {
                out.push('&');
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

const NAMED: [(&str, char); 18] = [
    ("amp", '&'),
    ("lt", '<'),
    ("gt", '>'),
    ("quot", '"'),
    ("apos", '\''),
    ("nbsp", '\u{a0}'),
    ("copy", '\u{a9}'),
    ("reg", '\u{ae}'),
    ("ndash", '\u{2013}'),
    ("mdash", '\u{2014}'),
    ("lsquo", '\u{2018}'),
    ("rsquo", '\u{2019}'),
    ("ldquo", '\u{201c}'),
    ("rdquo", '\u{201d}'),
    ("hellip", '\u{2026}'),
    ("middot", '\u{b7}'),
    ("bull", '\u{2022}'),
    ("trade", '\u{2122}'),
];

fn entity(name: &str) -> Option<char> {
    let Some(number) = name.strip_prefix('#') else {
        return NAMED.iter().find(|(known, _)| *known == name).map(|(_, character)| *character);
    };
    let value = number.strip_prefix(['x', 'X']).map_or_else(|| number.parse().ok(), |hex| u32::from_str_radix(hex, 16).ok())?;
    char::from_u32(value)
}
