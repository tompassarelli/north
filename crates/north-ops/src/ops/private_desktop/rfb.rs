use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(20);

pub(super) fn connect(port: u16) -> io::Result<TcpStream> {
    let mut stream =
        TcpStream::connect_timeout(&SocketAddr::from((Ipv4Addr::LOCALHOST, port)), TIMEOUT)?;
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.set_write_timeout(Some(TIMEOUT))?;
    handshake(&mut stream)?;
    Ok(stream)
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

#[cfg(test)]
pub(super) fn accept(listener: &std::net::TcpListener) -> TcpStream {
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
    stream
}
