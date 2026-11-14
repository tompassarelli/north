use std::io::{self, Write};
use std::thread::sleep;
use std::time::Duration;

use super::rfb;

const SHIFT: u32 = 0xffe1;
const ENTER: u32 = 0xff0d;
const TAB: u32 = 0xff09;
const KEY_GAP: Duration = Duration::from_millis(80);
const PLAIN: &str = r" `-=[]\;',./";
const SHIFTED: &str = "~!@#$%^&*()_+{}|:\"<>?";

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Key {
    pub(super) shift: bool,
    pub(super) keysym: u32,
}

pub(super) fn plan(text: &str) -> Result<Vec<Key>, String> {
    text.chars()
        .enumerate()
        .map(|(position, character)| {
            key_for(character)
                .ok_or_else(|| format!("character {} has no US-layout key", position + 1))
        })
        .collect()
}

fn key_for(character: char) -> Option<Key> {
    let plain = |keysym| Some(Key { shift: false, keysym });
    match character {
        '\n' => plain(ENTER),
        '\t' => plain(TAB),
        _ if character.is_ascii_lowercase() || character.is_ascii_digit() => {
            plain(u32::from(character))
        }
        _ if PLAIN.contains(character) => plain(u32::from(character)),
        _ if character.is_ascii_uppercase() || SHIFTED.contains(character) => Some(Key {
            shift: true,
            keysym: u32::from(character),
        }),
        _ => None,
    }
}

pub(super) fn send(port: u16, keys: &[Key]) -> io::Result<()> {
    let mut stream = rfb::connect(port)?;
    for key in keys {
        if key.shift {
            key_event(&mut stream, true, SHIFT)?;
        }
        key_event(&mut stream, true, key.keysym)?;
        key_event(&mut stream, false, key.keysym)?;
        if key.shift {
            key_event(&mut stream, false, SHIFT)?;
        }
        sleep(KEY_GAP);
    }
    stream.shutdown(std::net::Shutdown::Write)
}

fn key_event<W: Write>(stream: &mut W, down: bool, keysym: u32) -> io::Result<()> {
    let mut message = [0u8; 8];
    message[0] = 4;
    message[1] = u8::from(down);
    message[4..].copy_from_slice(&keysym.to_be_bytes());
    stream.write_all(&message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expected(character: char) -> Key {
        let shift = character.is_ascii_uppercase() || SHIFTED.contains(character);
        let keysym = match character {
            '\n' => ENTER,
            '\t' => TAB,
            _ => u32::from(character),
        };
        Key { shift, keysym }
    }

    #[test]
    fn every_printable_us_character_maps_to_its_key() {
        let printable: Vec<char> = (0x20u8..0x7f).map(char::from).chain(['\n', '\t']).collect();
        assert_eq!(printable.len(), 97);
        for character in printable {
            assert_eq!(
                plan(&character.to_string()),
                Ok(vec![expected(character)]),
                "{character:?}"
            );
        }
        assert_eq!(
            plan("Ab@#1x.Z_-+").unwrap(),
            "Ab@#1x.Z_-+".chars().map(expected).collect::<Vec<_>>()
        );
    }

}
