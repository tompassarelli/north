#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PieceKind {
    Word,
    Sep,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Piece {
    pub kind: PieceKind,
    pub start: usize,
    pub end: usize,
}

fn ends_word(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\r' | ';' | '|' | '\n' | '(' | ')')
}

fn quoted_end(chars: &[char], start: usize) -> Option<usize> {
    let quote = chars[start];
    let mut index = start + 1;
    while index < chars.len() && chars[index] != quote {
        let substitution =
            chars[index] == '`' || (chars[index] == '$' && chars.get(index + 1) == Some(&'('));
        if quote == '"' && substitution {
            return None;
        }
        index += if quote == '"' && chars[index] == '\\' { 2 } else { 1 };
    }
    (index < chars.len()).then_some(index + 1)
}

fn word_end(chars: &[char], start: usize) -> Option<usize> {
    let mut index = start;
    while index < chars.len() {
        let c = chars[index];
        let next = chars.get(index + 1);
        if ends_word(c) || (c == '&' && !(index > start && matches!(chars[index - 1], '>' | '<'))) {
            break;
        }
        index = match c {
            '\'' | '"' => quoted_end(chars, index)?,
            '\\' if next.is_some() => index + 2,
            '\\' | '`' => return None,
            '#' if index == start => return None,
            '$' if next == Some(&'(') => return None,
            _ => index + 1,
        };
    }
    Some(index)
}

pub fn pieces(text: &str) -> Option<Vec<Piece>> {
    let chars: Vec<char> = text.chars().collect();
    let mut found = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let c = chars[index];
        if matches!(c, ' ' | '\t' | '\r') {
            index += 1;
        } else if ends_word(c) || c == '&' {
            if c == '&' && chars.get(index + 1) == Some(&'>') {
                return None;
            }
            found.push(Piece { kind: PieceKind::Sep, start: index, end: index + 1 });
            index += 1;
        } else {
            let end = word_end(&chars, index)?;
            found.push(Piece { kind: PieceKind::Word, start: index, end });
            index = end;
        }
    }
    Some(found)
}

pub fn slice(chars: &[char], piece: Piece) -> String {
    chars[piece.start..piece.end].iter().collect()
}

pub fn splice(chars: &[char], mut edits: Vec<(usize, usize, String)>) -> String {
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.0));
    let mut result = chars.to_vec();
    for (start, end, text) in edits {
        result.splice(start..end, text.chars());
    }
    result.into_iter().collect()
}

pub fn segments(pieces: &[Piece]) -> Vec<Vec<Piece>> {
    let mut found = vec![Vec::new()];
    for piece in pieces {
        if piece.kind == PieceKind::Sep {
            found.push(Vec::new());
        } else {
            found.last_mut().unwrap().push(*piece);
        }
    }
    found
}
