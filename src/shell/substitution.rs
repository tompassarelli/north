use super::scan::strip_comments;

pub fn unquoted_mask(text: &str) -> String {
    let mut out: Vec<_> = text.chars().collect();
    let mut quote = None;
    let mut i = 0;
    while i < out.len() {
        let c = out[i];
        if let Some(q) = quote {
            if q == '"' && c == '\\' && i + 1 < out.len() {
                out[i] = '_';
                out[i + 1] = '_';
                i += 2;
                continue;
            }
            if c == q {
                quote = None;
            } else {
                out[i] = '_';
            }
        } else if c == '\'' || c == '"' {
            quote = Some(c);
        } else if c == '\\' && i + 1 < out.len() {
            out[i + 1] = '_';
            i += 1;
        }
        i += 1;
    }
    out.into_iter().collect()
}
pub fn shell_substitutions(text: &str) -> Vec<String> {
    let chars: Vec<_> = strip_comments(text).chars().collect();
    let mut bodies = Vec::new();
    let mut quote = None;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if quote == Some('\'') {
            if c == '\'' {
                quote = None;
            }
            i += 1;
            continue;
        }
        if c == '\\' && i + 1 < chars.len() {
            i += 2;
            continue;
        }
        if c == '"' {
            quote = if quote == Some('"') { None } else { Some('"') };
            i += 1;
            continue;
        }
        if c == '\'' && quote.is_none() {
            quote = Some('\'');
            i += 1;
            continue;
        }
        if c == '`' {
            let mut j = i + 1;
            let mut body = String::new();
            let mut closed = false;
            while j < chars.len() {
                if chars[j] == '\\' && j + 1 < chars.len() {
                    body.push(chars[j]);
                    body.push(chars[j + 1]);
                    j += 2;
                    continue;
                }
                if chars[j] == '`' {
                    bodies.push(body);
                    i = j + 1;
                    closed = true;
                    break;
                }
                body.push(chars[j]);
                j += 1;
            }
            if !closed {
                i += 1;
            }
            continue;
        }
        if c == '$' && chars.get(i + 1) == Some(&'(') {
            if chars.get(i + 2) == Some(&'(') {
                i += 3;
                continue;
            }
            let mut depth = 1;
            let mut nested_quote = None;
            let mut j = i + 2;
            let mut closed = false;
            while j < chars.len() {
                let nested = chars[j];
                if let Some(q) = nested_quote {
                    if q == '"' && nested == '\\' && j + 1 < chars.len() {
                        j += 2;
                        continue;
                    }
                    if nested == q {
                        nested_quote = None;
                    }
                    j += 1;
                    continue;
                }
                if nested == '\'' || nested == '"' {
                    nested_quote = Some(nested);
                } else if nested == '\\' && j + 1 < chars.len() {
                    j += 2;
                    continue;
                } else if nested == '(' {
                    depth += 1;
                } else if nested == ')' {
                    depth -= 1;
                    if depth == 0 {
                        bodies.push(chars[i + 2..j].iter().collect());
                        i = j + 1;
                        closed = true;
                        break;
                    }
                }
                j += 1;
            }
            if !closed {
                i += 2;
            }
            continue;
        }
        i += 1;
    }
    bodies
}
