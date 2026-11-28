use std::collections::BTreeMap;

use serde::Deserialize;
use serde::de::IgnoredAny;
use serde_json::{Map, Value, json};

use super::validate::{Proposal, invalid, offered, probability, proposal};
use super::{Config, Question, Request, task};
use crate::Result;
use crate::msg::Msg;

pub const SUM_TOLERANCE: f64 = 0.01;

fn names(count: usize) -> impl Iterator<Item = String> {
    let width = count.to_string().len();
    (1..=count).map(move |n| format!("item{n:0width$}"))
}

pub(super) fn request(config: &Config, questions: &[Question]) -> Option<Request> {
    let repository = &questions.first()?.repository;
    let mut items = Map::new();
    let mut asked = Map::new();
    for (name, question) in names(questions.len()).zip(questions) {
        let criteria: Map<String, Value> = (question.offers.iter())
            .map(|o| (o.key.clone(), o.description.as_str().into()))
            .collect();
        let instructions =
            format!("Pick the candidate endpoint that the call in `items.{name}` reaches.");
        asked.insert(
            name.clone(),
            json!({"type": "choice", "instructions": instructions, "criteria": criteria}),
        );
        items.insert(name, question.excerpt().into());
    }
    Some(Request {
        items: questions.iter().map(|q| q.id.clone()).collect(),
        url: format!("{}/api/alpha/decisions", config.url),
        body: json!({
            "model": config.model,
            "state": {"task": task(repository), "repository": repository, "items": items},
            "questions": asked,
            "provider": {"zdr": true, "data_collection": "deny"},
        }),
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Response {
    model: String,
    answers: BTreeMap<String, Answer>,
    #[serde(rename = "id")]
    _id: Option<IgnoredAny>,
    #[serde(rename = "provider")]
    _provider: Option<IgnoredAny>,
    #[serde(rename = "usage")]
    _usage: Option<IgnoredAny>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Answer {
    #[serde(rename = "type")]
    kind: String,
    choice: String,
    confidence: f64,
    probabilities: BTreeMap<String, f64>,
}

pub(super) fn answers(config: &Config, asked: &[&Question], body: &str) -> Vec<Result<Proposal>> {
    let names: Vec<String> = names(asked.len()).collect();
    let mut response = match read(config, &names, body) {
        Ok(response) => response,
        Err(e) => {
            let text = e.to_string();
            return asked.iter().map(|_| Err(text.clone().into())).collect();
        }
    };
    (names.iter().zip(asked))
        .map(|(name, question)| match response.answers.remove(name) {
            Some(answer) => validated(config, question, answer, &response.model),
            None => Err(invalid(Msg::AnswerMissing.text())),
        })
        .collect()
}

fn read(config: &Config, names: &[String], body: &str) -> Result<Response> {
    let response: Response = serde_json::from_str(body)
        .map_err(|e| invalid(Msg::AnswerUnreadable(&config.shown(&e.to_string())).text()))?;
    match response.answers.keys().find(|name| !names.contains(name)) {
        Some(name) => Err(invalid(Msg::AnswerUnknown(&config.shown(name)).text())),
        None => Ok(response),
    }
}

fn validated(config: &Config, question: &Question, answer: Answer, by: &str) -> Result<Proposal> {
    if answer.kind != "choice" {
        return Err(invalid(Msg::AnswerType(&config.shown(&answer.kind)).text()));
    }
    offered(config, question, &answer.choice)?;
    // ponytail: the documentation gives no range for `confidence`; [0,1] is assumed.
    let confidence = probability(answer.confidence)
        .map_err(|_| invalid(Msg::AnswerConfidence(answer.confidence).text()))?;
    for (key, &p) in &answer.probabilities {
        offered(config, question, key)
            .map_err(|_| invalid(Msg::AnswerKey(&config.shown(key)).text()))?;
        probability(p)?;
    }
    let sum: f64 = answer.probabilities.values().sum();
    if (sum - 1.0).abs() > SUM_TOLERANCE {
        let tolerance = SUM_TOLERANCE;
        return Err(invalid(Msg::AnswerSum { sum, tolerance }.text()));
    }
    let Some(&p) = answer.probabilities.get(&answer.choice) else {
        let choice = config.shown(&answer.choice);
        return Err(invalid(Msg::AnswerNoProbability(&choice).text()));
    };
    let mut proposal = proposal(config, question, &answer.choice, p, by)?;
    proposal.confidence = Some(confidence);
    Ok(proposal)
}

#[cfg(test)]
mod tests {
    use super::{Answer, names, validated};
    use crate::decider::{Config, Offer, Protocol, Question};
    use crate::decisions::Kind;

    #[test]
    fn probabilities_sum_to_one_within_the_rounding_tolerance() {
        let question = Question {
            id: "r::a.ts::fetch#0".into(),
            repository: "r".into(),
            call: "fetch(url)".into(),
            enclosing: None,
            top_level: true,
            offers: ["1", "2", "3"]
                .map(|k| Offer {
                    key: k.into(),
                    description: format!("r GET /{k}"),
                    target: format!("r::http::GET::/{k}"),
                })
                .into(),
            kind: Kind::DynamicUrl,
        };
        for (probabilities, valid) in [
            ([0.5, 0.25, 0.25], true),
            ([0.333, 0.333, 0.333], true),
            ([0.334, 0.334, 0.334], true),
            ([0.5, 0.25, 0.245], true),
            ([0.34, 0.34, 0.34], false),
            ([0.5, 0.25, 0.235], false),
            ([0.5, 0.3, 0.3], false),
            ([0.0, 0.0, 0.0], false),
        ] {
            let answer = Answer {
                kind: "choice".into(),
                choice: "1".into(),
                confidence: 0.5,
                probabilities: ["1", "2", "3"]
                    .map(String::from)
                    .into_iter()
                    .zip(probabilities)
                    .collect(),
            };
            let config = Config {
                protocol: Protocol::Decisions,
                url: "https://example.com".into(),
                key: None,
                model: "m".into(),
            };
            assert_eq!(
                validated(&config, &question, answer, "m").is_ok(),
                valid,
                "{probabilities:?}"
            );
        }
    }

    #[test]
    fn question_names_sort_in_item_order_past_nine_items() {
        let names: Vec<String> = names(12).collect();
        assert_eq!(names.first().map(String::as_str), Some("item01"));
        assert_eq!(names.last().map(String::as_str), Some("item12"));
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(sorted, names);
    }
}
