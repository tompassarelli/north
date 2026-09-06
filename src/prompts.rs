use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::clause_state::{ChatEntry, Prompt, PromptQuestion};
use crate::error::{NorthError, NorthResult};

pub struct IncomingPrompt {
    pub request: String,
    pub conversation: String,
    pub turn: String,
    pub kind: String,
    pub cancellation: String,
    pub questions: Vec<IncomingQuestion>,
}

pub struct IncomingQuestion {
    pub codec: String,
    pub multiple: bool,
    pub optional: bool,
    pub key: String,
    pub header: String,
    pub text: String,
    pub secret: bool,
    pub other: bool,
    pub options: Vec<IncomingOption>,
}

pub struct IncomingOption {
    pub label: String,
    pub description: String,
    pub value: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserQuestion {
    id: String,
    header: String,
    question: String,
    #[serde(default)]
    is_secret: bool,
    #[serde(default)]
    is_other: bool,
    options: Option<Vec<UserOption>>,
}

#[derive(Deserialize)]
struct UserOption {
    label: String,
    description: String,
}

pub fn decode_request(message: &Value, chat: &[ChatEntry]) -> NorthResult<Option<IncomingPrompt>> {
    let Some(id) = message.get("id") else { return Ok(None); };
    let Some(method) = message["method"].as_str() else { return Ok(None); };
    if !matches!(method, "item/tool/requestUserInput" | "item/commandExecution/requestApproval"
        | "item/fileChange/requestApproval" | "item/permissions/requestApproval" | "mcpServer/elicitation/request")
    { return Ok(None); }
    if !(id.is_string() || id.is_i64() || id.is_u64()) {
        return Err(NorthError::Protocol("Server request has an invalid ID".into()));
    }
    let params = &message["params"];
    let conversation = required_text(params, "threadId")?.to_owned();
    let turn = params["turnId"].as_str().unwrap_or_default().to_owned();
    let mut request = IncomingPrompt {
        request: serde_json::to_string(id)?, conversation, turn,
        kind: "decision".into(), cancellation: String::new(), questions: Vec::new(),
    };
    if method == "item/tool/requestUserInput" {
        request.kind = "questions".into();
        request.cancellation = json!({"answers":{}}).to_string();
        let questions: Vec<UserQuestion> = serde_json::from_value(params["questions"].clone())?;
        for question in questions {
            if request.questions.iter().any(|prior| prior.key == question.id) {
                return Err(NorthError::Protocol("Question IDs must be distinct".into()));
            }
            request.questions.push(IncomingQuestion {
                codec: String::new(), multiple: false, optional: false,
                key: question.id, header: question.header, text: question.question,
                secret: question.is_secret, other: question.is_other,
                options: question.options.unwrap_or_default().into_iter().map(|option| IncomingOption {
                    value: option.label.clone(), label: option.label, description: option.description,
                }).collect(),
            });
        }
        if request.questions.is_empty() {
            return Err(NorthError::Protocol("Question request is empty".into()));
        }
        return Ok(Some(request));
    }
    if method == "mcpServer/elicitation/request" && params["mode"] == "form" {
        decode_form(params, &mut request)?;
        return Ok(Some(request));
    }
    let mut detail = Vec::new();
    if let Some(reason) = params["reason"].as_str() { detail.push(reason.to_owned()); }
    let header;
    let options;
    match method {
        "item/commandExecution/requestApproval" => {
            if let Some(network) = params.get("networkApprovalContext").filter(|value| !value.is_null()) {
                header = "Allow network access?";
                detail.push(format!("Destination: {} ({})", required_text(network, "host")?, required_text(network, "protocol")?));
            } else {
                header = if params["kind"] == "writeStdin" { "Allow terminal input?" } else { "Allow this command?" };
                if let Some(command) = params["command"].as_str() { detail.push(command.to_owned()); }
            }
            if let Some(cwd) = params["cwd"].as_str() { detail.push(format!("Directory: {cwd}")); }
            if let Some(permissions) = params.get("additionalPermissions").filter(|value| !value.is_null()) {
                detail.push(format!("Requested access:\n{}", serde_json::to_string_pretty(permissions)?));
            }
            let decisions = params["availableDecisions"].as_array().cloned()
                .unwrap_or_else(|| vec![json!("accept"), json!("acceptForSession"), json!("decline"), json!("cancel")]);
            if decisions.iter().any(|decision| decision == "cancel") {
                request.cancellation = json!({"decision":"cancel"}).to_string();
            }
            options = decisions.iter().map(approval_option).collect::<NorthResult<Vec<_>>>()?;
        }
        "item/fileChange/requestApproval" => {
            header = "Allow these file changes?";
            if let Some(root) = params["grantRoot"].as_str() { detail.push(format!("Requested write access: {root}")); }
            if let Some(item) = chat.iter().find(|item| item.conversation == request.conversation
                && item.turn == request.turn && Some(item.key.as_str()) == params["itemId"].as_str())
            { detail.push(item.text.clone()); }
            request.cancellation = json!({"decision":"cancel"}).to_string();
            options = ["accept", "acceptForSession", "decline", "cancel"].iter()
                .map(|decision| approval_option(&json!(decision))).collect::<NorthResult<Vec<_>>>()?;
        }
        "item/permissions/requestApproval" => {
            header = "Allow additional access?";
            let permissions = params.get("permissions").filter(|value| value.is_object())
                .ok_or_else(|| NorthError::Protocol("Permission request omitted permissions".into()))?;
            detail.push(format!("Requested access:\n{}", serde_json::to_string_pretty(permissions)?));
            request.cancellation = json!({"permissions":{},"scope":"turn"}).to_string();
            options = vec![
                decision_option("Allow for this turn", "Grant only the access shown above for the current turn.", json!({"permissions":permissions,"scope":"turn"})),
                decision_option("Allow for this session", "Grant the access shown above for subsequent turns in this session.", json!({"permissions":permissions,"scope":"session"})),
                decision_option("Deny", "Do not grant any additional access.", json!({"permissions":{},"scope":"turn"})),
            ];
        }
        "mcpServer/elicitation/request" if params["mode"] == "url" => {
            header = "Complete the requested step";
            detail.push(required_text(params, "message")?.into());
            detail.push(required_text(params, "url")?.into());
            request.cancellation = json!({"action":"cancel","content":null}).to_string();
            options = vec![
                decision_option("I completed this step", "Confirm only after completing the step at the address above.", json!({"action":"accept","content":null})),
                decision_option("Decline", "Do not continue this request.", json!({"action":"decline","content":null})),
                decision_option("Cancel", "Cancel this request.", json!({"action":"cancel","content":null})),
            ];
        }
        _ => return Err(NorthError::Protocol("This server form type is not supported yet".into())),
    }
    if options.is_empty() { return Err(NorthError::Protocol("Approval request offers no decisions".into())); }
    request.questions.push(IncomingQuestion {
        codec: String::new(), multiple: false, optional: false,
        key: "decision".into(), header: header.into(), text: detail.join("\n\n"),
        secret: false, other: false, options,
    });
    Ok(Some(request))
}

fn decode_form(params: &Value, request: &mut IncomingPrompt) -> NorthResult<()> {
    let schema = &params["requestedSchema"];
    if schema["type"] != "object" {
        return Err(NorthError::Protocol("Tool form must request an object".into()));
    }
    let properties = schema["properties"].as_object()
        .ok_or_else(|| NorthError::Protocol("Tool form omitted its fields".into()))?;
    let message = required_text(params, "message")?;
    let server = required_text(params, "serverName")?;
    request.kind = "form".into();
    request.cancellation = json!({"action":"cancel","content":null}).to_string();
    for (key, field) in properties {
        let optional = !schema["required"].as_array().is_some_and(|required| required.iter().any(|value| value == key));
        let kind = required_text(field, "type")?;
        let multiple = kind == "array";
        let options = if multiple {
            enum_options(&field["items"], "anyOf")?.ok_or_else(|| NorthError::Protocol("Tool form array omitted its choices".into()))?
        } else if kind == "boolean" {
            vec![form_option("Yes", json!(true)), form_option("No", json!(false))]
        } else if kind == "string" {
            enum_options(field, "oneOf")?.unwrap_or_default()
        } else if matches!(kind, "number" | "integer") {
            Vec::new()
        } else {
            return Err(NorthError::Protocol(format!("Unsupported tool form field type: {kind}")));
        };
        let mut detail = format!("{server}\n\n{message}");
        if let Some(description) = field["description"].as_str() { detail.push_str(&format!("\n\n{description}")); }
        if kind == "number" || kind == "integer" {
            detail.push_str(if kind == "integer" { "\n\nEnter a whole number." } else { "\n\nEnter a number." });
        }
        if let Some(format) = field["format"].as_str() { detail.push_str(&format!("\n\nExpected format: {format}")); }
        for (name, label) in [("minimum", "Minimum"), ("maximum", "Maximum"), ("minLength", "Minimum characters"),
            ("maxLength", "Maximum characters"), ("minItems", "Minimum choices"), ("maxItems", "Maximum choices")]
        {
            if let Some(value) = field.get(name).filter(|value| !value.is_null()) { detail.push_str(&format!("\n{label}: {value}")); }
        }
        if let Some(default) = field.get("default").filter(|value| !value.is_null()) {
            detail.push_str(&format!("\nSuggested answer: {default}"));
        }
        request.questions.push(IncomingQuestion {
            key: key.clone(), header: field["title"].as_str().unwrap_or(key).into(), text: detail,
            secret: false, other: false, options, codec: field.to_string(), multiple, optional,
        });
    }
    request.questions.push(IncomingQuestion {
        key: String::new(), header: "Send these answers?".into(), text: format!("Send your answers to {server}."),
        secret: false, other: false, codec: String::new(), multiple: false, optional: false,
        options: vec![
            decision_option("Send answers", "Share the answers you entered with this tool.", json!("accept")),
            decision_option("Decline", "Do not share any answers.", json!("decline")),
        ],
    });
    Ok(())
}

fn form_option(label: &str, value: Value) -> IncomingOption {
    IncomingOption { label: label.into(), description: String::new(), value: value.to_string() }
}

fn enum_options(schema: &Value, variants: &str) -> NorthResult<Option<Vec<IncomingOption>>> {
    if let Some(values) = schema["enum"].as_array() {
        return values.iter().enumerate().map(|(index, value)| {
            let text = value.as_str().ok_or_else(|| NorthError::Protocol("Tool form choice must be text".into()))?;
            let label = schema["enumNames"].get(index).and_then(Value::as_str).unwrap_or(text);
            Ok(form_option(label, value.clone()))
        }).collect::<NorthResult<Vec<_>>>().map(Some);
    }
    schema[variants].as_array().map(|values| values.iter().map(|option| {
        let value = required_text(option, "const")?;
        Ok(form_option(required_text(option, "title")?, json!(value)))
    }).collect::<NorthResult<Vec<_>>>()).transpose()
}

/// Validate and encode a field at the foreign schema boundary. Selection and
/// answer progression remain in the source-owned prompt state.
pub fn text_answer(question: &PromptQuestion, text: &str) -> Result<String, String> {
    if question.codec.is_empty() { return Ok(text.into()); }
    let schema: Value = serde_json::from_str(&question.codec).map_err(|_| "This question could not be read.".to_owned())?;
    let value = match schema["type"].as_str() {
        Some("string") => json!(text),
        Some("number" | "integer") => serde_json::from_str::<Value>(text.trim())
            .ok().filter(Value::is_number).ok_or_else(|| "Enter a number.".to_owned())?,
        _ => return Err("Choose an answer from the list.".into()),
    };
    validate_form_value(&schema, &value)?;
    Ok(value.to_string())
}

pub fn multiple_answer(question: &PromptQuestion) -> Result<String, String> {
    let values = question.options.iter().filter(|option| option.checked)
        .map(|option| serde_json::from_str::<Value>(&option.value)).collect::<Result<Vec<_>, _>>()
        .map_err(|_| "These choices could not be read.".to_owned())?;
    let schema: Value = serde_json::from_str(&question.codec).map_err(|_| "This question could not be read.".to_owned())?;
    let value = Value::Array(values);
    validate_form_value(&schema, &value)?;
    Ok(value.to_string())
}

fn validate_form_value(schema: &Value, value: &Value) -> Result<(), String> {
    if let Some(number) = value.as_f64() {
        if schema["type"] == "integer" && number.fract() != 0.0 { return Err("Enter a whole number.".into()); }
        if let Some(minimum) = schema["minimum"].as_f64() && number < minimum { return Err(format!("Enter at least {minimum}.")); }
        if let Some(maximum) = schema["maximum"].as_f64() && number > maximum { return Err(format!("Enter no more than {maximum}.")); }
    }
    let size = value.as_str().map(|text| (text.chars().count() as u64, "minLength", "maxLength", "characters"))
        .or_else(|| value.as_array().map(|items| (items.len() as u64, "minItems", "maxItems", "choices")));
    if let Some((size, minimum, maximum, unit)) = size {
        if let Some(bound) = schema[minimum].as_u64() && size < bound { return Err(format!("Provide at least {bound} {unit}.")); }
        if let Some(bound) = schema[maximum].as_u64() && size > bound { return Err(format!("Provide no more than {bound} {unit}.")); }
    }
    Ok(())
}

fn required_text<'a>(value: &'a Value, key: &str) -> NorthResult<&'a str> {
    value[key].as_str().ok_or_else(|| NorthError::Protocol(format!("Server request omitted {key}")))
}

fn decision_option(label: &str, description: &str, result: Value) -> IncomingOption {
    IncomingOption { label: label.into(), description: description.into(), value: result.to_string() }
}

fn approval_option(decision: &Value) -> NorthResult<IncomingOption> {
    let (label, description): (String, String) = match decision.as_str() {
        Some("accept") => ("Allow once".into(), "Allow the requested action.".into()),
        Some("acceptForSession") => ("Allow for this session".into(), "Also allow matching requests for the rest of this session.".into()),
        Some("decline") => ("Deny".into(), "Decline this action and let the agent continue.".into()),
        Some("cancel") => ("Cancel turn".into(), "Decline this action and stop this turn.".into()),
        _ if decision.get("acceptWithExecpolicyAmendment").is_some() => (
            "Allow and remember command rule".into(),
            format!("Persist this command rule: {}", decision["acceptWithExecpolicyAmendment"]["execpolicy_amendment"]),
        ),
        _ if decision.get("applyNetworkPolicyAmendment").is_some() => (
            "Remember network rule".into(),
            format!("Persist this network rule: {}", decision["applyNetworkPolicyAmendment"]["network_policy_amendment"]),
        ),
        _ => return Err(NorthError::Protocol("Approval contains an unknown decision".into())),
    };
    Ok(decision_option(&label, &description, json!({"decision":decision})))
}

pub fn response(prompt: &Prompt) -> NorthResult<Value> {
    let result = if prompt.outcome == "cancel" {
        serde_json::from_str(&prompt.cancellation)?
    } else if prompt.kind == "questions" {
        let mut answers = serde_json::Map::new();
        for question in &prompt.questions {
            if !question.answered { return Err(NorthError::Protocol("Question is unanswered".into())); }
            answers.insert(question.key.clone(), json!({"answers":[question.answer]}));
        }
        json!({"answers":answers})
    } else if prompt.kind == "form" {
        let consent = prompt.questions.last().filter(|question| question.answered)
            .ok_or_else(|| NorthError::Protocol("Form consent is unanswered".into()))?;
        let action: Value = serde_json::from_str(&consent.answer)?;
        let mut content = serde_json::Map::new();
        for question in prompt.questions.iter().take(prompt.questions.len() - 1) {
            if !question.answered { return Err(NorthError::Protocol("Form field is unanswered".into())); }
            let value: Value = serde_json::from_str(&question.answer)?;
            if !(question.optional && value.is_null()) { content.insert(question.key.clone(), value); }
        }
        json!({"action":action,"content":if action == "accept" { Value::Object(content) } else { Value::Null }})
    } else {
        let question = prompt.questions.first().filter(|question| question.answered)
            .ok_or_else(|| NorthError::Protocol("Approval is unanswered".into()))?;
        serde_json::from_str(&question.answer)?
    };
    let id: Value = serde_json::from_str(&prompt.request)?;
    Ok(json!({"id":id,"result":result}))
}

pub fn current_question(prompt: &Prompt) -> Option<&PromptQuestion> {
    prompt.questions.iter().find(|question| question.number == prompt.current)
}

pub fn render(frame: &mut Frame<'_>, area: Rect, prompt: &Prompt, input: &str, scroll: u16) {
    let block = Block::default().borders(Borders::ALL).title(" Answer needed ")
        .border_style(Style::default().fg(Color::Yellow));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let mut heading = vec![Line::from(format!("Conversation {} · {}", prompt.conversation, prompt.status))];
    let mut choices = Vec::new();
    let mut body = "";
    let mut selected_line = 0;
    if let Some(question) = current_question(prompt) {
        heading.push(Line::from(format!("{} / {} · {}", question.number, prompt.questions.len(), question.header))
            .style(Style::default().add_modifier(Modifier::BOLD)));
        body = &question.text;
        for option in &question.options {
            let selected = option.number == question.selected;
            if selected { selected_line = choices.len(); }
            choices.push(Line::from(vec![
                Span::styled(if selected { "› " } else { "  " }, Style::default().fg(Color::Cyan)),
                Span::raw(if question.multiple { if option.checked { "[x] " } else { "[ ] " } } else { "" }),
                Span::styled(option.label.as_str(), if selected { Style::default().add_modifier(Modifier::BOLD) } else { Style::default() }),
            ]));
            if !option.description.is_empty() { choices.push(Line::from(format!("    {}", option.description))); }
        }
        if question.other && !question.options.is_empty() {
            if question.selected == 0 { selected_line = choices.len(); }
            choices.push(Line::from(if question.selected == 0 { "› Write another answer" } else { "  Write another answer" }));
        }
        if question.text_entry {
            let displayed = if question.secret { "•".repeat(input.chars().count()) } else { input.to_owned() };
            choices.push(Line::from(format!("Answer: {displayed}")));
        }
        if question.multiple { choices.push(Line::from("Space toggles a choice · Enter confirms")); }
        if question.optional { choices.push(Line::from("Optional · Ctrl+S leaves this answer out")); }
        if !question.problem.is_empty() {
            choices.push(Line::from(question.problem.as_str()).style(Style::default().fg(Color::Yellow)));
        }
    }
    let rows = Layout::default().direction(Direction::Vertical).constraints([
        Constraint::Length(2), Constraint::Min(1),
        Constraint::Length((choices.len() as u16).min(inner.height.saturating_sub(4))),
        Constraint::Length(1),
    ]).split(inner);
    frame.render_widget(Paragraph::new(heading), rows[0]);
    let body_height: usize = body.lines().map(|line| line.chars().count().max(1).div_ceil(usize::from(rows[1].width.max(1)))).sum();
    let scroll = scroll.min(body_height.saturating_sub(rows[1].height as usize).min(u16::MAX as usize) as u16);
    frame.render_widget(Paragraph::new(body).wrap(Wrap { trim: false }).scroll((scroll, 0)), rows[1]);
    let offset = selected_line.saturating_sub(rows[2].height.saturating_sub(2) as usize).min(u16::MAX as usize) as u16;
    frame.render_widget(Paragraph::new(choices).scroll((offset, 0)), rows[2]);
    let help = if prompt.cancellation.is_empty() { "↑/↓ choose · Enter answer · PgUp/PgDn details" }
        else { "↑/↓ choose · Enter answer · Esc cancel · PgUp/PgDn details" };
    frame.render_widget(Paragraph::new(help), rows[3]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clause_state::NorthState;

    fn state() -> NorthState {
        let mut state = NorthState::open().unwrap();
        state.request_new_conversation().unwrap();
        state.settle_new_conversation("thread").unwrap();
        state
    }

    fn command(id: Value, thread: &str) -> Value {
        json!({"id":id,"method":"item/commandExecution/requestApproval","params":{
            "threadId":thread,"turnId":"turn","itemId":"command","command":"git status",
            "availableDecisions":["decline","accept","cancel"],
        }})
    }

    #[test]
    fn approval_uses_only_offered_decisions_and_retains_exact_request_identity() {
        let mut state = state();
        let input = decode_request(&command(json!("7"), "thread"), &[]).unwrap().unwrap();
        state.observe_prompt(&input).unwrap();
        state.observe_prompt(&input).unwrap();
        assert_eq!(state.prompts().len(), 1);
        let prompt = state.active_prompt().unwrap();
        assert_eq!(prompt.questions[0].options.len(), 3);
        assert_eq!(prompt.questions[0].options[0].label, "Deny");
        assert_eq!(state.prepare_prompt_response().unwrap(), None);
        state.navigate_prompt(1).unwrap();
        state.answer_prompt_choice().unwrap();
        state.prepare_prompt_response().unwrap().unwrap();
        assert_eq!(response(state.active_prompt().unwrap()).unwrap(), json!({"id":"7","result":{"decision":"accept"}}));
        state.clear_host_effect().unwrap();
        assert_eq!(state.prepare_prompt_response().unwrap(), None);
        state.resolve_prompt("other-thread", "\"7\"").unwrap();
        assert!(state.active_prompt().is_some());
        state.resolve_prompt("thread", "\"7\"").unwrap();
        state.prompt_response_written(1, true).unwrap();
        assert!(state.active_prompt().is_none());
        assert!(state.prompts()[0].questions[0].answer.is_empty());
    }

    #[test]
    fn multiple_questions_accept_choices_and_free_text_without_crossing_threads() {
        let mut state = state();
        let hidden = decode_request(&command(json!(8), "other-thread"), &[]).unwrap().unwrap();
        state.observe_prompt(&hidden).unwrap();
        assert!(state.active_prompt().is_none());
        let request = json!({"id":9,"method":"item/tool/requestUserInput","params":{
            "threadId":"thread","turnId":"turn","itemId":"questions","questions":[
                {"id":"design","header":"Design","question":"Choose an approach", "isOther":true,
                 "options":[{"label":"Small","description":"A small change"}]},
                {"id":"details","header":"Details","question":"Anything else?", "isSecret":true,"options":null},
            ]
        }});
        state.observe_prompt(&decode_request(&request, &[]).unwrap().unwrap()).unwrap();
        state.navigate_prompt(1).unwrap();
        assert!(state.active_prompt().unwrap().questions[0].text_entry);
        state.answer_prompt_text("A different design").unwrap();
        assert_eq!(state.prepare_prompt_response().unwrap(), None);
        state.answer_prompt_text("private answer fixture").unwrap();
        state.prepare_prompt_response().unwrap().unwrap();
        assert_eq!(response(state.active_prompt().unwrap()).unwrap(), json!({"id":9,"result":{"answers":{
            "design":{"answers":["A different design"]},"details":{"answers":["private answer fixture"]},
        }}}));
        state.clear_host_effect().unwrap();
        state.finish_observed_turn("other-thread", "turn").unwrap();
        assert!(state.active_prompt().is_some());
        state.finish_observed_turn("thread", "turn").unwrap();
        assert!(state.active_prompt().is_none());
        assert!(state.prompts()[1].questions.iter().all(|question| question.answer.is_empty()));
    }

    #[test]
    fn permission_grants_are_exactly_the_requested_access_and_explicit_scope() {
        let requested = json!({"network":{"enabled":true},"fileSystem":{"write":["/tmp/north-proof"]}});
        for (selection, expected) in [
            (0, json!({"permissions":requested,"scope":"turn"})),
            (1, json!({"permissions":requested,"scope":"session"})),
            (2, json!({"permissions":{},"scope":"turn"})),
        ] {
            let mut state = state();
            let request = json!({"id":1,"method":"item/permissions/requestApproval","params":{
                "threadId":"thread","turnId":"turn","itemId":"permission","permissions":requested,
            }});
            state.observe_prompt(&decode_request(&request, &[]).unwrap().unwrap()).unwrap();
            for _ in 0..selection { state.navigate_prompt(1).unwrap(); }
            state.answer_prompt_choice().unwrap();
            state.prepare_prompt_response().unwrap().unwrap();
            assert_eq!(response(state.active_prompt().unwrap()).unwrap(), json!({"id":1,"result":expected}));
        }
    }

    #[test]
    fn cancellation_is_explicit_and_unknown_delivery_never_resends() {
        let mut state = state();
        state.observe_prompt(&decode_request(&command(json!(1), "thread"), &[]).unwrap().unwrap()).unwrap();
        state.cancel_prompt().unwrap();
        let number = state.prepare_prompt_response().unwrap().unwrap();
        assert_eq!(response(state.active_prompt().unwrap()).unwrap(), json!({"id":1,"result":{"decision":"cancel"}}));
        state.clear_host_effect().unwrap();
        state.prompt_response_written(number, false).unwrap();
        assert_eq!(state.active_prompt().unwrap().status, "delivery unknown");
        assert_eq!(state.prepare_prompt_response().unwrap(), None);
    }

    #[test]
    fn empty_forms_still_require_consent_and_declining_does_not_send_content() {
        let mut state = state();
        let message = json!({"id":73,"method":"mcpServer/elicitation/request","params":{
            "threadId":"thread","serverName":"fixture-tool","mode":"form","message":"Continue?",
            "requestedSchema":{"type":"object","properties":{}}
        }});
        state.observe_prompt(&decode_request(&message, &[]).unwrap().unwrap()).unwrap();
        assert!(state.prepare_prompt_response().unwrap().is_none());
        state.navigate_prompt(1).unwrap();
        state.answer_prompt_choice().unwrap();
        state.prepare_prompt_response().unwrap().unwrap();
        assert_eq!(response(state.active_prompt().unwrap()).unwrap(), json!({"id":73,"result":{"action":"decline","content":null}}));
    }
}
