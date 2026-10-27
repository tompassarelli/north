use std::collections::HashMap;

type OpenTag = (String, Vec<(String, String)>, bool);

#[derive(Debug, Default, Clone)]
pub(super) struct Element {
    pub(super) tag: String,
    pub(super) attributes: Vec<(String, String)>,
    pub(super) text: String,
    pub(super) children: Vec<Element>,
}

impl Element {
    pub(super) fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
    }

    pub(super) fn find(&self, tag: &str) -> Option<&Element> {
        self.children.iter().find(|child| child.tag == tag)
    }

    pub(super) fn find_all(&self, tag: &str) -> Vec<&Element> {
        self.children.iter().filter(|child| child.tag == tag).collect()
    }

    pub(super) fn find_text(&self, tag: &str) -> Option<String> {
        self.find(tag).map(|child| child.text.clone())
    }

    pub(super) fn text_or(&self, tag: &str, default: &str) -> String {
        self.find_text(tag).unwrap_or_else(|| default.to_owned())
    }

    pub(super) fn descendants(&self) -> Vec<&Element> {
        let mut found = Vec::new();
        for child in &self.children {
            found.push(child);
            found.extend(child.descendants());
        }
        found
    }

    pub(super) fn descendants_named(&self, tag: &str) -> Vec<&Element> {
        self.descendants().into_iter().filter(|element| element.tag == tag).collect()
    }

    pub(super) fn walk(&self) -> Vec<&Element> {
        let mut found = vec![self];
        found.extend(self.descendants());
        found
    }

    pub(super) fn local(&self) -> &str {
        self.tag.rsplit('}').next().unwrap_or(&self.tag)
    }
}

pub(super) fn parse(payload: &[u8]) -> Result<Element, String> {
    let text = std::str::from_utf8(payload).map_err(|_| "XML is not UTF-8".to_owned())?;
    let mut reader = Reader { text, at: 0 };
    reader.document()
}

struct Reader<'a> {
    text: &'a str,
    at: usize,
}

type Scope = HashMap<String, String>;

impl Reader<'_> {
    fn rest(&self) -> &str {
        &self.text[self.at..]
    }

    fn skip_until(&mut self, end: &str) -> Result<(), String> {
        match self.rest().find(end) {
            Some(offset) => {
                self.at += offset + end.len();
                Ok(())
            }
            None => Err(format!("unterminated construct before {end:?}")),
        }
    }

    fn document(&mut self) -> Result<Element, String> {
        if self.rest().starts_with('\u{feff}') {
            self.at += '\u{feff}'.len_utf8();
        }
        let mut root = None;
        loop {
            let rest = self.rest();
            let trimmed = rest.trim_start();
            self.at += rest.len() - trimmed.len();
            if self.rest().is_empty() {
                break;
            }
            if self.rest().starts_with("<?") {
                self.skip_until("?>")?;
            } else if self.rest().starts_with("<!--") {
                self.skip_until("-->")?;
            } else if self.rest().starts_with("<!DOCTYPE") {
                self.doctype()?;
            } else if self.rest().starts_with('<') {
                if root.is_some() {
                    return Err("junk after document element".into());
                }
                root = Some(self.element(&Scope::new())?);
            } else {
                return Err("text outside the document element".into());
            }
        }
        root.ok_or_else(|| "no element found".to_owned())
    }

    fn doctype(&mut self) -> Result<(), String> {
        let mut depth = 0;
        for (offset, character) in self.rest().char_indices() {
            match character {
                '[' => depth += 1,
                ']' => depth -= 1,
                '>' if depth <= 0 => {
                    self.at += offset + 1;
                    return Ok(());
                }
                _ => {}
            }
        }
        Err("unterminated DOCTYPE".into())
    }

    fn name(&mut self) -> Result<String, String> {
        let end = self.rest().find(|c: char| c.is_whitespace() || matches!(c, '=' | '>' | '/')).unwrap_or(self.rest().len());
        if end == 0 {
            return Err("name expected".into());
        }
        let name = self.rest()[..end].to_owned();
        self.at += end;
        Ok(name)
    }

    fn blank(&mut self) {
        let rest = self.rest();
        self.at += rest.len() - rest.trim_start().len();
    }

    fn open_tag(&mut self) -> Result<OpenTag, String> {
        self.at += 1;
        let raw = self.name()?;
        let mut attributes = Vec::new();
        loop {
            self.blank();
            if self.rest().starts_with("/>") {
                self.at += 2;
                return Ok((raw, attributes, true));
            }
            if self.rest().starts_with('>') {
                self.at += 1;
                return Ok((raw, attributes, false));
            }
            let key = self.name()?;
            self.blank();
            if !self.rest().starts_with('=') {
                return Err("attribute without a value".into());
            }
            self.at += 1;
            self.blank();
            let quote = self.rest().chars().next().filter(|c| matches!(c, '"' | '\'')).ok_or("attribute value must be quoted")?;
            self.at += 1;
            let end = self.rest().find(quote).ok_or("unterminated attribute value")?;
            let value = unescape(&self.rest()[..end])?;
            self.at += end + 1;
            if attributes.iter().any(|(existing, _): &(String, String)| *existing == key) {
                return Err("duplicate attribute".into());
            }
            attributes.push((key, value));
        }
    }

    fn element(&mut self, parent: &Scope) -> Result<Element, String> {
        let (raw, raw_attributes, empty) = self.open_tag()?;
        let mut scope = parent.clone();
        for (key, value) in &raw_attributes {
            if key == "xmlns" {
                scope.insert(String::new(), value.clone());
            } else if let Some(prefix) = key.strip_prefix("xmlns:") {
                scope.insert(prefix.to_owned(), value.clone());
            }
        }
        let tag = qualified(&raw, &scope, true)?;
        let mut attributes = Vec::new();
        for (key, value) in raw_attributes {
            if key == "xmlns" || key.starts_with("xmlns:") {
                continue;
            }
            attributes.push((qualified(&key, &scope, false)?, value));
        }
        let mut element = Element { tag, attributes, text: String::new(), children: Vec::new() };
        if !empty {
            self.content(&mut element, &raw, &scope)?;
        }
        Ok(element)
    }

    fn content(&mut self, element: &mut Element, raw: &str, scope: &Scope) -> Result<(), String> {
        loop {
            let next = self.rest().find('<').ok_or("unclosed element")?;
            let data = &self.rest()[..next];
            if element.children.is_empty() {
                element.text.push_str(&unescape(data)?);
            }
            self.at += next;
            if self.rest().starts_with("</") {
                self.at += 2;
                let close = self.name()?;
                self.blank();
                if !self.rest().starts_with('>') || close != raw {
                    return Err("mismatched tag".into());
                }
                self.at += 1;
                return Ok(());
            } else if self.rest().starts_with("<!--") {
                self.skip_until("-->")?;
            } else if self.rest().starts_with("<![CDATA[") {
                self.at += "<![CDATA[".len();
                let end = self.rest().find("]]>").ok_or("unterminated CDATA")?;
                if element.children.is_empty() {
                    element.text.push_str(&self.rest()[..end]);
                }
                self.at += end + 3;
            } else if self.rest().starts_with("<?") {
                self.skip_until("?>")?;
            } else {
                let child = self.element(scope)?;
                element.children.push(child);
            }
        }
    }
}

fn qualified(name: &str, scope: &Scope, default_namespace: bool) -> Result<String, String> {
    match name.split_once(':') {
        Some((prefix, local)) => match scope.get(prefix) {
            Some(uri) => Ok(format!("{{{uri}}}{local}")),
            None if prefix == "xml" => Ok(format!("{{http://www.w3.org/XML/1998/namespace}}{local}")),
            None => Err(format!("unbound prefix {prefix}")),
        },
        None => match scope.get("").filter(|uri| default_namespace && !uri.is_empty()) {
            Some(uri) => Ok(format!("{{{uri}}}{name}")),
            None => Ok(name.to_owned()),
        },
    }
}

fn unescape(text: &str) -> Result<String, String> {
    if !text.contains('&') {
        return Ok(text.to_owned());
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let tail = &rest[start + 1..];
        let end = tail.find(';').ok_or("unterminated entity")?;
        let entity = &tail[..end];
        match entity {
            "amp" => out.push('&'),
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            _ => {
                let number = entity.strip_prefix("#x").and_then(|hex| u32::from_str_radix(hex, 16).ok()).or_else(|| entity.strip_prefix('#').and_then(|dec| dec.parse().ok()));
                out.push(number.and_then(char::from_u32).ok_or("undefined entity")?);
            }
        }
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}
