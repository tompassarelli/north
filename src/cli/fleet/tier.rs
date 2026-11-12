use crate::error::{NorthError, NorthResult};
use crate::fleet::model::{Effort, LaunchRequest};
use crate::dispatch::{Ask, Start, Tier, draw_untasked_window, start_hand, tier_reason, untasked};
use crate::fleet::route::{NOTE, Route};

use super::placement::{self, Overrides};

pub(super) fn apply(request: &mut LaunchRequest, overrides: &mut Overrides) -> NorthResult<Option<Start>> {
    if request.provider.label() != "claude" && !request.verification && request.role() == crate::fleet::model::Role::Worker
        && let Some(item) = request.task()
    {
        match crate::dispatch::boxless_launch_refusal(&item) {
            Ok(Some(why)) => return Err(NorthError::Refused(why)),
            Ok(None) => {}
            Err(why) => eprintln!("north fleet: {item} unread, so its Done-when box is not checked: {why}"),
        }
    }
    if request.provider.label() != "claude" {
        return Ok(None);
    }
    let routed = Routed::of(request)?;
    if request.verification {
        if let Some(routed) = routed {
            routed.pin(request)?;
        }
        return Ok(None);
    }
    let mut brief = placement::brief_text(request.system_prompt.as_deref(), &request.prompt)?;
    brief.extend(request.named_brief());
    let reason = tier_reason(&brief, overrides.tier.as_deref());
    let header = header(request)?;
    let (item, brief) = match request.task() {
        Some(item) => (item, brief),
        None if untasked(&header, request.model.as_deref(), reason.as_deref()).map_err(NorthError::Refused)? => (request.name.clone(), header),
        None => {
            overrides.tier = reason;
            if request.window().is_none() {
                request.compact_window = draw_untasked_window(&request.name, &header);
            }
            return Ok(None);
        }
    };
    let worker = request.role() == crate::fleet::model::Role::Worker;
    let ask = Ask { item: &item, brief: &brief, prompt: &request.prompt, model: request.model.as_deref(), reason: reason.as_deref(), worker, places: true };
    let start = begin(ask, routed.as_ref())?;
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

struct Routed {
    model: String,
    effort: String,
    pinned: bool,
    first: bool,
    why: String,
}

impl Routed {
    fn of(request: &LaunchRequest) -> NorthResult<Option<Self>> {
        let Some(route) = request.route.as_deref().and_then(Route::parse) else { return Ok(None) };
        if request.task().is_none() && !request.verification {
            return Err(NorthError::Refused("--route starts a box, so the launch names its issue (--task or a first `Item:` line)".to_owned()));
        }
        let rung = route.rung();
        let tier = [Tier::Haiku, Tier::Sonnet, Tier::Opus].into_iter().find(|tier| tier.serves(rung.family)).ok_or_else(|| NorthError::Refused(format!("no tier serves {}", rung.family)))?;
        let (model, effort) = tier.model_effort(&crate::pacing::placement::config().map_err(NorthError::Refused)?).map_err(NorthError::Refused)?;
        let (first, why) = match &route {
            Route::First(_, second) => (true, format!("the route's first rung, {} after a failed check", second.word())),
            Route::Second { after, .. } => (false, format!("the route's second rung after {after}'s attempt stopped short")),
        };
        Ok(Some(Self { model, effort: rung.effort.map_or(effort, |effort| effort.label().to_owned()), pinned: rung.effort.is_some(), first, why }))
    }

    fn pin(&self, request: &mut LaunchRequest) -> NorthResult<()> {
        request.model = Some(self.model.clone());
        request.effort = Some(Effort::parse(&self.effort)?);
        if self.first {
            request.prompt.push_str(NOTE);
        }
        Ok(())
    }
}

fn begin(ask: Ask, routed: Option<&Routed>) -> NorthResult<Start> {
    let Some(routed) = routed else { return start_hand(&ask).map_err(NorthError::Refused) };
    let mut start = start_hand(&Ask { model: Some(&routed.model), reason: Some(&routed.why), ..ask }).map_err(NorthError::Refused)?;
    if routed.pinned || start.effort.is_none() {
        start.effort = Some(routed.effort.clone());
    }
    if routed.first {
        start.prompt.push_str(NOTE);
    }
    Ok(start)
}

pub(super) fn admit_class(request: &LaunchRequest) -> NorthResult<()> {
    let header = header(request)?;
    let refusal = crate::router::brief::class_refusal(&header).or_else(|| crate::platform::signin::brief_refusal(&header));
    refusal.map_or(Ok(()), |why| Err(NorthError::Refused(format!("brief refused: {why}"))))
}

pub(super) fn recorded(start: Option<Start>, request: &LaunchRequest, name: &str) {
    let Some(start) = start else { return };
    let host = request.machine.as_ref().map_or_else(crate::platform::host::name, |machine| machine.name.clone());
    if let Err(why) = start.record(&host, &format!("{}:{}", request.provider.label(), request.account), name, request.route.as_deref()) {
        eprintln!("north fleet: tier not recorded: {why}");
    }
}

fn header(request: &LaunchRequest) -> NorthResult<String> {
    Ok(headers(&[&placement::brief_text(request.system_prompt.as_deref(), "")?, &request.prompt, &request.named_brief().unwrap_or_default()]))
}

fn headers(texts: &[&str]) -> String {
    let blocks = texts.iter().map(|text| text.trim_start().lines().take_while(|line| !line.trim().is_empty()).collect::<Vec<_>>().join("\n"));
    blocks.collect::<Vec<_>>().join("\n")
}
