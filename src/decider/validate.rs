use std::fmt::Display;

use crate::Result;
use crate::decisions::Decision;
use crate::msg::Msg;

use super::{Config, Offer, Question};

pub(crate) struct Proposal {
    pub decision: Decision,
    pub p: f64,
    pub candidate: String,
    pub confidence: Option<f64>,
}

pub(super) fn invalid(reason: impl Display) -> Box<dyn std::error::Error> {
    Msg::DecideInvalid(&reason.to_string()).text().into()
}

pub(super) fn probability(p: f64) -> Result<f64> {
    match crate::decisions::probability(p) {
        true => Ok(p),
        false => Err(invalid(Msg::AnswerProbability(p).text())),
    }
}

pub(super) fn proposal(
    config: &Config,
    question: &Question,
    choice: &str,
    p: f64,
    by: &str,
) -> Result<Proposal> {
    let offer = offered(config, question, choice)?;
    let p = probability(p)?;
    let shown = config.shown(by);
    if by.is_empty() || by == "human" || by.chars().any(char::is_control) {
        return Err(invalid(Msg::AnswerBy(&shown).text()));
    }
    Ok(Proposal {
        decision: Decision {
            kind: question.kind,
            call: question.id.clone(),
            target: offer.target.clone(),
            by: shown,
            p: Some(p),
        },
        p,
        candidate: offer.description.clone(),
        confidence: None,
    })
}

pub(super) fn offered<'q>(
    config: &Config,
    question: &'q Question,
    choice: &str,
) -> Result<&'q Offer> {
    let found = question.offers.iter().find(|o| o.key == choice);
    found.ok_or_else(|| {
        let keys = question.keys().join(", ");
        let choice = &config.shown(choice);
        invalid(
            Msg::AnswerChoice {
                choice,
                keys: &keys,
            }
            .text(),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::probability;

    #[test]
    fn a_probability_is_a_number_from_zero_to_one() {
        for (p, valid) in [
            (0.0, true),
            (0.5, true),
            (1.0, true),
            (-0.01, false),
            (1.01, false),
            (-1.0, false),
            (2.0, false),
            (f64::NAN, false),
            (f64::INFINITY, false),
            (f64::NEG_INFINITY, false),
        ] {
            assert_eq!(probability(p).is_ok(), valid, "{p}");
        }
    }
}
