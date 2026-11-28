use serde::Deserialize;
use serde_json::json;

use super::validate::{Proposal, invalid, proposal};
use super::{Config, Question, Request, task};
use crate::Result;
use crate::msg::Msg;

pub(super) fn request(config: &Config, question: &Question) -> Request {
    let user = format!(
        "{}\nCandidates:\n{}",
        question.excerpt(),
        question.listing()
    );
    Request {
        items: vec![question.id.clone()],
        url: format!("{}/chat/completions", config.url),
        body: json!({
            "model": config.model,
            "messages": [
                {"role": "system", "content": task(&question.repository)},
                {"role": "user", "content": user},
            ],
            "response_format": {
                "type": "json_schema",
                "json_schema": {
                    "name": "decision",
                    "strict": true,
                    "schema": {
                        "type": "object",
                        "properties": {
                            "choice": {"type": "string", "enum": question.keys()},
                            "probability": {"type": "number"},
                        },
                        "required": ["choice", "probability"],
                        "additionalProperties": false,
                    },
                },
            },
        }),
    }
}

#[derive(Deserialize)]
struct Completion {
    model: Option<String>,
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Deserialize)]
struct Message {
    content: String,
}

#[derive(Deserialize)]
struct Answer {
    choice: String,
    probability: f64,
}

pub(super) fn answer(config: &Config, question: &Question, body: &str) -> Result<Proposal> {
    let unreadable =
        |e: serde_json::Error| invalid(Msg::AnswerUnreadable(&config.shown(&e.to_string())).text());
    let completion: Completion = serde_json::from_str(body).map_err(unreadable)?;
    let message = completion
        .choices
        .first()
        .ok_or_else(|| invalid(Msg::AnswerNoChoices.text()))?;
    let answer: Answer = serde_json::from_str(&message.message.content).map_err(unreadable)?;
    let by = completion.model.as_deref().unwrap_or(&config.model);
    proposal(config, question, &answer.choice, answer.probability, by)
}
