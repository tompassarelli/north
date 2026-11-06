#[repr(u8)]
#[derive(Clone, Copy)]
enum Code {
    NoVerdict = 75,
    TransportFailed = 255,
}

pub const NO_VERDICT: u8 = Code::NoVerdict as u8;
pub const NO_VERDICT_STATUS: i32 = Code::NoVerdict as i32;
pub const TRANSPORT_FAILED: u8 = Code::TransportFailed as u8;
pub const TRANSPORT_FAILED_TEXT: &str = "exited 255";
