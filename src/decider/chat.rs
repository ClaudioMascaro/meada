//! The `chat` protocol (spec #18): any OpenAI-compatible chat completions endpoint, a local
//! model included. One `POST <url>/chat/completions` per item; the response format is a JSON
//! schema with `choice`, one of the candidate keys, and `probability`, a number the model
//! reports about itself.

use serde_json::json;

use super::{Config, Question, Request, task};

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
