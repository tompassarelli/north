use crate::ask_once;
use crate::error::{NorthError, NorthResult};
use crate::gates::GateRequest;

impl GateRequest {
    pub(crate) fn decision_rule(&self) -> NorthResult<()> {
        if let Some(category) = self.ask_once.as_deref().filter(|category| !ask_once::is_category(category)) {
            return Err(NorthError::Usage(format!("--ask-once names no ask-once category ({category}): use one of {}", categories())));
        }
        let class = ask_once::class_of(&self.options[self.recommend - 1]);
        match (self.has_default(), self.ask_once.as_deref(), class) {
            (true, Some(_), _) => Err(NorthError::Usage("a gate that names an ask-once category waits for Tom and takes no --default or --default-after: drop them, or drop --ask-once".into())),
            (true, None, Some(class)) => Err(NorthError::Usage(format!("the recommended option is an ask-once matter ({class}) that only Tom answers: open the gate with --ask-once {class} and without --default and --default-after, and it waits for his answer"))),
            (true, None, None) if self.default_applies() => Ok(()),
            (true, None, None) => Err(NorthError::Usage(format!("--default must name an option from 1 to {} and --default-after must be a duration", self.options.len()))),
            (false, Some(_), _) => Ok(()),
            (false, None, _) => Err(NorthError::Usage(format!("a gate without --default and --default-after must name the ask-once category of its recommended option with --ask-once ({}); otherwise add --default N --default-after DURATION", categories()))),
        }
    }

    fn default_applies(&self) -> bool {
        (1..=self.options.len()).contains(&self.default) && self.default_after_ms > 0
    }
}

fn categories() -> String {
    ask_once::categories().collect::<Vec<_>>().join(", ")
}
