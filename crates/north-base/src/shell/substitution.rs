use super::scan::strip_comments;

fn single_quoted(out: &mut [char], stack: &mut Vec<char>, i: usize) -> usize {
    if out[i] == '\'' {
        stack.pop();
    } else {
        out[i] = '_';
    }
    i + 1
}

fn double_quoted(out: &mut [char], stack: &mut Vec<char>, i: usize) -> usize {
    let c = out[i];
    if c == '\\' && i + 1 < out.len() {
        out[i] = '_';
        out[i + 1] = '_';
        return i + 2;
    }
    if c == '"' {
        stack.pop();
    } else if c == '$' && out.get(i + 1) == Some(&'(') {
        out[i] = '_';
        out[i + 1] = '_';
        stack.push('S');
        return i + 2;
    } else {
        out[i] = '_';
    }
    i + 1
}

fn code(out: &mut [char], stack: &mut Vec<char>, i: usize) -> usize {
    let c = out[i];
    let nested = !stack.is_empty();
    if c == '\\' && i + 1 < out.len() {
        out[i + 1] = '_';
        return i + 2;
    }
    if c == '\'' || c == '"' {
        stack.push(c);
    } else if nested && c == '(' {
        stack.push('n');
    } else if nested && c == ')' && stack.pop() == Some('S') {
        out[i] = '_';
    }
    i + 1
}

pub fn unquoted_mask(text: &str) -> String {
    let mut out: Vec<_> = text.chars().collect();
    let mut stack: Vec<char> = Vec::new();
    let mut i = 0;
    while i < out.len() {
        i = match stack.last().copied() {
            Some('\'') => single_quoted(&mut out, &mut stack, i),
            Some('"') => double_quoted(&mut out, &mut stack, i),
            _ => code(&mut out, &mut stack, i),
        };
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
