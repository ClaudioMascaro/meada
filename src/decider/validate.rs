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

use super::Question;

/// An answer that passed validation: the decision it would record, and the candidate it picks
/// as a person reads it.
pub(crate) struct Proposal {
    pub decision: Decision,
    /// The probability of the decision.
    pub p: f64,
    /// The picked candidate: repository identity, method and path.
    pub candidate: String,
}

/// The error for an answer that is not valid, with `reason` as its detail.
pub(super) fn invalid(reason: impl Display) -> Box<dyn std::error::Error> {
    Msg::DecideInvalid(&reason.to_string()).text().into()
}

/// `p` if it is a number in [0,1] (so neither NaN nor infinite).
pub(super) fn probability(p: f64) -> Result<f64> {
    match (0.0..=1.0).contains(&p) {
        true => Ok(p),
        false => Err(invalid(Msg::AnswerProbability(p).text())),
    }
}

/// The answer of a decider to `question`: `choice` must be an offered key, `p` a probability,
/// and `by` a model id (`human` is the person's, so a model may not take it).
pub(super) fn proposal(question: &Question, choice: &str, p: f64, by: &str) -> Result<Proposal> {
    let Some(at) = question.keys().iter().position(|key| *key == choice) else {
        let keys = question.keys().join(", ");
        return Err(invalid(
            Msg::AnswerChoice {
                choice,
                keys: &keys,
            }
            .text(),
        ));
    };
    let p = probability(p)?;
    if by.is_empty() || by == "human" {
        return Err(invalid(Msg::AnswerBy(by).text()));
    }
    Ok(Proposal {
        decision: Decision {
            kind: question.kind,
            call: question.id.clone(),
            target: question.targets[at].clone(),
            by: by.into(),
            p: Some(p),
        },
        p,
        candidate: question.candidates[at].1.clone(),
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
