//! What every protocol checks of an answer before it becomes a decision (spec #18): the choice
//! is one of the offered keys, the probability is in [0,1], and the model id can be recorded.
//! The `chat` adapter reads an answer into these three values and calls `proposal`; the
//! `decisions` adapter calls it once per question, after its own strict shape checks.
//!
//! Every error here already carries its final text (`invalid answer: <reason>`), ready to be
//! printed under the item id.

use std::fmt::Display;

use crate::Result;
use crate::decisions::Decision;
use crate::msg::Msg;

use super::{Config, Offer, Question};

/// An answer that passed validation: the decision it would record, and the candidate it picks
/// as a person reads it.
pub(crate) struct Proposal {
    pub decision: Decision,
    /// The probability of the decision.
    pub p: f64,
    /// The picked candidate: repository identity, method and path.
    pub candidate: String,
    /// How sure the decider says it is, when the protocol gives it: printed, not recorded.
    pub confidence: Option<f64>,
}

/// The error for an answer that is not valid, with `reason` as its detail.
pub(super) fn invalid(reason: impl Display) -> Box<dyn std::error::Error> {
    Msg::DecideInvalid(&reason.to_string()).text().into()
}

/// `p` if it is a number in [0,1] (so neither NaN nor infinite).
pub(super) fn probability(p: f64) -> Result<f64> {
    match crate::decisions::probability(p) {
        true => Ok(p),
        false => Err(invalid(Msg::AnswerProbability(p).text())),
    }
}

/// The answer of a decider to `question`: `choice` must be an offered key, `p` a probability,
/// and `by` a model id: printed and recorded, so no control characters, and not `human`, which
/// is the person's. Every upstream string printed or recorded goes through `Config::shown`.
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

/// The offer of `question` whose key is `choice`, or the error naming it.
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
