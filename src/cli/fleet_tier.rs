use crate::error::{NorthError, NorthResult};
use crate::fleet::model::{Effort, LaunchRequest};
use crate::ops::dispatch::{Ask, Start, start_hand, tier_reason, untasked};

use super::fleet_placement::{self, Overrides};

pub(super) fn apply(request: &mut LaunchRequest, overrides: &mut Overrides) -> NorthResult<Option<Start>> {
    if request.provider.label() != "claude" || request.verification {
        return Ok(None);
    }
    let mut brief = fleet_placement::brief_text(request.system_prompt.as_deref(), &request.prompt)?;
    brief.extend(request.named_brief());
    let reason = tier_reason(&brief, overrides.tier.as_deref());
    let header = header(request)?;
    let (item, brief) = match request.task() {
        Some(item) => (item, brief),
        None if untasked(&header, request.model.as_deref(), reason.as_deref()).map_err(NorthError::Refused)? => (request.name.clone(), header),
        None => {
            overrides.tier = reason;
            return Ok(None);
        }
    };
    let ask = Ask { item: &item, brief: &brief, prompt: &request.prompt, model: request.model.as_deref(), reason: reason.as_deref() };
    let start = start_hand(&ask).map_err(NorthError::Refused)?;
    request.model = Some(start.model.clone());
    if request.effort.is_none() {
        request.effort = start.effort.as_deref().map(Effort::parse).transpose()?;
    }
    request.prompt.clone_from(&start.prompt);
    overrides.tier.clone_from(&start.overridden);
    let model = request.model.as_deref().unwrap_or_default();
    match &start.overridden {
        Some(reason) => eprintln!("tier: {item} runs {model}, not the pick {} ({reason})", start.route.tier.name()),
        None => eprintln!("tier: {item} runs {} on {model} ({})", start.route.tier.name(), start.route.why),
    }
    Ok(Some(start))
}

pub(super) fn admit_class(request: &LaunchRequest) -> NorthResult<()> {
    crate::router::brief::class_refusal(&header(request)?).map_or(Ok(()), |why| Err(NorthError::Refused(format!("brief refused: {why}"))))
}

pub(super) fn recorded(start: Option<Start>, request: &LaunchRequest, name: &str) {
    let Some(start) = start else { return };
    let host = request.machine.as_ref().map_or_else(crate::platform::host::name, |machine| machine.name.clone());
    if let Err(why) = start.record(&host, &format!("{}:{}", request.provider.label(), request.account), name) {
        eprintln!("north fleet: tier not recorded: {why}");
    }
}

fn header(request: &LaunchRequest) -> NorthResult<String> {
    Ok(headers(&[&fleet_placement::brief_text(request.system_prompt.as_deref(), "")?, &request.prompt, &request.named_brief().unwrap_or_default()]))
}

fn headers(texts: &[&str]) -> String {
    let blocks = texts.iter().map(|text| text.trim_start().lines().take_while(|line| !line.trim().is_empty()).collect::<Vec<_>>().join("\n"));
    blocks.collect::<Vec<_>>().join("\n")
}
