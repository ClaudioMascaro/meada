//! The `chat` protocol (spec #18): any OpenAI-compatible chat completions endpoint, a local
//! model included. One `POST <url>/chat/completions` per item; the response format is a JSON
//! schema with `choice`, one of the candidate keys, and `probability`, a number the model
//! reports about itself.

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

/// The success body of a chat completion, as far as Meada reads it.
#[derive(Deserialize)]
struct Completion {
    /// The model that answered; the configured one stands in when it is absent.
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

/// What the response format asks the model for. The probability is self-reported.
#[derive(Deserialize)]
struct Answer {
    choice: String,
    probability: f64,
}

/// Reads the body of the answer to `question` into a validated proposal. `by` is the model id
/// in the response, or the configured model when the response omits it.
pub(super) fn answer(config: &Config, question: &Question, body: &str) -> Result<Proposal> {
    let unreadable =
        |e: serde_json::Error| invalid(Msg::AnswerUnreadable(&config.shown(&e.to_string())).text());
    let completion: Completion = serde_json::from_str(body).map_err(unreadable)?;
    let message = completion
        .choices
        .first()
        .ok_or_else(|| invalid(Msg::AnswerUnreadable("no choices").text()))?;
    let answer: Answer = serde_json::from_str(&message.message.content).map_err(unreadable)?;
    let by = completion.model.as_deref().unwrap_or(&config.model);
    proposal(config, question, &answer.choice, answer.probability, by)
}
