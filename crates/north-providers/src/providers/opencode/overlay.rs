use crate::platform::harness;

const OVERLAY: &str = "harness/overlays/deepseek.md";
pub const OVERLAY_ARM: &str = "deepseek-overlay";
pub const CONTROL_ARM: &str = "deepseek-control";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Arm {
    Overlay,
    Control,
}

impl Arm {
    pub fn of(session: &str) -> Self {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in session.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100_0000_01b3);
        }
        if hash & 1 == 0 { Self::Overlay } else { Self::Control }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Overlay => OVERLAY_ARM,
            Self::Control => CONTROL_ARM,
        }
    }

    pub fn brief(self, brief: &str) -> String {
        match self {
            Self::Overlay => {
                let overlay = harness::table(OVERLAY).trim();
                match brief.trim() {
                    "" => overlay.to_owned(),
                    brief => format!("{brief}\n\n{overlay}"),
                }
            }
            Self::Control => brief.to_owned(),
        }
    }
}
