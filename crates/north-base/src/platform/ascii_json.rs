//! JSON text with every character outside printable ASCII written as a `\uXXXX` escape, as Python's `json.dumps` writes it by default.

use serde::Serialize;
use serde_json::ser::Formatter;
use std::io::{self, Write};

/// What goes between array items and between an object key and its value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Separators {
    /// `,` and `:`, as `serde_json::to_string` writes them.
    Compact,
    /// `, ` and `: `, as Python's `json.dumps` writes them.
    Python,
}

/// A serde_json formatter that escapes each character outside U+0020..=U+007E, a character above U+FFFF as its UTF-16 surrogate pair.
#[derive(Debug, Clone, Copy)]
pub struct AsciiFormatter(pub Separators);

impl AsciiFormatter {
    fn separator(self, compact: &'static [u8], python: &'static [u8]) -> &'static [u8] {
        match self.0 {
            Separators::Compact => compact,
            Separators::Python => python,
        }
    }
}

impl Formatter for AsciiFormatter {
    fn begin_array_value<W: ?Sized + Write>(&mut self, writer: &mut W, first: bool) -> io::Result<()> {
        if first { Ok(()) } else { writer.write_all(self.separator(b",", b", ")) }
    }

    fn begin_object_key<W: ?Sized + Write>(&mut self, writer: &mut W, first: bool) -> io::Result<()> {
        if first { Ok(()) } else { writer.write_all(self.separator(b",", b", ")) }
    }

    fn begin_object_value<W: ?Sized + Write>(&mut self, writer: &mut W) -> io::Result<()> {
        writer.write_all(self.separator(b":", b": "))
    }

    fn write_string_fragment<W: ?Sized + Write>(&mut self, writer: &mut W, fragment: &str) -> io::Result<()> {
        let mut start = 0;
        for (at, character) in fragment.char_indices().filter(|(_, character)| !(' '..='~').contains(character)) {
            writer.write_all(&fragment.as_bytes()[start..at])?;
            for unit in character.encode_utf16(&mut [0; 2]) {
                write!(writer, "\\u{unit:04x}")?;
            }
            start = at + character.len_utf8();
        }
        writer.write_all(&fragment.as_bytes()[start..])
    }
}

/// `value` as ASCII-only JSON text with `separators`.
pub fn to_ascii_string<T: ?Sized + Serialize>(value: &T, separators: Separators) -> serde_json::Result<String> {
    let mut text = Vec::new();
    value.serialize(&mut serde_json::Serializer::with_formatter(&mut text, AsciiFormatter(separators)))?;
    Ok(String::from_utf8(text).expect("the formatter writes ASCII alone"))
}
