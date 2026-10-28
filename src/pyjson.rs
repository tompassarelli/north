use std::collections::HashMap;
use std::ops::{Index, IndexMut};

// CPython 3.13's default JSON scanner allows 9,995 nested containers in a top-level call.
const MAX_DEPTH: usize = 9995;

#[derive(Debug, Clone, Eq)]
pub struct PyString {
    pub codepoints: Vec<u32>,
    text: Option<String>,
}

impl PyString {
    fn decoded(codepoints: Vec<u32>) -> Self {
        let text = codepoints.iter().copied().map(char::from_u32).collect();
        Self {
            codepoints,
            text,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        self.text.as_deref()
    }

    pub fn lossy(&self) -> std::borrow::Cow<'_, str> {
        match &self.text {
            Some(text) => text.into(),
            None => self
                .codepoints
                .iter()
                .map(|point| char::from_u32(*point).unwrap_or('\u{fffd}'))
                .collect::<String>()
                .into(),
        }
    }
}

impl From<&str> for PyString {
    fn from(text: &str) -> Self {
        Self::decoded(text.chars().map(u32::from).collect())
    }
}

impl PartialEq<str> for PyString {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == Some(other)
    }
}

impl PartialEq for PyString {
    fn eq(&self, other: &Self) -> bool {
        self.codepoints == other.codepoints
    }
}

#[derive(Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number { raw: String, output: String },
    String(PyString),
    Array(Vec<Value>),
    Object(Vec<(PyString, Value)>),
}

impl Drop for Value {
    fn drop(&mut self) {
        fn drain(value: &mut Value, pending: &mut Vec<Value>) {
            match value {
                Value::Array(values) => pending.append(values),
                Value::Object(values) => pending.extend(values.drain(..).map(|(_, value)| value)),
                _ => {}
            }
        }
        let mut pending = Vec::new();
        drain(self, &mut pending);
        while let Some(mut value) = pending.pop() {
            drain(&mut value, &mut pending);
        }
    }
}

impl Clone for Value {
    fn clone(&self) -> Self {
        enum Task<'a> {
            Value(&'a Value),
            Array(usize),
            Object(Vec<PyString>),
        }
        let mut tasks = vec![Task::Value(self)];
        let mut output = Vec::new();
        while let Some(task) = tasks.pop() {
            match task {
                Task::Value(value) => match value {
                    Self::Null => output.push(Self::Null),
                    Self::Bool(v) => output.push(Self::Bool(*v)),
                    Self::Number { raw, output: text } => output.push(Self::Number {
                        raw: raw.clone(),
                        output: text.clone(),
                    }),
                    Self::String(v) => output.push(Self::String(v.clone())),
                    Self::Array(values) => {
                        tasks.push(Task::Array(values.len()));
                        tasks.extend(values.iter().rev().map(Task::Value));
                    }
                    Self::Object(values) => {
                        tasks.push(Task::Object(
                            values.iter().map(|(key, _)| key.clone()).collect(),
                        ));
                        tasks.extend(values.iter().rev().map(|(_, value)| Task::Value(value)));
                    }
                },
                Task::Array(len) => {
                    let values = output.split_off(output.len() - len);
                    output.push(Self::Array(values));
                }
                Task::Object(keys) => {
                    let values = output.split_off(output.len() - keys.len());
                    output.push(Self::Object(keys.into_iter().zip(values).collect()));
                }
            }
        }
        output.pop().unwrap()
    }
}

impl Value {
    pub fn object<const N: usize>(entries: [(&str, Value); N]) -> Self {
        Self::Object(
            entries
                .into_iter()
                .map(|(key, value)| (key.into(), value))
                .collect(),
        )
    }

    pub fn get(&self, key: &str) -> Option<&Self> {
        self.as_object()?
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
    }

    pub fn as_object(&self) -> Option<&[(PyString, Value)]> {
        match self {
            Self::Object(values) => Some(values),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Self::Array(values) => Some(values),
            _ => None,
        }
    }

    pub fn as_string(&self) -> Option<&PyString> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        self.as_string()?.as_str()
    }

    pub fn is_object(&self) -> bool {
        matches!(self, Self::Object(_))
    }
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Number { raw, .. } => raw.parse().ok(),
            _ => None,
        }
    }

    pub fn float(value: f64) -> Self {
        let text = float_output(value);
        Self::Number { raw: text.clone(), output: text }
    }
}

impl From<&str> for Value {
    fn from(text: &str) -> Self {
        Self::String(text.into())
    }
}

impl From<String> for Value {
    fn from(text: String) -> Self {
        text.as_str().into()
    }
}

impl PartialEq<str> for Value {
    fn eq(&self, text: &str) -> bool {
        self.as_str() == Some(text)
    }
}

impl PartialEq<&str> for Value {
    fn eq(&self, text: &&str) -> bool {
        self == *text
    }
}

impl Index<&str> for Value {
    type Output = Self;
    fn index(&self, key: &str) -> &Self {
        static NULL: Value = Value::Null;
        self.get(key).unwrap_or(&NULL)
    }
}

impl IndexMut<&str> for Value {
    fn index_mut(&mut self, key: &str) -> &mut Self {
        let Self::Object(values) = self else {
            panic!("expected object")
        };
        let index = values
            .iter()
            .position(|(name, _)| name == key)
            .unwrap_or_else(|| {
                values.push((key.into(), Self::Null));
                values.len() - 1
            });
        &mut values[index].1
    }
}

#[derive(Debug, PartialEq)]
pub struct Error;

fn decode(bytes: &[u8]) -> Result<Vec<u32>, Error> {
    let (width, little, skip) = if bytes.starts_with(&[0, 0, 0xfe, 0xff]) {
        (4, false, 4)
    } else if bytes.starts_with(&[0xff, 0xfe, 0, 0]) {
        (4, true, 4)
    } else if bytes.starts_with(&[0xfe, 0xff]) {
        (2, false, 2)
    } else if bytes.starts_with(&[0xff, 0xfe]) {
        (2, true, 2)
    } else if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        (1, false, 3)
    } else if bytes.len() >= 4 && bytes[..3] == [0, 0, 0] {
        (4, false, 0)
    } else if bytes.len() >= 4 && bytes[1..4] == [0, 0, 0] {
        (4, true, 0)
    } else if bytes.len() >= 2 && bytes[0] == 0 {
        (2, false, 0)
    } else if bytes.len() >= 2 && bytes[1] == 0 {
        (2, true, 0)
    } else {
        (1, false, 0)
    };
    let bytes = &bytes[skip..];
    let mut output = Vec::new();
    if width == 4 {
        if !bytes.len().is_multiple_of(4) {
            return Err(Error);
        }
        for chunk in bytes.chunks_exact(4) {
            let chunk = chunk.try_into().unwrap();
            let point = if little {
                u32::from_le_bytes(chunk)
            } else {
                u32::from_be_bytes(chunk)
            };
            if point > 0x10ffff {
                return Err(Error);
            }
            output.push(point);
        }
    } else if width == 2 {
        if !bytes.len().is_multiple_of(2) {
            return Err(Error);
        }
        let units: Vec<_> = bytes
            .chunks_exact(2)
            .map(|b| {
                if little {
                    u16::from_le_bytes([b[0], b[1]])
                } else {
                    u16::from_be_bytes([b[0], b[1]])
                }
            })
            .collect();
        let mut index = 0;
        while index < units.len() {
            let high = u32::from(units[index]);
            index += 1;
            if (0xd800..=0xdbff).contains(&high)
                && index < units.len()
                && (0xdc00..=0xdfff).contains(&units[index])
            {
                output.push(0x10000 + ((high - 0xd800) << 10) + u32::from(units[index]) - 0xdc00);
                index += 1;
            } else {
                output.push(high);
            }
        }
    } else {
        let mut index = 0;
        while index < bytes.len() {
            let first = bytes[index];
            let (len, mut point, minimum) = match first {
                0..=0x7f => (1, u32::from(first), 0),
                0xc2..=0xdf => (2, u32::from(first & 0x1f), 0x80),
                0xe0..=0xef => (3, u32::from(first & 0xf), 0x800),
                0xf0..=0xf4 => (4, u32::from(first & 7), 0x10000),
                _ => return Err(Error),
            };
            if index + len > bytes.len() {
                return Err(Error);
            }
            for byte in &bytes[index + 1..index + len] {
                if byte & 0xc0 != 0x80 {
                    return Err(Error);
                }
                point = (point << 6) | u32::from(byte & 0x3f);
            }
            if point < minimum || point > 0x10ffff {
                return Err(Error);
            }
            output.push(point);
            index += len;
        }
    }
    Ok(output)
}

struct Reader<'a> {
    input: &'a [u32],
    position: usize,
}

impl Reader<'_> {
    fn peek(&self) -> Option<u32> {
        self.input.get(self.position).copied()
    }
    fn whitespace(&mut self) {
        while matches!(self.peek(), Some(0x20 | 0x09 | 0x0a | 0x0d)) {
            self.position += 1;
        }
    }
    fn take(&mut self, text: &str) -> bool {
        if self.input[self.position..]
            .iter()
            .copied()
            .take(text.len())
            .eq(text.bytes().map(u32::from))
        {
            self.position += text.len();
            true
        } else {
            false
        }
    }
    fn hex(&mut self) -> Result<u32, Error> {
        let mut value = 0;
        for _ in 0..4 {
            let digit = self
                .peek()
                .and_then(char::from_u32)
                .and_then(|c| c.to_digit(16))
                .ok_or(Error)?;
            value = value * 16 + digit;
            self.position += 1;
        }
        Ok(value)
    }
    fn string(&mut self) -> Result<PyString, Error> {
        if self.peek() != Some(34) {
            return Err(Error);
        }
        self.position += 1;
        let mut points = Vec::new();
        loop {
            let point = self.peek().ok_or(Error)?;
            self.position += 1;
            match point {
                34 => {
                    return Ok(PyString::decoded(points));
                }
                0..=31 => return Err(Error),
                92 => {
                    let escape = self.peek().ok_or(Error)?;
                    self.position += 1;
                    points.push(match escape {
                        34 | 47 | 92 => escape,
                        98 => 8,
                        102 => 12,
                        110 => 10,
                        114 => 13,
                        116 => 9,
                        117 => {
                            let high = self.hex()?;
                            if (0xd800..=0xdbff).contains(&high)
                                && self.input.get(self.position..self.position + 2)
                                    == Some(&[92, 117])
                            {
                                let saved = self.position;
                                self.position += 2;
                                if let Ok(low @ 0xdc00..=0xdfff) = self.hex() {
                                    0x10000 + ((high - 0xd800) << 10) + low - 0xdc00
                                } else {
                                    self.position = saved;
                                    high
                                }
                            } else {
                                high
                            }
                        }
                        _ => return Err(Error),
                    });
                }
                _ => points.push(point),
            }
        }
    }
    fn number(&mut self) -> Result<Value, Error> {
        let start = self.position;
        if self.peek() == Some(45) {
            self.position += 1;
        }
        match self.peek() {
            Some(48) => self.position += 1,
            Some(49..=57) => {
                while matches!(self.peek(), Some(48..=57)) {
                    self.position += 1;
                }
            }
            _ => return Err(Error),
        }
        let mut float = false;
        if self.peek() == Some(46) {
            float = true;
            self.position += 1;
            let digits = self.position;
            while matches!(self.peek(), Some(48..=57)) {
                self.position += 1;
            }
            if self.position == digits {
                return Err(Error);
            }
        }
        if matches!(self.peek(), Some(69 | 101)) {
            float = true;
            self.position += 1;
            if matches!(self.peek(), Some(43 | 45)) {
                self.position += 1;
            }
            let digits = self.position;
            while matches!(self.peek(), Some(48..=57)) {
                self.position += 1;
            }
            if self.position == digits {
                return Err(Error);
            }
        }
        let raw: String = self.input[start..self.position]
            .iter()
            .map(|&c| char::from_u32(c).unwrap())
            .collect();
        let output = if float {
            float_output(raw.parse().map_err(|_| Error)?)
        } else {
            if raw.trim_start_matches('-').len() > 4300 {
                return Err(Error);
            }
            if raw == "-0" { "0".into() } else { raw.clone() }
        };
        Ok(Value::Number { raw, output })
    }
}

fn float_output(value: f64) -> String {
    if value.is_nan() {
        return "NaN".into();
    }
    if value.is_infinite() {
        return if value.is_sign_negative() {
            "-Infinity"
        } else {
            "Infinity"
        }
        .into();
    }
    let mut buffer = ryu::Buffer::new();
    let text = buffer.format_finite(value);
    let sign = if value.is_sign_negative() { "-" } else { "" };
    let text = text.trim_start_matches('-');
    let (mantissa, shift) = text
        .split_once('e')
        .map_or((text, 0), |(mantissa, exponent)| {
            (mantissa, exponent.parse::<i32>().unwrap())
        });
    let decimal = mantissa.find('.').unwrap_or(mantissa.len()) as i32;
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let Some(first) = digits.find(|c| c != '0') else {
        return format!("{sign}0.0");
    };
    let exponent = decimal - first as i32 - 1 + shift;
    let digits = digits[first..].trim_end_matches('0');
    if !(-4..16).contains(&exponent) {
        let fraction = if digits.len() > 1 {
            format!(".{}", &digits[1..])
        } else {
            String::new()
        };
        format!("{sign}{}{fraction}e{exponent:+03}", &digits[..1])
    } else {
        let decimal = exponent + 1;
        if decimal <= 0 {
            format!("{sign}0.{}{digits}", "0".repeat((-decimal) as usize))
        } else if decimal as usize >= digits.len() {
            format!(
                "{sign}{digits}{}.0",
                "0".repeat(decimal as usize - digits.len())
            )
        } else {
            format!(
                "{sign}{}.{}",
                &digits[..decimal as usize],
                &digits[decimal as usize..]
            )
        }
    }
}

enum Frame {
    Array(Vec<Value>),
    Object(Vec<(PyString, Value)>, HashMap<Vec<u32>, usize>, PyString),
}

pub fn loads(bytes: &[u8]) -> Result<Value, Error> {
    let input = decode(bytes)?;
    loads_points(&input)
}

pub fn loads_string(text: &PyString) -> Result<Value, Error> {
    loads_points(&text.codepoints)
}

fn loads_points(input: &[u32]) -> Result<Value, Error> {
    let mut reader = Reader { input, position: 0 };
    let mut frames = Vec::new();
    let mut ready = None;
    loop {
        reader.whitespace();
        if ready.is_none() {
            ready = Some(match reader.peek() {
                Some(34) => Value::String(reader.string()?),
                Some(91 | 123) => {
                    let object = reader.peek() == Some(123);
                    reader.position += 1;
                    reader.whitespace();
                    if frames.len() >= MAX_DEPTH {
                        return Err(Error);
                    }
                    if reader.peek() == Some(if object { 125 } else { 93 }) {
                        reader.position += 1;
                        if object {
                            Value::Object(Vec::new())
                        } else {
                            Value::Array(Vec::new())
                        }
                    } else {
                        if object {
                            let key = reader.string()?;
                            reader.whitespace();
                            if !reader.take(":") {
                                return Err(Error);
                            }
                            frames.push(Frame::Object(Vec::new(), HashMap::new(), key));
                        } else {
                            frames.push(Frame::Array(Vec::new()));
                        }
                        continue;
                    }
                }
                Some(116) if reader.take("true") => Value::Bool(true),
                Some(102) if reader.take("false") => Value::Bool(false),
                Some(110) if reader.take("null") => Value::Null,
                Some(78) if reader.take("NaN") => Value::Number {
                    raw: "NaN".into(),
                    output: "NaN".into(),
                },
                Some(73) if reader.take("Infinity") => Value::Number {
                    raw: "Infinity".into(),
                    output: "Infinity".into(),
                },
                Some(45) if reader.take("-Infinity") => Value::Number {
                    raw: "-Infinity".into(),
                    output: "-Infinity".into(),
                },
                Some(45 | 48..=57) => reader.number()?,
                _ => return Err(Error),
            });
            reader.whitespace();
        }
        let value = ready.take().unwrap();
        let Some(frame) = frames.last_mut() else {
            return if reader.position == input.len() {
                Ok(value)
            } else {
                Err(Error)
            };
        };
        let close = match frame {
            Frame::Array(values) => {
                values.push(value);
                93
            }
            Frame::Object(values, indices, key) => {
                if let Some(&index) = indices.get(&key.codepoints) {
                    values[index].1 = value;
                } else {
                    indices.insert(key.codepoints.clone(), values.len());
                    values.push((key.clone(), value));
                }
                125
            }
        };
        if reader.peek() == Some(close) {
            reader.position += 1;
            ready = Some(match frames.pop().unwrap() {
                Frame::Array(values) => Value::Array(values),
                Frame::Object(values, _, _) => Value::Object(values),
            });
        } else if reader.take(",") {
            reader.whitespace();
            if let Frame::Object(_, _, key) = frames.last_mut().unwrap() {
                *key = reader.string()?;
                reader.whitespace();
                if !reader.take(":") {
                    return Err(Error);
                }
            }
        } else {
            return Err(Error);
        }
    }
}

fn write_string(text: &PyString, output: &mut String) {
    use std::fmt::Write;
    output.push('"');
    for &point in &text.codepoints {
        match point {
            34 => output.push_str("\\\""),
            92 => output.push_str("\\\\"),
            8 => output.push_str("\\b"),
            12 => output.push_str("\\f"),
            10 => output.push_str("\\n"),
            13 => output.push_str("\\r"),
            9 => output.push_str("\\t"),
            32..=126 => output.push(char::from_u32(point).unwrap()),
            0..=0xffff => {
                write!(output, "\\u{point:04x}").unwrap();
            }
            _ => {
                let high = 0xd800 + ((point - 0x10000) >> 10);
                let low = 0xdc00 + ((point - 0x10000) & 0x3ff);
                write!(output, "\\u{high:04x}\\u{low:04x}").unwrap();
            }
        }
    }
    output.push('"');
}

pub fn dumps(value: &Value) -> String {
    enum Task<'a> {
        Value(&'a Value),
        String(&'a PyString),
        Text(&'static str),
    }
    let mut tasks = vec![Task::Value(value)];
    let mut output = String::new();
    while let Some(task) = tasks.pop() {
        match task {
            Task::Text(text) => output.push_str(text),
            Task::String(text) => write_string(text, &mut output),
            Task::Value(value) => match value {
                Value::Null => output.push_str("null"),
                Value::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
                Value::Number { output: text, .. } => output.push_str(text),
                Value::String(text) => write_string(text, &mut output),
                Value::Array(values) => {
                    output.push('[');
                    tasks.push(Task::Text("]"));
                    for (index, value) in values.iter().enumerate().rev() {
                        tasks.push(Task::Value(value));
                        if index > 0 {
                            tasks.push(Task::Text(", "));
                        }
                    }
                }
                Value::Object(values) => {
                    output.push('{');
                    tasks.push(Task::Text("}"));
                    for (index, (key, value)) in values.iter().enumerate().rev() {
                        tasks.push(Task::Value(value));
                        tasks.push(Task::Text(": "));
                        tasks.push(Task::String(key));
                        if index > 0 {
                            tasks.push(Task::Text(", "));
                        }
                    }
                }
            },
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Generator(u64);

    impl Generator {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn string(&mut self) -> String {
            use std::fmt::Write;
            let mut output = String::from("\"");
            for _ in 0..self.next() % 12 {
                let point = self.next() as u32 % 0x110000;
                match self.next() % 5 {
                    0 => output.push_str(
                        ["\\\"", "\\\\", "\\/", "\\b", "\\f", "\\n", "\\r", "\\t"]
                            [self.next() as usize % 8],
                    ),
                    1 => {
                        write!(output, "\\u{:04x}", point & 0xffff).unwrap();
                    }
                    2 => output.push_str(
                        [
                            "\\ud800",
                            "\\udfff",
                            "\\ud83d\\ude00",
                            "\\ud800\\ud800",
                            "\\u0000",
                            "\\u007f",
                        ][self.next() as usize % 6],
                    ),
                    _ => {
                        if let Some(character) =
                            char::from_u32(point).filter(|c| *c >= ' ' && !matches!(c, '"' | '\\'))
                        {
                            output.push(character);
                        }
                    }
                }
            }
            output.push('"');
            output
        }

        fn document(&mut self, depth: usize) -> String {
            match self.next() % if depth == 0 { 6 } else { 9 } {
                0 => ["null", "true", "false", "NaN", "Infinity", "-Infinity"]
                    [self.next() as usize % 6]
                    .into(),
                1 => format!(
                    "{}{}",
                    if self.next().is_multiple_of(2) {
                        "-"
                    } else {
                        ""
                    },
                    self.next()
                ),
                2 => format!(
                    "{}{}{}{}",
                    self.next(),
                    self.next(),
                    self.next(),
                    self.next()
                ),
                3 => format!(
                    "{}{}.{}e{}{}",
                    if self.next().is_multiple_of(2) {
                        "-"
                    } else {
                        ""
                    },
                    self.next(),
                    self.next(),
                    if self.next().is_multiple_of(2) {
                        "-"
                    } else {
                        "+"
                    },
                    self.next() % 600
                ),
                4 => format!("{:?}", f64::from_bits(self.next())),
                5 => self.string(),
                6 => {
                    let count = self.next() % 5;
                    let values: Vec<_> = (0..count).map(|_| self.document(depth - 1)).collect();
                    format!("[ {} ]", values.join(" ,\n\t"))
                }
                _ => {
                    let key = self.string();
                    let first = self.document(depth - 1);
                    let second = self.document(depth - 1);
                    format!("{{ {key} : {first}, \"a\": 0, {key}: {second}, \"\\u0061\": 1 }}")
                }
            }
        }
    }

    #[test]
    fn python_json_acceptance_and_dumps_match() {
        let mut documents: Vec<Vec<u8>> = [
            "\"\\ud800\"",
            "\"\\udc00\"",
            "\"\\ud83d\\ude00\"",
            "\"\\ud800\\u0041\"",
            "\"\\ud800\\ud800\"",
            "{\"b\":1,\"a\":2,\"b\":3,\"\\u0061\":4}",
            "18446744073709551617",
            "-0",
            "-0.0",
            "1e23",
            "1e-4",
            "1e-5",
            "1e15",
            "1e16",
            "1e309",
            "-1e309",
            "1e-9999",
            "-1e-9999",
            "2.2250738585072012e-308",
            "5e-324",
            "714906239970683.3",
            "NaN",
            "Infinity",
            "-Infinity",
            "",
            " ",
            "[1,]",
            "{\"a\":1,}",
            "\"\\x00\"",
            "\"\\u000g\"",
            "\"\\ud800\\u000g\"",
            "\"\n\"",
            "\"\\v\"",
            "01",
            "+1",
            ".1",
            "1.",
            "1e",
            "1e+",
            "nan",
            "inf",
            "-NaN",
            "null null",
            "truex",
            "\u{feff}{}",
            "[",
            "{",
            "\"",
            "null\0",
            "{\"\\ud800\\udc00\":1,\"𐀀\":2}",
            "[\"\\b\\f\\n\\r\\t\\/\\\\\\\"\",\"\u{7f}\"]",
        ]
        .into_iter()
        .map(|text| text.as_bytes().to_vec())
        .collect();
        for length in [4300, 4301, 5000] {
            documents.push("9".repeat(length).into_bytes());
            documents.push(format!("-{}", "9".repeat(length)).into_bytes());
            documents.push(format!("{}.0", "9".repeat(length)).into_bytes());
        }
        for depth in [128, 998, 1000, 1500, 9994, 9995, 9996, 10000] {
            documents.push(format!("{}0{}", "[".repeat(depth), "]".repeat(depth)).into_bytes());
            documents
                .push(format!("{}0{}", "{\"a\":".repeat(depth), "}".repeat(depth)).into_bytes());
        }
        for text in ["{\"x\":\"é😀\"}", "true", "{}", "\"\\ud800\""] {
            let units: Vec<_> = text.encode_utf16().collect();
            for little in [false, true] {
                let utf16: Vec<u8> = units
                    .iter()
                    .flat_map(|unit| {
                        if little {
                            unit.to_le_bytes()
                        } else {
                            unit.to_be_bytes()
                        }
                    })
                    .collect();
                let utf32: Vec<u8> = text
                    .chars()
                    .flat_map(|c| {
                        if little {
                            u32::from(c).to_le_bytes()
                        } else {
                            u32::from(c).to_be_bytes()
                        }
                    })
                    .collect();
                documents.push(utf16.clone());
                documents.push(utf32.clone());
                documents.push(
                    [
                        if little {
                            vec![0xff, 0xfe]
                        } else {
                            vec![0xfe, 0xff]
                        },
                        utf16,
                    ]
                    .concat(),
                );
                documents.push(
                    [
                        if little {
                            vec![0xff, 0xfe, 0, 0]
                        } else {
                            vec![0, 0, 0xfe, 0xff]
                        },
                        utf32,
                    ]
                    .concat(),
                );
            }
            documents.push([vec![0xef, 0xbb, 0xbf], text.as_bytes().to_vec()].concat());
        }
        documents.extend([
            vec![b'"', 0xed, 0xa0, 0x80, b'"'],
            vec![b'"', 0xed, 0xa0, 0x80, 0xed, 0xb0, 0x80, b'"'],
            vec![b'"', 0xc0, 0x80, b'"'],
            vec![0xff],
            vec![0, 0, 0x11, 0, 0, 0, 0, 0],
        ]);
        let results: Vec<Option<String>> =
            include_str!("fixtures/pyjson/cpython-3.13.15-dumps.jsonl")
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
        let mut generator = Generator(0xd138_058c_a79b_4661);
        while documents.len() < 256 {
            let mut text = generator.document(5);
            match generator.next() % 8 {
                0 => text.push(','),
                1 => {
                    text.pop();
                }
                2 => {
                    text.insert(0, '\u{b}');
                }
                3 => text = format!(" \t\r\n{text}\r\n"),
                _ => {}
            }
            documents.push(text.into_bytes());
        }
        assert_eq!(results.len(), documents.len());
        let mut accepted = 0;
        for (index, (document, expected)) in documents.iter().zip(results).enumerate() {
            let value = loads(document);
            let actual = value.as_ref().ok().map(dumps);
            assert_eq!(
                actual,
                expected,
                "case {index}: {:?}",
                String::from_utf8_lossy(document)
            );
            if let Ok(value) = value {
                accepted += 1;
                assert_eq!(dumps(&value.clone()), actual.unwrap(), "clone case {index}");
            }
        }
        eprintln!(
            "Python json oracle: {} cases, {accepted} accepted, {} rejected, 0 differences",
            documents.len(),
            documents.len() - accepted
        );
    }
}
