use std::cell::RefCell;

use super::verb::Outside;

/// Records each gh call and answers it with `reply`; refuses text holding `secret`.
#[derive(Default)]
pub(crate) struct Recorded {
    pub calls: RefCell<Vec<Vec<String>>>,
    pub secret: Option<&'static str>,
    pub reply: &'static str,
}

impl Outside for Recorded {
    fn scan(&self, text: &str) -> Result<(), String> {
        match self.secret {
            Some(secret) if text.contains(secret) => Err("gitleaks found a secret in the item text; credentials stay in Bitwarden, so nothing was stored".into()),
            _ => Ok(()),
        }
    }

    fn gh(&self, args: &[String]) -> Result<String, String> {
        self.calls.borrow_mut().push(args.to_vec());
        Ok(self.reply.to_owned())
    }
}
