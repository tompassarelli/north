use regex::Regex;
use std::ops::Range;
use std::sync::LazyLock;

static CFG_TEST: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"#\s*(!?)\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]").unwrap());
static SHARED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:Rc\s*<\s*RefCell|RefCell|Arc\s*<\s*(?:std\s*::\s*)?(?:sync\s*::\s*)?(?:Mutex|RwLock))\s*<").unwrap()
});
static TRAIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\btrait\s+([A-Za-z_][A-Za-z0-9_]*)").unwrap());
static IMPL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bimpl\b").unwrap());
static FOR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bfor\b").unwrap());
static MOD_DECL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bmod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;").unwrap());
static SPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());

pub(super) struct RustFile {
    pub(super) path: String,
    code: String,
    tests: Vec<Range<usize>>,
    pub(super) whole_test: bool,
}

pub(super) struct Impl {
    pub(super) name: String,
    pub(super) line: usize,
    pub(super) test: bool,
}

impl RustFile {
    pub(super) fn new(path: &str, text: &str) -> Self {
        let code = mask(text);
        let mut whole_test = false;
        let mut tests = Vec::new();
        for found in CFG_TEST.captures_iter(&code) {
            let whole = found.get(0).expect("whole match");
            if found.get(1).is_some_and(|bang| !bang.is_empty()) {
                whole_test = true;
            }
            tests.push(whole.start()..item_end(code.as_bytes(), whole.end()));
        }
        Self { path: path.to_owned(), code, tests, whole_test }
    }

    fn is_test(&self, at: usize) -> bool {
        self.whole_test || self.tests.iter().any(|range| range.contains(&at))
    }

    fn line(&self, at: usize) -> usize {
        self.code.as_bytes()[..at].iter().filter(|byte| **byte == b'\n').count() + 1
    }

    pub(super) fn shared_state(&self) -> Vec<(usize, String)> {
        SHARED
            .find_iter(&self.code)
            .filter(|found| !self.is_test(found.start()))
            .map(|found| (self.line(found.start()), format!("{}..>", SPACE.replace_all(found.as_str(), ""))))
            .collect()
    }

    pub(super) fn traits(&self) -> Vec<(String, usize)> {
        TRAIT
            .captures_iter(&self.code)
            .filter_map(|found| {
                let name = found.get(1)?;
                (!self.is_test(name.start())).then(|| (name.as_str().to_owned(), self.line(name.start())))
            })
            .collect()
    }

    pub(super) fn impls(&self) -> Vec<Impl> {
        IMPL.find_iter(&self.code)
            .filter_map(|found| {
                let name = implemented_trait(&self.code[found.end()..])?;
                Some(Impl { name, line: self.line(found.start()), test: self.is_test(found.start()) })
            })
            .collect()
    }

    pub(super) fn child_modules(&self, all: bool) -> Vec<String> {
        MOD_DECL
            .captures_iter(&self.code)
            .filter(|found| all || self.whole_test || self.is_test(found.get(0).expect("whole match").start()))
            .flat_map(|found| child_files(&self.path, &found[1]))
            .collect()
    }
}

fn child_files(path: &str, name: &str) -> [String; 2] {
    let file = path.rsplit('/').next().unwrap_or(path);
    let dir = if matches!(file, "mod.rs" | "lib.rs" | "main.rs") {
        path[..path.len() - file.len()].trim_end_matches('/').to_owned()
    } else {
        path.strip_suffix(".rs").unwrap_or(path).to_owned()
    };
    let dir = if dir.is_empty() { String::new() } else { format!("{dir}/") };
    [format!("{dir}{name}.rs"), format!("{dir}{name}/mod.rs")]
}

fn implemented_trait(rest: &str) -> Option<String> {
    let header_end = rest.find(['{', ';']).unwrap_or(rest.len());
    let header = &rest[..header_end];
    let generics = generics_end(header);
    let found = FOR.find_iter(header).find(|found| found.start() >= generics && depth_at(header, found.start()) == 0)?;
    let path = header[generics..found.start()].trim().trim_start_matches('!').trim();
    let path = path.split('<').next().unwrap_or(path);
    let name = path.rsplit("::").next().unwrap_or(path).trim();
    (!name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_')).then(|| name.to_owned())
}

fn generics_end(header: &str) -> usize {
    let trimmed = header.trim_start();
    if !trimmed.starts_with('<') {
        return 0;
    }
    let offset = header.len() - trimmed.len();
    let mut depth = 0i32;
    for (at, byte) in trimmed.bytes().enumerate() {
        match byte {
            b'<' => depth += 1,
            b'>' if at > 0 && trimmed.as_bytes()[at - 1] == b'-' => {}
            b'>' => {
                depth -= 1;
                if depth == 0 {
                    return offset + at + 1;
                }
            }
            _ => {}
        }
    }
    header.len()
}

fn depth_at(header: &str, until: usize) -> i32 {
    let bytes = header.as_bytes();
    let mut depth = 0i32;
    for (at, byte) in bytes[..until].iter().enumerate() {
        match byte {
            b'<' | b'(' | b'[' => depth += 1,
            b'>' if at > 0 && bytes[at - 1] == b'-' => {}
            b'>' | b')' | b']' => depth -= 1,
            _ => {}
        }
    }
    depth
}

fn item_end(bytes: &[u8], from: usize) -> usize {
    let mut depth = 0i32;
    for (offset, byte) in bytes[from..].iter().enumerate() {
        match byte {
            b'{' | b'(' | b'[' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth <= 0 {
                    return from + offset + 1;
                }
            }
            b')' | b']' => depth -= 1,
            b';' if depth == 0 => return from + offset + 1,
            _ => {}
        }
    }
    bytes.len()
}

pub(super) fn mask(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = bytes.to_vec();
    let mut at = 0;
    while at < bytes.len() {
        let end = literal_end(bytes, at);
        if end > at {
            for byte in &mut out[at..end] {
                if *byte != b'\n' {
                    *byte = b' ';
                }
            }
            at = end;
        } else {
            at += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn ident(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn literal_end(bytes: &[u8], at: usize) -> usize {
    match (bytes[at], bytes.get(at + 1).copied()) {
        (b'/', Some(b'/')) => bytes[at..].iter().position(|byte| *byte == b'\n').map_or(bytes.len(), |end| at + end),
        (b'/', Some(b'*')) => block_comment_end(bytes, at),
        (b'"', _) => quoted_end(bytes, at + 1, b'"'),
        (b'\'', _) => char_end(bytes, at),
        (b'r', _) if at == 0 || !ident(bytes[at - 1]) || (bytes[at - 1] == b'b' && (at < 2 || !ident(bytes[at - 2]))) => {
            raw_end(bytes, at)
        }
        _ => at,
    }
}

fn block_comment_end(bytes: &[u8], at: usize) -> usize {
    let mut depth = 0;
    let mut index = at;
    while index + 1 < bytes.len() {
        match (bytes[index], bytes[index + 1]) {
            (b'/', b'*') => {
                depth += 1;
                index += 2;
            }
            (b'*', b'/') => {
                depth -= 1;
                index += 2;
                if depth == 0 {
                    return index;
                }
            }
            _ => index += 1,
        }
    }
    bytes.len()
}

fn quoted_end(bytes: &[u8], from: usize, close: u8) -> usize {
    let mut index = from;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            byte if byte == close => return index + 1,
            _ => index += 1,
        }
    }
    bytes.len()
}

fn char_end(bytes: &[u8], at: usize) -> usize {
    match bytes.get(at + 1) {
        Some(b'\\') => quoted_end(bytes, at + 1, b'\''),
        Some(first) => {
            let width = match *first {
                0xF0.. => 4,
                0xE0.. => 3,
                0xC0.. => 2,
                _ => 1,
            };
            if bytes.get(at + 1 + width) == Some(&b'\'') { at + 2 + width } else { at }
        }
        None => at,
    }
}

fn raw_end(bytes: &[u8], at: usize) -> usize {
    let hashes = bytes[at + 1..].iter().take_while(|byte| **byte == b'#').count();
    if bytes.get(at + 1 + hashes) != Some(&b'"') {
        return at;
    }
    let body = at + 2 + hashes;
    let mut index = body;
    while index < bytes.len() {
        if bytes[index] == b'"' && bytes[index + 1..].iter().take(hashes).filter(|byte| **byte == b'#').count() == hashes {
            return index + 1 + hashes;
        }
        index += 1;
    }
    bytes.len()
}
