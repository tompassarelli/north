#[repr(u8)]
#[derive(Clone, Copy)]
enum Code {
    RedMain = 3,
    NoVerdict = 75,
    SlotsBusy = 96,
    AlreadyPlaced = 94,
    TimedOut = 124,
    Cancelled = 130,
    Killed = 137,
    TransportFailed = 255,
}

pub const RED_MAIN: u8 = Code::RedMain as u8;
pub const RED_MAIN_STATUS: i32 = Code::RedMain as i32;
pub const NO_VERDICT: u8 = Code::NoVerdict as u8;
pub const NO_VERDICT_STATUS: i32 = Code::NoVerdict as i32;
pub const SLOTS_BUSY: u8 = Code::SlotsBusy as u8;
pub const SLOTS_BUSY_STATUS: i32 = Code::SlotsBusy as i32;
pub const ALREADY_PLACED: u8 = Code::AlreadyPlaced as u8;
pub const ALREADY_PLACED_STATUS: i32 = Code::AlreadyPlaced as i32;
pub const TIMED_OUT: u8 = Code::TimedOut as u8;
pub const TIMED_OUT_STATUS: i32 = Code::TimedOut as i32;
pub const CANCELLED_STATUS: i32 = Code::Cancelled as i32;
pub const KILLED: u8 = Code::Killed as u8;
pub const KILLED_STATUS: i32 = Code::Killed as i32;
pub const TRANSPORT_FAILED: u8 = Code::TransportFailed as u8;
pub const TRANSPORT_FAILED_STATUS: i32 = Code::TransportFailed as i32;
pub const TRANSPORT_FAILED_TEXT: &str = "exited 255";
