mod chat;
mod decisions;
mod validate;

use std::env::VarError;
use std::fs;
use std::io::Write;
use std::net::IpAddr;
use std::ops::Range;
use std::path::Path;
use std::time::Duration;

use serde_json::Value;

use crate::Result;
use crate::decisions::{Decision, Kind};
use crate::engine::OpenItem;
use crate::lang;
use crate::msg::Msg;
use validate::Proposal;

// ponytail: the excerpt of one call stays small; past this an item is left to a person.
pub const MAX_CANDIDATES: usize = 50;

const TIMEOUT: Duration = Duration::from_secs(120);

pub const MIN_P: f64 = 0.5;

const HIDDEN: &str = "<hidden>";

const PROTOCOL: &str = "MEADA_DECIDER_PROTOCOL";
const URL: &str = "MEADA_DECIDER_URL";
const KEY: &str = "MEADA_DECIDER_KEY";
const MODEL: &str = "MEADA_DECIDER_MODEL";

const DECISIONS_URL: &str = "https://openrouter.ai";
const DECISIONS_MODEL: &str = "typesafe/jev-1.14";

fn task(repository: &str) -> String {
    format!(
        "Pick the HTTP endpoint that a call in the repository {repository} reaches. You see the \
         call, the function it is in, and the candidate endpoints, each under a key. Answer with \
         the key of one candidate and the probability, from 0 to 1, that it is the right one."
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Protocol {
    Decisions,
    Chat,
}

pub(crate) struct Config {
    pub protocol: Protocol,
    pub url: String,
    key: Option<String>,
    pub model: String,
}

impl Config {
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

fn allowed(url: &str) -> bool {
    match url.split_once("://") {
        Some((scheme, rest)) if scheme.eq_ignore_ascii_case("https") => !rest.is_empty(),
        Some((scheme, _)) if scheme.eq_ignore_ascii_case("http") => loopback(url),
        _ => false,
    }
}

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

pub(crate) struct Question {
    pub id: String,
    /// The identity of the repository that holds the call.
    pub repository: String,
    pub call: String,
    pub enclosing: Option<String>,
    pub top_level: bool,
    pub offers: Vec<Offer>,
    pub kind: Kind,
}

pub(crate) struct Offer {
    pub key: String,
    pub description: String,
    pub target: String,
}

impl Question {
    fn new(root: &Path, stored: Stored, item: &OpenItem) -> Result<Self> {
        let enclosing = match &item.enclosing {
            Some(symbol) => {
                let path = root.join(&item.path);
                let source =
                    fs::read(&path).map_err(|error| crate::decisions::at_path(&path, error))?;
                function_like(root, stored, &item.path, &source, &symbol.span).then(|| {
                    let text = source.get(symbol.span.clone()).unwrap_or_default();
                    String::from_utf8_lossy(text).into_owned()
                })
            }
            None => None,
        };
        Ok(Self {
            id: item.id.clone(),
            repository: item.repository.clone(),
            call: item.expression.clone(),
            enclosing,
            top_level: item.enclosing.is_none(),
            offers: (item.candidates.iter().enumerate())
                .map(|(n, c)| Offer {
                    key: (n + 1).to_string(),
                    description: c.describe(),
                    target: Decision::target(c),
                })
                .collect(),
            kind: item.decision_kind(),
        })
    }

    pub fn excerpt(&self) -> String {
        match (&self.enclosing, self.top_level) {
            (Some(function), _) => format!("Call:\n{}\n\nIn:\n{function}\n", self.call),
            (None, true) => format!("Call (at the top level of its file):\n{}\n", self.call),
            (None, false) => format!("Call:\n{}\n", self.call),
        }
    }

    pub fn listing(&self) -> String {
        (self.offers.iter())
            .map(|o| format!("{}: {}\n", o.key, o.description))
            .collect()
    }

    pub fn keys(&self) -> Vec<&str> {
        self.offers.iter().map(|o| o.key.as_str()).collect()
    }
}

pub(crate) type Stored<'a> = &'a dyn Fn(&str) -> Option<String>;

fn setting_on_disk(pack: &dyn lang::LanguagePack, root: &Path, path: &str) -> String {
    let mut dirs = vec![String::new()];
    let mut walked = String::new();
    for part in path
        .split('/')
        .rev()
        .skip(1)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        walked = format!("{walked}{part}/");
        dirs.push(walked.clone());
    }
    let mut found: Vec<(String, Vec<u8>)> = Vec::new();
    for name in pack.config_files() {
        let name = name.strip_prefix("**/").unwrap_or(name);
        for dir in &dirs {
            let config = format!("{dir}{name}");
            if let Some(src) = crate::index::read_config(root, &config) {
                found.push((config, src));
            }
        }
    }
    let configs: Vec<(&str, &[u8])> = found
        .iter()
        .map(|(n, c)| (n.as_str(), c.as_slice()))
        .collect();
    pack.settings(&configs)(path)
}

fn function_like(
    root: &Path,
    stored: Stored,
    path: &str,
    source: &[u8],
    span: &Range<usize>,
) -> bool {
    use lang::Kind::{Function, Inline, Method};
    let Some(pack) = lang::pack_for(path).map(lang::pack) else {
        return false;
    };
    let setting = stored(path).unwrap_or_else(|| setting_on_disk(pack, root, path));
    (pack.parse_with(path, source, &setting)).is_ok_and(|facts| {
        (facts.defs.iter())
            .any(|d| d.range == *span && matches!(d.kind, Function | Method | Inline))
    })
}

pub(crate) struct Request {
    pub items: Vec<String>,
    pub url: String,
    pub body: Value,
}

impl Config {
    fn requests(&self, questions: &[Question]) -> Vec<Request> {
        match self.protocol {
            Protocol::Chat => questions.iter().map(|q| chat::request(self, q)).collect(),
            Protocol::Decisions => decisions::request(self, questions).into_iter().collect(),
        }
    }

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

    fn failure(&self, status: u16, body: &str) -> String {
        let message = serde_json::from_str::<Value>(body)
            .ok()
            .and_then(|v| Some(self.shown(v["error"]["message"].as_str()?)));
        let message = message.as_deref().filter(|m| !m.is_empty());
        Msg::HttpStatus { status, message }.text()
    }

    pub(super) fn shown(&self, text: &str) -> String {
        let text: String = text.chars().filter(|c| !c.is_control()).collect();
        match &self.key {
            Some(key) => text.replace(key.as_str(), HIDDEN),
            None => text,
        }
    }
}

#[expect(clippy::too_many_arguments)]
pub(crate) fn run(
    config: &Config,
    root: &Path,
    stored: Stored,
    items: &[OpenItem],
    show_request: bool,
    min_p: f64,
    mut accept: impl FnMut(&Decision) -> Result<()>,
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
                questions.push(Question::new(root, stored, item)?);
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
        .max_redirects(0)
        .http_status_as_error(false);
    if loopback(&config.url) {
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
            if let Ok(proposal) = &answer
                && proposal.p >= min_p
            {
                accept(&proposal.decision)?;
            }
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

pub(crate) fn min_p(value: f64) -> Result<f64> {
    match crate::decisions::probability(value) {
        true => Ok(value),
        false => Err(Msg::DecideMinP.text().into()),
    }
}

#[derive(Default)]
pub(crate) struct Outcome {
    pub decisions: Vec<Decision>,
    pub failed: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str =
        "macro_rules! cfg_rt { ($($i:item)*) => { $($i)* } }\ncfg_rt! { pub fn spawn() {} }\n";

    fn span() -> Range<usize> {
        let start = SRC.find("pub fn spawn").unwrap();
        start..start + "pub fn spawn() {}".len()
    }

    #[test]
    fn a_function_in_an_item_macro_body_is_function_like_under_the_stored_setting() {
        let root = Path::new("/nonexistent");
        let stored = |_: &str| Some(lang::setting("", "cfg_rt"));
        assert!(function_like(
            root,
            &stored,
            "src/lib.rs",
            SRC.as_bytes(),
            &span()
        ));
        let none = |_: &str| None;
        assert!(!function_like(
            root,
            &none,
            "src/lib.rs",
            SRC.as_bytes(),
            &span()
        ));
    }
}
