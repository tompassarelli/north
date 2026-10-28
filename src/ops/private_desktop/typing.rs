use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::thread::sleep;
use std::time::Duration;

const SHIFT: u32 = 0xffe1;
const ENTER: u32 = 0xff0d;
const TAB: u32 = 0xff09;
const TIMEOUT: Duration = Duration::from_secs(20);
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
    let mut stream = TcpStream::connect_timeout(
        &SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        TIMEOUT,
    )?;
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.set_write_timeout(Some(TIMEOUT))?;
    handshake(&mut stream)?;
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

fn handshake<S: Read + Write>(stream: &mut S) -> io::Result<()> {
    let mut version = [0u8; 12];
    stream.read_exact(&mut version)?;
    if &version != b"RFB 003.008\n" {
        return Err(io::Error::other("VNC server does not speak RFB 3.8"));
    }
    stream.write_all(b"RFB 003.008\n")?;
    let mut count = [0u8; 1];
    stream.read_exact(&mut count)?;
    let mut types = vec![0u8; usize::from(count[0])];
    stream.read_exact(&mut types)?;
    if !types.contains(&1) {
        return Err(io::Error::other("VNC server requires authentication"));
    }
    stream.write_all(&[1])?;
    let mut result = [0u8; 4];
    stream.read_exact(&mut result)?;
    if result != [0; 4] {
        return Err(io::Error::other("VNC security handshake failed"));
    }
    stream.write_all(&[1])?;
    let mut init = [0u8; 24];
    stream.read_exact(&mut init)?;
    let name_length = u32::from_be_bytes([init[20], init[21], init[22], init[23]]);
    io::copy(&mut stream.take(u64::from(name_length)), &mut io::sink())?;
    Ok(())
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
    use std::net::TcpListener;

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

    #[test]
    fn text_outside_the_us_layout_is_refused_with_its_position() {
        for bad in ["é", "\r", "€", "a\x00"] {
            assert!(plan(bad).is_err(), "{bad:?}");
        }
        assert_eq!(
            plan("ab€").unwrap_err(),
            "character 3 has no US-layout key"
        );
    }

    #[test]
    fn shifted_keys_are_wrapped_in_a_held_shift_key() {
        let received = serve_and_type(&plan("@a").unwrap());
        let events: Vec<(u8, u32)> = received
            .chunks(8)
            .map(|chunk| {
                assert_eq!(chunk[0], 4);
                (chunk[1], u32::from_be_bytes(chunk[4..8].try_into().unwrap()))
            })
            .collect();
        assert_eq!(
            events,
            vec![
                (1, SHIFT),
                (1, 0x40),
                (0, 0x40),
                (0, SHIFT),
                (1, 0x61),
                (0, 0x61)
            ]
        );
    }

    #[test]
    fn a_server_that_demands_authentication_is_refused() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.write_all(b"RFB 003.008\n").unwrap();
            let mut version = [0u8; 12];
            stream.read_exact(&mut version).unwrap();
            stream.write_all(&[1, 2]).unwrap();
        });
        let error = send(port, &plan("a").unwrap()).unwrap_err();
        server.join().unwrap();
        assert!(error.to_string().contains("authentication"));
    }

    fn serve_and_type(keys: &[Key]) -> Vec<u8> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.write_all(b"RFB 003.008\n").unwrap();
            let mut reply = [0u8; 12];
            stream.read_exact(&mut reply).unwrap();
            assert_eq!(&reply, b"RFB 003.008\n");
            stream.write_all(&[1, 1]).unwrap();
            let mut chosen = [0u8; 1];
            stream.read_exact(&mut chosen).unwrap();
            assert_eq!(chosen, [1]);
            stream.write_all(&[0; 4]).unwrap();
            let mut shared = [0u8; 1];
            stream.read_exact(&mut shared).unwrap();
            let mut init = [0u8; 24];
            init[23] = 4;
            stream.write_all(&init).unwrap();
            stream.write_all(b"test").unwrap();
            let mut received = Vec::new();
            stream.read_to_end(&mut received).unwrap();
            received
        });
        send(port, keys).unwrap();
        server.join().unwrap()
    }
}
