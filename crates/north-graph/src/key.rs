use crate::term::{Atom, Float, Instant, TermError};

const STRING: u8 = 1;
const INT: u8 = 2;
const FLOAT: u8 = 3;
const BOOL: u8 = 4;
const KEYWORD: u8 = 5;
const INSTANT: u8 = 6;
const TRIPLE: u8 = 7;

#[derive(Debug, Eq, PartialEq)]
pub enum Key {
    Atom(Atom),
    Triple([i64; 3]),
}

pub fn atom_key(atom: &Atom) -> Vec<u8> {
    match atom {
        Atom::String(text) => tagged(STRING, text.as_bytes()),
        Atom::Int(value) => tagged(INT, &value.to_be_bytes()),
        Atom::Float(value) => tagged(FLOAT, &value.bits().to_be_bytes()),
        Atom::Bool(value) => tagged(BOOL, &[u8::from(*value)]),
        Atom::Keyword(name) => tagged(KEYWORD, name.as_bytes()),
        Atom::Instant(instant) => {
            let mut payload = instant.seconds().to_be_bytes().to_vec();
            payload.extend_from_slice(&instant.nanos().to_be_bytes());
            tagged(INSTANT, &payload)
        }
    }
}

pub fn triple_key(children: [i64; 3]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(24);
    for child in children {
        payload.extend_from_slice(&child.to_be_bytes());
    }
    tagged(TRIPLE, &payload)
}

fn tagged(tag: u8, payload: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(payload.len() + 1);
    key.push(tag);
    key.extend_from_slice(payload);
    key
}

pub fn decode_key(key: &[u8]) -> Result<Key, TermError> {
    let bad = |why: &str| TermError::InvalidKey(why.to_string());
    let (tag, payload) = key.split_first().ok_or_else(|| bad("empty key"))?;
    match *tag {
        STRING => Ok(Key::Atom(Atom::String(text(payload)?))),
        KEYWORD => Ok(Key::Atom(Atom::Keyword(text(payload)?))),
        INT => Ok(Key::Atom(Atom::Int(i64::from_be_bytes(array(payload)?)))),
        FLOAT => {
            let bits = u64::from_be_bytes(array(payload)?);
            let float = Float::from_bits(bits);
            if float.bits() != bits {
                return Err(bad("float bits are not canonical"));
            }
            Ok(Key::Atom(Atom::Float(float)))
        }
        BOOL => match payload {
            [0] => Ok(Key::Atom(Atom::Bool(false))),
            [1] => Ok(Key::Atom(Atom::Bool(true))),
            _ => Err(bad("bool payload")),
        },
        INSTANT => {
            let bytes: [u8; 12] = array(payload)?;
            let seconds = i64::from_be_bytes(array(&bytes[..8])?);
            let nanos = u32::from_be_bytes(array(&bytes[8..])?);
            Ok(Key::Atom(Atom::Instant(Instant::new(seconds, nanos)?)))
        }
        TRIPLE => {
            let bytes: [u8; 24] = array(payload)?;
            let child = |index: usize| array(&bytes[index * 8..index * 8 + 8]).map(i64::from_be_bytes);
            Ok(Key::Triple([child(0)?, child(1)?, child(2)?]))
        }
        _ => Err(bad("unknown tag")),
    }
}

fn text(payload: &[u8]) -> Result<String, TermError> {
    String::from_utf8(payload.to_vec()).map_err(|_| TermError::InvalidKey("text is not UTF-8".to_string()))
}

fn array<const N: usize>(payload: &[u8]) -> Result<[u8; N], TermError> {
    payload.try_into().map_err(|_| TermError::InvalidKey(format!("payload is {} bytes, expected {N}", payload.len())))
}
