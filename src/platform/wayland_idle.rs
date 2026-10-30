use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

const DISPLAY: u32 = 1;
const REGISTRY: u32 = 2;
const SYNC: u32 = 3;
const SEAT: u32 = 4;
const NOTIFIER: u32 = 5;
const NOTIFICATION: u32 = 6;
const INPUT_IDLE_VERSION: u32 = 2;

pub(crate) struct InputIdle<S> {
    stream: S,
}

impl InputIdle<UnixStream> {
    pub(crate) fn connect(timeout_ms: u32) -> io::Result<Self> {
        let display = std::env::var_os("WAYLAND_DISPLAY").ok_or_else(|| io::Error::other("WAYLAND_DISPLAY is unset"))?;
        let mut path = PathBuf::from(&display);
        if path.is_relative() {
            let runtime = std::env::var_os("XDG_RUNTIME_DIR").ok_or_else(|| io::Error::other("XDG_RUNTIME_DIR is unset"))?;
            path = PathBuf::from(runtime).join(display);
        }
        Self::start(UnixStream::connect(path)?, timeout_ms)
    }
}

impl<S: Read + Write> InputIdle<S> {
    pub(crate) fn start(mut stream: S, timeout_ms: u32) -> io::Result<Self> {
        send(&mut stream, DISPLAY, 1, &Args::new().uint(REGISTRY))?;
        send(&mut stream, DISPLAY, 0, &Args::new().uint(SYNC))?;
        let (mut seat, mut notifier) = (None, None);
        loop {
            let (object, opcode, body) = receive(&mut stream)?;
            match (object, opcode) {
                (DISPLAY, 0) => return Err(display_error(&body)),
                (REGISTRY, 0) => {
                    let mut fields = Fields(&body);
                    let (name, interface, version) = (fields.uint()?, fields.string()?, fields.uint()?);
                    match interface.as_str() {
                        "wl_seat" if seat.is_none() => seat = Some(name),
                        "ext_idle_notifier_v1" if version >= INPUT_IDLE_VERSION => notifier = Some(name),
                        _ => {}
                    }
                }
                (SYNC, 0) => break,
                _ => {}
            }
        }
        let seat = seat.ok_or_else(|| io::Error::other("the compositor offers no wl_seat"))?;
        let notifier = notifier.ok_or_else(|| io::Error::other("the compositor offers no ext_idle_notifier_v1 version 2 input-idle notification"))?;
        send(&mut stream, REGISTRY, 0, &Args::new().uint(seat).string("wl_seat").uint(1).uint(SEAT))?;
        send(&mut stream, REGISTRY, 0, &Args::new().uint(notifier).string("ext_idle_notifier_v1").uint(INPUT_IDLE_VERSION).uint(NOTIFIER))?;
        send(&mut stream, NOTIFIER, 2, &Args::new().uint(NOTIFICATION).uint(timeout_ms).uint(SEAT))?;
        Ok(Self { stream })
    }

    pub(crate) fn next(&mut self) -> io::Result<bool> {
        loop {
            match receive(&mut self.stream)? {
                (DISPLAY, 0, body) => return Err(display_error(&body)),
                (NOTIFICATION, 0, _) => return Ok(true),
                (NOTIFICATION, 1, _) => return Ok(false),
                _ => {}
            }
        }
    }
}

fn display_error(body: &[u8]) -> io::Error {
    let mut fields = Fields(body);
    let detail = (|| Some((fields.uint().ok()?, fields.uint().ok()?, fields.string().ok()?)))();
    match detail {
        Some((object, code, message)) => io::Error::other(format!("wayland error on object {object} code {code}: {message}")),
        None => io::Error::other("wayland error"),
    }
}

struct Args(Vec<u8>);

impl Args {
    fn new() -> Self {
        Self(Vec::new())
    }

    fn uint(mut self, value: u32) -> Self {
        self.0.extend_from_slice(&value.to_ne_bytes());
        self
    }

    fn string(mut self, value: &str) -> Self {
        let length = value.len() as u32 + 1;
        self.0.extend_from_slice(&length.to_ne_bytes());
        self.0.extend_from_slice(value.as_bytes());
        self.0.push(0);
        while !self.0.len().is_multiple_of(4) {
            self.0.push(0);
        }
        self
    }
}

fn send(stream: &mut impl Write, object: u32, opcode: u16, args: &Args) -> io::Result<()> {
    let size = 8 + args.0.len() as u32;
    let mut message = Vec::with_capacity(size as usize);
    message.extend_from_slice(&object.to_ne_bytes());
    message.extend_from_slice(&((size << 16) | u32::from(opcode)).to_ne_bytes());
    message.extend_from_slice(&args.0);
    stream.write_all(&message)
}

fn receive(stream: &mut impl Read) -> io::Result<(u32, u16, Vec<u8>)> {
    let mut header = [0u8; 8];
    stream.read_exact(&mut header)?;
    let object = u32::from_ne_bytes(header[..4].try_into().unwrap());
    let word = u32::from_ne_bytes(header[4..].try_into().unwrap());
    let size = (word >> 16) as usize;
    if size < 8 {
        return Err(io::Error::other("wayland message shorter than its header"));
    }
    let mut body = vec![0u8; size - 8];
    stream.read_exact(&mut body)?;
    Ok((object, (word & 0xffff) as u16, body))
}

struct Fields<'a>(&'a [u8]);

impl Fields<'_> {
    fn uint(&mut self) -> io::Result<u32> {
        let (head, rest) = self.0.split_first_chunk::<4>().ok_or_else(|| io::Error::other("truncated wayland message"))?;
        self.0 = rest;
        Ok(u32::from_ne_bytes(*head))
    }

    fn string(&mut self) -> io::Result<String> {
        let length = self.uint()? as usize;
        let padded = length.div_ceil(4) * 4;
        if length == 0 || padded > self.0.len() {
            return Err(io::Error::other("truncated wayland string"));
        }
        let text = String::from_utf8_lossy(&self.0[..length - 1]).into_owned();
        self.0 = &self.0[padded..];
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(object: u32, opcode: u16, args: Args) -> Vec<u8> {
        let mut out = Vec::new();
        send(&mut out, object, opcode, &args).unwrap();
        out
    }

    #[test]
    fn subscribes_to_input_idle_on_the_v2_notifier_and_reports_idled_then_resumed() {
        let (client, mut compositor) = UnixStream::pair().unwrap();
        let server = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for _ in 0..2 {
                requests.push(receive(&mut compositor).unwrap());
            }
            let mut script = Vec::new();
            script.extend(event(REGISTRY, 0, Args::new().uint(7).string("ext_idle_notifier_v1").uint(1)));
            script.extend(event(REGISTRY, 0, Args::new().uint(9).string("wl_seat").uint(9)));
            script.extend(event(REGISTRY, 0, Args::new().uint(15).string("ext_idle_notifier_v1").uint(2)));
            script.extend(event(SYNC, 0, Args::new().uint(0)));
            compositor.write_all(&script).unwrap();
            for _ in 0..3 {
                requests.push(receive(&mut compositor).unwrap());
            }
            let mut script = event(NOTIFICATION, 0, Args::new());
            script.extend(event(SEAT, 1, Args::new().string("seat0")));
            script.extend(event(NOTIFICATION, 1, Args::new()));
            compositor.write_all(&script).unwrap();
            requests
        });
        let mut idle = InputIdle::start(client, 300_000).unwrap();
        assert_eq!((idle.next().unwrap(), idle.next().unwrap()), (true, false));
        let requests = server.join().unwrap();
        assert_eq!(requests[0], (DISPLAY, 1, Args::new().uint(REGISTRY).0));
        assert_eq!(requests[2], (REGISTRY, 0, Args::new().uint(9).string("wl_seat").uint(1).uint(SEAT).0));
        assert_eq!(requests[3], (REGISTRY, 0, Args::new().uint(15).string("ext_idle_notifier_v1").uint(2).uint(NOTIFIER).0));
        assert_eq!(requests[4], (NOTIFIER, 2, Args::new().uint(NOTIFICATION).uint(300_000).uint(SEAT).0));
    }

    #[test]
    fn refuses_a_compositor_without_input_idle() {
        let (client, mut compositor) = UnixStream::pair().unwrap();
        let mut script = event(REGISTRY, 0, Args::new().uint(9).string("wl_seat").uint(9));
        script.extend(event(REGISTRY, 0, Args::new().uint(7).string("ext_idle_notifier_v1").uint(1)));
        script.extend(event(SYNC, 0, Args::new().uint(0)));
        compositor.write_all(&script).unwrap();
        let error = InputIdle::start(client, 1000).err().unwrap();
        assert!(error.to_string().contains("version 2"), "{error}");
    }
}
