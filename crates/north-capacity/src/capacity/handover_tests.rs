#![cfg(test)]

use super::handover::*;

#[test]
fn capacity_frames_round_trip_and_a_cut_stream_is_an_error_not_an_end() {
    let frames = [Frame::Started("ccx33".to_owned()), Frame::Out(b"ok\n".to_vec()), Frame::Err(b"warn\n".to_vec()), Frame::NotSynced("busy".to_owned()), Frame::Exit(101)];
    let mut wire = Vec::new();
    for frame in &frames {
        frame.write(&mut wire).unwrap();
    }
    let mut source = wire.as_slice();
    let mut read = Vec::new();
    while let Some(frame) = Frame::read(&mut source).unwrap() {
        read.push(frame);
    }
    assert_eq!(read, frames);
    let mut cut = &wire[..wire.len() - 1];
    let outcome = loop {
        match Frame::read(&mut cut) {
            Ok(Some(_)) => {}
            other => break other,
        }
    };
    assert!(outcome.is_err());
    let mut huge = vec![b'o'];
    huge.extend((MAX_FRAME_BYTES as u32 + 1).to_be_bytes());
    assert!(Frame::read(&mut huge.as_slice()).is_err());
}
