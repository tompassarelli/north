const DOLLAR: char = '\u{1}';
const TICK: char = '\u{2}';

struct Neutralizer {
    single: bool,
    double: bool,
    tick: bool,
    escaped: bool,
    comment: bool,
    previous: char,
    out: String,
}

impl Neutralizer {
    fn comment_char(&mut self, ch: char, literal: bool, word_start: bool) -> bool {
        if self.comment {
            self.comment = ch != '\n';
            return self.comment;
        }
        let starts = ch == '#' && word_start && !self.single && !self.double && !literal;
        self.comment = starts;
        starts
    }

    fn push(&mut self, ch: char) {
        let literal = self.escaped;
        self.escaped = !literal && !self.single && ch == '\\';
        let word_start = self.previous.is_whitespace() || ";&|(".contains(self.previous);
        self.previous = ch;
        if self.comment_char(ch, literal, word_start) {
            return;
        }
        match ch {
            '\'' if !literal && !self.double => self.single = !self.single,
            '"' if !literal && !self.single => self.double = !self.double,
            '$' if self.single || literal => return self.out.push(DOLLAR),
            '`' if self.single || literal => return self.out.push(TICK),
            '`' => {
                self.out.push_str(if self.tick { ")" } else { "$(" });
                self.tick = !self.tick;
                return;
            }
            _ => {}
        }
        self.out.push(ch);
    }
}

pub(super) fn neutralize(text: &str) -> String {
    let mut state = Neutralizer {
        single: false,
        double: false,
        tick: false,
        escaped: false,
        comment: false,
        previous: '\n',
        out: String::new(),
    };
    text.chars().for_each(|ch| state.push(ch));
    state.out
}

pub(super) fn restore(script: &str) -> String {
    script.replace(DOLLAR, "$").replace(TICK, "`")
}
