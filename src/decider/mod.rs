//! Decider mode of `meada decide` (spec #18, ADR 0006): the configuration from the environment,
//! the excerpt of each item, the requests of the configured protocol and the HTTP client. This
//! is the only network path of Meada, and only `meada decide --decider` reaches it.
//!
//! The protocol adapters (`chat` and `decisions`) only turn questions into
//! requests and answers into proposals; reading the environment, choosing the eligible items,
//! printing and sending stay here. The key is held by `Config` alone, which has no `Debug`, and
//! leaves it only as the `Authorization` header of a request on the wire.

mod chat;
mod decisions;
mod validate;

use std::env::VarError;
use std::fs;
use std::io::Write;
use std::net::IpAddr;
use std::path::Path;
use std::time::Duration;

use serde_json::Value;

use crate::Result;
use crate::decisions::{Decision, Kind};
use crate::engine::OpenItem;
use crate::msg::Msg;
use validate::Proposal;

/// An item with more candidates than this is skipped in decider mode and reported as such; the
/// excerpt would grow past what a judgment of one call needs. Human mode still offers it.
pub const MAX_CANDIDATES: usize = 50;

/// How long one request may take, connection to last byte. There are no retries.
const TIMEOUT: Duration = Duration::from_secs(120);

/// A model decision is written when its `p` is at least this, unless `--min-p` says otherwise;
/// below it the proposal is only printed, as a suggestion. Changed by a PR with data (ADR 0006).
/// With `chat` the probability is the model's own report about itself, not a calibrated measure.
pub const MIN_P: f64 = 0.5;

/// What `--show-request` prints in place of the `Authorization` header value.
const HIDDEN: &str = "<hidden>";

const PROTOCOL: &str = "MEADA_DECIDER_PROTOCOL";
const URL: &str = "MEADA_DECIDER_URL";
const KEY: &str = "MEADA_DECIDER_KEY";
const MODEL: &str = "MEADA_DECIDER_MODEL";

/// The defaults of the `decisions` protocol: OpenRouter, and Jev.
const DECISIONS_URL: &str = "https://openrouter.ai";
const DECISIONS_MODEL: &str = "typesafe/jev-1.13";

/// The task every protocol states, with the repository identity of the item.
fn task(repository: &str) -> String {
    format!(
        "Pick the HTTP endpoint that a call in the repository {repository} reaches. You see the \
         call, the function it is in, and the candidate endpoints, each under a key. Answer with \
         the key of one candidate and the probability, from 0 to 1, that it is the right one."
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Protocol {
    /// OpenRouter's decisions API: one request carries every item.
    Decisions,
    /// An OpenAI-compatible chat completions endpoint: one request per item.
    Chat,
}

/// The decider configuration. Deliberately not `Debug`: the key must never be printed.
pub(crate) struct Config {
    pub protocol: Protocol,
    /// The base URL, without a trailing `/`.
    pub url: String,
    key: Option<String>,
    pub model: String,
}

impl Config {
    /// Reads the decider variables. A missing or invalid one is an error naming it; an empty
    /// value counts as missing. Nothing is read from a file.
    pub(crate) fn from_env() -> Result<Self> {
        let var = |name: &'static str| match std::env::var(name) {
            Ok(value) if value.is_empty() => Ok(None),
            Ok(value) => Ok(Some(value)),
            Err(VarError::NotPresent) => Ok(None),
            Err(VarError::NotUnicode(_)) => Err(Msg::DeciderInvalid(name).text()),
        };
        let protocol = match var(PROTOCOL)?.as_deref() {
            None | Some("decisions") => Protocol::Decisions,
            Some("chat") => Protocol::Chat,
            Some(_) => return Err(Msg::DeciderProtocol.text().into()),
        };
        let (url, model) = match protocol {
            Protocol::Decisions => (Some(DECISIONS_URL), Some(DECISIONS_MODEL)),
            Protocol::Chat => (None, None),
        };
        let required = |name, default: Option<&str>| match var(name)? {
            Some(value) => Ok(value),
            None => default
                .map(String::from)
                .ok_or_else(|| Msg::KeyMissing(name).text()),
        };
        let url = required(URL, url)?;
        if !allowed(&url) {
            return Err(Msg::DeciderHttps.text().into());
        }
        let model = required(MODEL, model)?;
        let key = var(KEY)?;
        match &key {
            None if protocol == Protocol::Decisions => {
                return Err(Msg::KeyMissing(KEY).text().into());
            }
            // A token of visible ASCII only: anything else could not travel as a header value.
            Some(key) if !key.bytes().all(|b| b.is_ascii_graphic()) => {
                return Err(Msg::DeciderInvalid(KEY).text().into());
            }
            _ => {}
        }
        Ok(Self {
            protocol,
            url: url.trim_end_matches('/').into(),
            key,
            model,
        })
    }
}

/// HTTPS anywhere; plain HTTP only to a loopback address (a local model, the test stub). The
/// scheme is case-insensitive.
fn allowed(url: &str) -> bool {
    match url.split_once("://") {
        Some((scheme, rest)) if scheme.eq_ignore_ascii_case("https") => !rest.is_empty(),
        Some((scheme, _)) if scheme.eq_ignore_ascii_case("http") => loopback(url),
        _ => false,
    }
}

/// Whether the host of `url` is `localhost` or a loopback IP address.
fn loopback(url: &str) -> bool {
    let Some((_, rest)) = url.split_once("://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#', '\\']).next().unwrap_or_default();
    let host = authority.rsplit('@').next().unwrap_or_default();
    let host = match host.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or_default(),
        None => host.split(':').next().unwrap_or_default(),
    };
    host.eq_ignore_ascii_case("localhost")
        || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// What a decider sees of one item (ADR 0002, invariant 2): the call expression, the span of
/// its enclosing function read from disk, and the candidates. No provider code, nothing else
/// from the repository.
pub(crate) struct Question {
    /// The item id, its call-site identity.
    pub id: String,
    /// The identity of the repository that holds the call.
    pub repository: String,
    /// The call as written, on one line.
    pub call: String,
    /// The enclosing function as it is on disk; `None` for a call at the top level of a file.
    pub enclosing: Option<String>,
    /// The candidates, in candidate order.
    pub offers: Vec<Offer>,
    /// The kind of decision that resolves the item.
    pub kind: Kind,
}

/// One candidate as a decider is offered it.
pub(crate) struct Offer {
    /// The candidate's 1-based position, as a string.
    pub key: String,
    /// Its repository identity, method and path.
    pub description: String,
    /// The `target` of the decision that picks it.
    pub target: String,
}

impl Question {
    fn new(root: &Path, item: &OpenItem) -> Result<Self> {
        let enclosing = match &item.enclosing {
            Some(symbol) => {
                let source = fs::read(root.join(&item.path))?;
                let text = source.get(symbol.span.clone()).unwrap_or_default();
                Some(String::from_utf8_lossy(text).into_owned())
            }
            None => None,
        };
        Ok(Self {
            id: item.id.clone(),
            repository: item.repository.clone(),
            call: item.expression.clone(),
            enclosing,
            offers: (item.candidates.iter().enumerate())
                .map(|(n, c)| Offer {
                    key: (n + 1).to_string(),
                    description: c.describe(),
                    target: Decision::target(c),
                })
                .collect(),
            kind: Kind::of(item),
        })
    }

    /// The call and its enclosing function, as plain text.
    pub fn excerpt(&self) -> String {
        match &self.enclosing {
            Some(function) => format!("Call:\n{}\n\nIn:\n{function}\n", self.call),
            None => format!("Call (at the top level of its file):\n{}\n", self.call),
        }
    }

    /// The candidates, one `<key>: <description>` per line.
    pub fn listing(&self) -> String {
        (self.offers.iter())
            .map(|o| format!("{}: {}\n", o.key, o.description))
            .collect()
    }

    pub fn keys(&self) -> Vec<&str> {
        self.offers.iter().map(|o| o.key.as_str()).collect()
    }
}

/// One HTTP request of a run: always a `POST` of a JSON body.
pub(crate) struct Request {
    /// The ids of the items it asks about, in item order.
    pub items: Vec<String>,
    /// The full endpoint URL.
    pub url: String,
    pub body: Value,
}

impl Config {
    /// The requests the configured protocol sends for `questions`, in item order.
    fn requests(&self, questions: &[Question]) -> Vec<Request> {
        match self.protocol {
            Protocol::Chat => questions.iter().map(|q| chat::request(self, q)).collect(),
            Protocol::Decisions => decisions::request(self, questions).into_iter().collect(),
        }
    }

    /// What the configured protocol reads out of the success `body` of `request`: one result
    /// per item of the request, in order, each a validated proposal or its error text.
    fn answers(
        &self,
        request: &Request,
        questions: &[Question],
        body: &str,
    ) -> Vec<Result<Proposal>> {
        let asked = request
            .items
            .iter()
            .filter_map(|id| questions.iter().find(|q| q.id == *id));
        match self.protocol {
            Protocol::Chat => asked.map(|q| chat::answer(self, q, body)).collect(),
            Protocol::Decisions => decisions::answers(self, &asked.collect::<Vec<_>>(), body),
        }
    }

    /// Prints `request` as it would leave the machine, with the key replaced by `HIDDEN`.
    fn show(&self, request: &Request, out: &mut impl Write) -> Result<()> {
        writeln!(out, "POST {}", request.url)?;
        if self.key.is_some() {
            writeln!(out, "Authorization: {HIDDEN}")?;
        }
        writeln!(out, "Content-Type: application/json")?;
        writeln!(out)?;
        writeln!(out, "{}", serde_json::to_string_pretty(&request.body)?)?;
        Ok(())
    }

    /// Sends `request` and returns the body of a success answer. The key goes only into the
    /// `Authorization` header; no error message carries a header. An endpoint that reflects
    /// the key in its answer would put it in what is printed and recorded, so every
    /// occurrence in the body (as is, and as JSON escapes it) becomes `HIDDEN` here, before
    /// anything parses the body.
    ///
    /// A non-success answer is an error with its status and, when its body has the usual
    /// `{"error": {"message": ...}}` shape, that message: it is how a refusal (such as no
    /// provider honouring the data-retention preferences) reaches the user. Nothing retries.
    fn send(&self, agent: &ureq::Agent, request: &Request) -> Result<String> {
        let mut call = agent
            .post(&request.url)
            .header("Content-Type", "application/json");
        if let Some(key) = &self.key {
            call = call.header("Authorization", format!("Bearer {key}"));
        }
        let mut answer = call.send(request.body.to_string())?;
        let status = answer.status();
        let body = match answer.body_mut().read_to_string() {
            Ok(body) => body,
            Err(_) if !status.is_success() => String::new(),
            Err(e) => return Err(e.into()),
        };
        let body = match &self.key {
            Some(key) => {
                let escaped = serde_json::to_string(key)?;
                body.replace(key, HIDDEN)
                    .replace(&escaped[1..escaped.len() - 1], HIDDEN)
            }
            None => body,
        };
        if !status.is_success() {
            return Err(self.failure(status.as_u16(), &body).into());
        }
        Ok(body)
    }

    /// The error text of a non-success answer: `http status: <code>`, then `: <message>` when
    /// the body carries one, through `shown`.
    fn failure(&self, status: u16, body: &str) -> String {
        let message = serde_json::from_str::<Value>(body)
            .ok()
            .and_then(|v| Some(self.shown(v["error"]["message"].as_str()?)));
        let message = message.as_deref().filter(|m| !m.is_empty());
        Msg::HttpStatus { status, message }.text()
    }

    /// Text from upstream made fit to print or record: control characters dropped (so it
    /// cannot drive the terminal), then the key replaced by `HIDDEN`. Every decoded string an
    /// answer contributes goes through here, since a JSON escape such as `&` hides the key
    /// from the redaction of the raw body in `send`.
    pub(super) fn shown(&self, text: &str) -> String {
        let text: String = text.chars().filter(|c| !c.is_control()).collect();
        match &self.key {
            Some(key) => text.replace(key.as_str(), HIDDEN),
            None => text,
        }
    }
}

/// Decider mode over `items`: skips the items with no candidate or more than `MAX_CANDIDATES`
/// and reports them, then prints the requests (`show_request`) or sends them in item order.
/// Each answer is validated for its item alone: a proposal with `p` at least `min_p` is printed
/// and returned to be written, a weaker one is printed as a suggestion, and a failed request
/// or an invalid answer is printed under the item id and counted in `Outcome::failed`.
pub(crate) fn run(
    config: &Config,
    root: &Path,
    items: &[OpenItem],
    show_request: bool,
    min_p: f64,
    out: &mut impl Write,
) -> Result<Outcome> {
    let mut questions = Vec::new();
    let mut skipped = false;
    for item in items {
        let skip = match item.candidates.len() {
            0 => Msg::DecideNoCandidates,
            n if n > MAX_CANDIDATES => Msg::DecideTooMany {
                count: n,
                max: MAX_CANDIDATES,
            },
            _ => {
                questions.push(Question::new(root, item)?);
                continue;
            }
        };
        writeln!(out, "{}\n{}", item.id, skip.text())?;
        skipped = true;
    }
    let requests = config.requests(&questions);
    if show_request {
        for (n, request) in requests.iter().enumerate() {
            if n > 0 || skipped {
                writeln!(out)?;
            }
            config.show(request, out)?;
        }
        return Ok(Outcome::default());
    }
    let mut agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        // A redirect could carry the key to another host or drop HTTPS.
        .max_redirects(0)
        // `send` reads the body of a non-success answer for the reason it gives.
        .http_status_as_error(false);
    if loopback(&config.url) {
        // A proxy would carry the key and the excerpt off the machine, in clear over HTTP.
        agent = agent.proxy(None);
    }
    let agent: ureq::Agent = agent.build().into();
    let mut outcome = Outcome::default();
    for request in &requests {
        let answers = match config.send(&agent, request) {
            Ok(body) => config.answers(request, &questions, &body),
            Err(e) => {
                let failed = Msg::DecideFailed(&e.to_string()).text();
                (request.items.iter())
                    .map(|_| Err(failed.clone().into()))
                    .collect()
            }
        };
        for (id, answer) in request.items.iter().zip(answers) {
            writeln!(out, "{id}")?;
            match answer {
                Ok(proposal) if proposal.p >= min_p => {
                    let line = Msg::DecideProposal {
                        candidate: &proposal.candidate,
                        p: proposal.p,
                        confidence: proposal.confidence,
                        by: &proposal.decision.by,
                    };
                    writeln!(out, "{}", line.text())?;
                    outcome.decisions.push(proposal.decision);
                }
                Ok(proposal) => {
                    let line = Msg::DecideSuggestion {
                        candidate: &proposal.candidate,
                        p: proposal.p,
                        confidence: proposal.confidence,
                        min_p,
                    };
                    writeln!(out, "{}", line.text())?;
                }
                Err(e) => {
                    writeln!(out, "{e}")?;
                    outcome.failed += 1;
                }
            }
        }
    }
    Ok(outcome)
}

/// A `--min-p` value, which must be a probability.
pub(crate) fn min_p(value: f64) -> Result<f64> {
    match crate::decisions::probability(value) {
        true => Ok(value),
        false => Err(Msg::DecideMinP.text().into()),
    }
}

/// What a decider run produced.
#[derive(Default)]
pub(crate) struct Outcome {
    /// The decisions to write: model decisions with `p` at least the threshold.
    pub decisions: Vec<Decision>,
    /// How many items had a failed request or an invalid answer.
    pub failed: usize,
}
