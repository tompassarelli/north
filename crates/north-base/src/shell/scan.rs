#[derive(Clone, Copy, PartialEq)]
pub(super) enum Class {
    Code,
    Quoted,
    Comment,
}

enum Frame {
    Code(Vec<bool>),
    Double,
}

struct Scanner {
    chars: Vec<char>,
    classes: Vec<Class>,
    stack: Vec<Frame>,
    previous: Option<char>,
    closed_substitution: bool,
}

pub(super) fn classify(text: &str) -> Vec<Class> {
    let chars: Vec<_> = text.chars().collect();
    let mut scanner = Scanner {
        classes: vec![Class::Code; chars.len()],
        chars,
        stack: vec![Frame::Code(Vec::new())],
        previous: Some('\n'),
        closed_substitution: false,
    };
    let mut i = 0;
    while i < scanner.chars.len() {
        i = match scanner.stack.last() {
            Some(Frame::Double) => scanner.double(i),
            _ => scanner.code(i),
        };
    }
    scanner.classes
}

pub(super) fn strip_comments(text: &str) -> String {
    text.chars()
        .zip(classify(text))
        .map(|(c, class)| if class == Class::Comment { ' ' } else { c })
        .collect()
}

impl Scanner {
    fn mark(&mut self, range: std::ops::Range<usize>, class: Class) -> usize {
        let end = range.end.min(self.chars.len());
        self.classes[range.start..end].fill(class);
        range.end
    }

    fn code(&mut self, i: usize) -> usize {
        let c = self.chars[i];
        let word_start = self.previous.is_some_and(|p| " \t\n;&|(".contains(p))
            || (self.previous == Some(')') && !self.closed_substitution);
        self.closed_substitution = false;
        if c == '#' && word_start {
            let end = self.chars[i..]
                .iter()
                .position(|c| *c == '\n')
                .map_or(self.chars.len(), |n| i + n);
            return self.mark(i..end, Class::Comment);
        }
        if c == '\\' {
            self.previous = None;
            return self.mark(i + 1..i + 2, Class::Quoted);
        }
        if c == '\'' {
            self.previous = None;
            let close = self.chars[i + 1..].iter().position(|c| *c == '\'');
            return self.mark(
                i..close.map_or(self.chars.len(), |n| i + n + 2),
                Class::Quoted,
            );
        }
        if c == '$' && self.chars.get(i + 1) == Some(&'\'') {
            self.previous = None;
            let end = ansi_c_end(&self.chars, i + 2);
            return self.mark(i..end, Class::Quoted);
        }
        if c == '"' {
            self.previous = None;
            self.stack.push(Frame::Double);
            return self.mark(i..i + 1, Class::Quoted);
        }
        self.nest(c);
        self.previous = Some(c);
        i + 1
    }

    fn nest(&mut self, c: char) {
        let root = self.stack.len() == 1;
        let Some(Frame::Code(openers)) = self.stack.last_mut() else {
            return;
        };
        if c == '(' {
            let substitution = self.previous.is_some_and(|p| "$<>=".contains(p))
                || (self.previous == Some('(') && openers.last() == Some(&true));
            openers.push(substitution);
        } else if c == ')' {
            self.closed_substitution = openers.pop().unwrap_or(false);
            if openers.is_empty() && !root {
                self.stack.pop();
            }
        }
    }

    fn double(&mut self, i: usize) -> usize {
        match self.chars[i] {
            '"' => {
                self.stack.pop();
                self.mark(i..i + 1, Class::Quoted)
            }
            '\\' => self.mark(i..i + 2, Class::Quoted),
            '$' if self.chars.get(i + 1) == Some(&'(') => {
                self.stack.push(Frame::Code(vec![true]));
                self.previous = Some('(');
                i + 2
            }
            _ => self.mark(i..i + 1, Class::Quoted),
        }
    }
}

fn ansi_c_end(chars: &[char], mut i: usize) -> usize {
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 2,
            '\'' => return i + 1,
            _ => i += 1,
        }
    }
    chars.len()
}
