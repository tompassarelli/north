pub(super) fn js_space(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\x0B' | '\x0C' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}'
        | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}')
}

pub(super) fn squash(s: &str) -> String {
    s.split(js_space).filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" ")
}

pub(super) fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

pub(super) fn utf16_head(s: &str, units: usize) -> String {
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        used += c.len_utf16();
        if used > units {
            break;
        }
        out.push(c);
    }
    out
}

pub(super) fn utf16_tail(s: &str, units: usize) -> &str {
    let mut used = 0;
    for (i, c) in s.char_indices().rev() {
        used += c.len_utf16();
        if used > units {
            return &s[i + c.len_utf8()..];
        }
    }
    s
}
