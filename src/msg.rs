//! The CLI's message catalog: pt-BR and English, one match arm per message. The language comes
//! from `LANG` (ADR 0008): pt-BR when it starts with `pt`, English otherwise. Only the CLI calls
//! `init`, so the MCP server and the library tests stay in English.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::decisions::Cause;

static PT: AtomicBool = AtomicBool::new(false);

/// Picks the language from the value of `LANG`; `None` is English.
pub fn init(lang: Option<&str>) {
    PT.store(lang.is_some_and(|l| l.starts_with("pt")), Ordering::Relaxed);
}

pub enum Msg<'a> {
    NotIndexed,
    OtherVersion,
    Indexed(i64),
    MissingPeer {
        repo: &'a str,
        named_by: &'a str,
    },
    StaleDecision {
        call: &'a str,
        target: &'a str,
        cause: Cause,
    },
    WaitingDecision {
        call: &'a str,
        target: &'a str,
        peer: &'a str,
    },
    TooManyRepos {
        found: usize,
        max: usize,
    },
    ViaUnmatched {
        contract: &'a str,
        via: &'a str,
    },
    ViaFallback(&'a str),
    UnlinkedResource(&'a str),
    KeyMissing(&'a str),
    KeyLater {
        key: &'a str,
        release: &'a str,
    },
    KeyUnknown {
        key: &'a str,
        file: &'a str,
    },
    TrackA,
    NotLowercase(&'a str),
    Empty,
    About,
    Index,
    Status,
    Export,
    Mcp,
    Decide,
    DecideItemHelp,
    DecideDryRunHelp,
    DecideListHelp,
    DecidePruneHelp,
    DecidePruned(usize),
    CallRepeated(&'a str),
    DecideNothingToPrune,
    DecideNoItems,
    DecideUnknownItem(&'a str),
    DecideNoCandidates,
    DecidePrompt(usize),
    DecideRetry(usize),
    DecideNothing,
    DecideWrote(usize),
    DecideDryRun,
    DecideDeciderHelp,
    DecideShowRequestHelp,
    DecideTooMany {
        count: usize,
        max: usize,
    },
    DecideFailed(&'a str),
    /// `confidence` is printed when the protocol gives one (`decisions`), never recorded.
    DecideProposal {
        candidate: &'a str,
        p: f64,
        confidence: Option<f64>,
        by: &'a str,
    },
    DecideSuggestion {
        candidate: &'a str,
        p: f64,
        confidence: Option<f64>,
        min_p: f64,
    },
    DecideInvalid(&'a str),
    AnswerUnreadable(&'a str),
    AnswerChoice {
        choice: &'a str,
        keys: &'a str,
    },
    AnswerProbability(f64),
    AnswerBy(&'a str),
    AnswerType(&'a str),
    AnswerConfidence(f64),
    AnswerKey(&'a str),
    AnswerSum {
        sum: f64,
        tolerance: f64,
    },
    AnswerNoProbability(&'a str),
    AnswerMissing,
    AnswerUnknown(&'a str),
    DecideMinP,
    DecideMinPHelp,
    DecideFailures(usize),
    DeciderInvalid(&'a str),
    DeciderProtocol,
    DeciderHttps,
    DecisionPHuman,
    DecisionPMissing,
    DecisionPRange,
    HttpStatus {
        status: u16,
        message: Option<&'a str>,
    },
    AnswerNoChoices,
}

impl Msg<'_> {
    pub fn text(&self) -> String {
        let pt = PT.load(Ordering::Relaxed);
        let pick = |en: String, pt_br: String| if pt { pt_br } else { en };
        match self {
            Msg::NotIndexed => pick("not indexed".into(), "não indexado".into()),
            Msg::OtherVersion => pick(
                "indexed by another version; rebuilt on next use".into(),
                "indexado por outra versão; reconstruído no próximo uso".into(),
            ),
            Msg::Indexed(n) => pick(
                format!("indexed, {n} files"),
                format!("indexado, {n} arquivos"),
            ),
            Msg::MissingPeer { repo, named_by } => pick(
                format!("missing peer: {repo}, named by {named_by}"),
                format!("par não encontrado: {repo}, citado por {named_by}"),
            ),
            Msg::StaleDecision { call, target, cause } => {
                let (en, pt_br) = match cause {
                    Cause::CallGone => ("call site gone", "chamada removida"),
                    Cause::ItemNotOpen => ("item no longer open", "item não está mais aberto"),
                    Cause::TargetGone => ("target gone", "alvo removido"),
                    Cause::NotCandidate => {
                        ("target no longer a candidate", "alvo não é mais candidato")
                    }
                };
                pick(
                    format!("stale decision: {call} -> {target} ({en})"),
                    format!("decisão obsoleta: {call} -> {target} ({pt_br})"),
                )
            }
            Msg::WaitingDecision { call, target, peer } => pick(
                format!("decision waiting on missing peer {peer}: {call} -> {target}"),
                format!("decisão aguardando o par ausente {peer}: {call} -> {target}"),
            ),
            Msg::TooManyRepos { found, max } => pick(
                format!("the workspace has {found} repositories; v0.1 reads at most {max}"),
                format!("o workspace tem {found} repositórios; a v0.1 lê no máximo {max}"),
            ),
            Msg::ViaUnmatched { contract, via } => pick(
                format!("{contract}: `via` {via} matches no symbol, ignored"),
                format!("{contract}: `via` {via} não corresponde a nenhum símbolo, ignorado"),
            ),
            Msg::ViaFallback(contract) => pick(
                format!("{contract}: no `via` matches a symbol, so the repository as a whole is the side"),
                format!("{contract}: nenhum `via` corresponde a um símbolo, então o repositório inteiro é o lado"),
            ),
            Msg::UnlinkedResource(contract) => pick(
                format!("{contract} is unlinked: no writer and reader in different repositories"),
                format!("{contract} sem ligação: nenhum escritor e leitor em repositórios diferentes"),
            ),
            Msg::KeyMissing(key) => pick(format!("`{key}` is missing"), format!("falta `{key}`")),
            Msg::KeyLater { key, release } => pick(
                format!("`{key}` is not in v0.1; {release}"),
                format!("`{key}` não existe na v0.1; {release}"),
            ),
            Msg::KeyUnknown { key, file } => pick(
                format!("`{key}` is not a {file} key"),
                format!("`{key}` não é uma chave do {file}"),
            ),
            Msg::TrackA => pick(
                "the documents release of track A adds it (spec #21)".into(),
                "a versão de documentos da trilha A a adiciona (spec #21)".into(),
            ),
            Msg::NotLowercase(value) => pick(
                format!(
                    "`{value}` is not a lowercase identifier \
                     (ASCII letters, digits and hyphens, starting with a letter)"
                ),
                format!(
                    "`{value}` não é um identificador em minúsculas \
                     (letras ASCII, dígitos e hífens, começando por uma letra)"
                ),
            ),
            Msg::Empty => pick("is empty".into(), "está vazio".into()),
            Msg::About => pick(
                "LLM-free code graph for agents and people".into(),
                "Grafo de código sem LLM para agentes e pessoas".into(),
            ),
            Msg::Index => pick(
                "Build the indexes of the workspace of the repository in the current directory."
                    .into(),
                "Constrói os índices do workspace do repositório no diretório atual.".into(),
            ),
            Msg::Status => pick(
                "Show the workspace: each repository, its index state and any peer not found."
                    .into(),
                "Mostra o workspace: cada repositório, o estado do índice e os pares não encontrados."
                    .into(),
            ),
            Msg::Export => pick(
                "Print the index as sorted JSON, with the links that touch it.".into(),
                "Imprime o índice como JSON ordenado, com os links que o tocam.".into(),
            ),
            Msg::Mcp => pick(
                "Serve the repository in the current directory to agents over MCP (stdio).".into(),
                "Serve o repositório do diretório atual a agentes por MCP (stdio).".into(),
            ),
            Msg::Decide => pick(
                "Resolve the open items of this repository, ambiguous links and dynamic URLs, \
                 and record the picks in meada.decisions.toml."
                    .into(),
                "Resolve os itens abertos deste repositório, links ambíguos e URLs dinâmicas, \
                 e registra as escolhas em meada.decisions.toml."
                    .into(),
            ),
            Msg::DecideItemHelp => pick(
                "Work on this item only, even one already decided, to redo it (a call-site id, \
                 as `meada decide --list` or the MCP `search` tool prints it)."
                    .into(),
                "Trabalha só neste item, mesmo que já decidido, para refazê-lo (o id da chamada, \
                 como `meada decide --list` ou a ferramenta `search` do MCP o imprime)."
                    .into(),
            ),
            Msg::DecideDryRunHelp => pick(
                "Print the decisions the run would write and leave the file unchanged.".into(),
                "Imprime as decisões que a execução gravaria e deixa o arquivo como está.".into(),
            ),
            Msg::DecideListHelp => pick(
                "Print the open items; neither ask nor write.".into(),
                "Imprime os itens abertos; não pergunta nem grava.".into(),
            ),
            Msg::DecidePruneHelp => pick(
                "Remove the stale decisions from meada.decisions.toml; keep those waiting on a \
                 missing peer."
                    .into(),
                "Remove as decisões obsoletas de meada.decisions.toml; mantém as que aguardam \
                 um par ausente."
                    .into(),
            ),
            Msg::DecidePruned(n) => pick(
                format!(
                    "pruned {n} stale decision{} from meada.decisions.toml",
                    if *n == 1 { "" } else { "s" }
                ),
                format!(
                    "{n} {} de meada.decisions.toml",
                    if *n == 1 { "decisão obsoleta removida" } else { "decisões obsoletas removidas" }
                ),
            ),
            Msg::CallRepeated(call) => pick(
                format!("`call`: `{call}` is decided twice, one entry per call"),
                format!("`call`: `{call}` aparece duas vezes, uma entrada por chamada"),
            ),
            Msg::DecideNothingToPrune => {
                pick("nothing to prune".into(), "nada a remover".into())
            }
            Msg::DecideNoItems => pick("no open items".into(), "nenhum item aberto".into()),
            Msg::DecideUnknownItem(id) => pick(
                format!("`{id}` is not an open or decided item of this repository"),
                format!("`{id}` não é um item aberto nem decidido deste repositório"),
            ),
            Msg::DecideNoCandidates => pick(
                "no candidate fits; skipped".into(),
                "nenhum candidato serve; pulado".into(),
            ),
            Msg::DecidePrompt(n) => pick(
                format!("pick 1-{n}, or s to skip:"),
                format!("escolha 1-{n}, ou s para pular:"),
            ),
            Msg::DecideRetry(n) => pick(
                format!("enter a number from 1 to {n}, or s to skip:"),
                format!("digite um número de 1 a {n}, ou s para pular:"),
            ),
            Msg::DecideNothing => pick("nothing decided".into(), "nada decidido".into()),
            Msg::DecideWrote(n) => pick(
                format!(
                    "wrote {n} decision{} to meada.decisions.toml",
                    if *n == 1 { "" } else { "s" }
                ),
                format!(
                    "{n} {} em meada.decisions.toml",
                    if *n == 1 { "decisão gravada" } else { "decisões gravadas" }
                ),
            ),
            Msg::DecideDryRun => pick(
                "dry run, meada.decisions.toml would be:".into(),
                "execução a seco, meada.decisions.toml ficaria assim:".into(),
            ),
            Msg::DecideDeciderHelp => pick(
                "Ask the model the MEADA_DECIDER_* variables configure instead of a person."
                    .into(),
                "Pergunta ao modelo configurado pelas variáveis MEADA_DECIDER_* em vez de a uma \
                 pessoa."
                    .into(),
            ),
            Msg::DecideShowRequestHelp => pick(
                "With --decider: print every request the run would send, and send none.".into(),
                "Com --decider: imprime cada requisição que a execução enviaria, e não envia \
                 nenhuma."
                    .into(),
            ),
            Msg::DecideTooMany { count, max } => pick(
                format!("{count} candidates, more than {max}; skipped in decider mode"),
                format!("{count} candidatos, mais de {max}; pulado no modo decisor"),
            ),
            Msg::DecideFailed(error) => pick(
                format!("request failed: {error}"),
                format!("a requisição falhou: {error}"),
            ),
            Msg::DecideProposal {
                candidate,
                p,
                confidence,
                by,
            } => {
                let (en, pt_br) = confidence_of(*confidence);
                pick(
                    format!("proposed: {candidate} (p={p:.2}{en}, by {by})"),
                    format!("proposto: {candidate} (p={p:.2}{pt_br}, por {by})"),
                )
            }
            Msg::DecideSuggestion {
                candidate,
                p,
                confidence,
                min_p,
            } => {
                let (en, pt_br) = confidence_of(*confidence);
                pick(
                    format!(
                        "suggestion, not written: {candidate} (p={p:.2}{en}, below --min-p \
                         {min_p:.2})"
                    ),
                    format!(
                        "sugestão, não gravada: {candidate} (p={p:.2}{pt_br}, abaixo de --min-p \
                         {min_p:.2})"
                    ),
                )
            }
            Msg::DecideInvalid(reason) => pick(
                format!("invalid answer: {reason}"),
                format!("resposta inválida: {reason}"),
            ),
            Msg::AnswerUnreadable(why) => pick(
                format!("unreadable: {why}"),
                format!("ilegível: {why}"),
            ),
            Msg::AnswerChoice { choice, keys } => pick(
                format!("choice `{choice}` is not one of the offered keys ({keys})"),
                format!("a escolha `{choice}` não é uma das chaves oferecidas ({keys})"),
            ),
            Msg::AnswerProbability(p) => pick(
                format!("probability {p} is not between 0 and 1"),
                format!("a probabilidade {p} não está entre 0 e 1"),
            ),
            Msg::AnswerBy(by) => pick(
                format!("model id {by:?} cannot be recorded"),
                format!("o id de modelo {by:?} não pode ser gravado"),
            ),
            Msg::AnswerType(kind) => pick(
                format!("answer type `{kind}` is not `choice`"),
                format!("o tipo de resposta `{kind}` não é `choice`"),
            ),
            Msg::AnswerConfidence(c) => pick(
                format!("confidence {c} is not between 0 and 1"),
                format!("a confiança {c} não está entre 0 e 1"),
            ),
            Msg::AnswerKey(key) => pick(
                format!("probability key `{key}` is not an offered key"),
                format!("a chave de probabilidade `{key}` não é uma chave oferecida"),
            ),
            Msg::AnswerSum { sum, tolerance } => pick(
                format!("probabilities sum to {sum:.4}, not 1 (tolerance {tolerance})"),
                format!("as probabilidades somam {sum:.4}, não 1 (tolerância {tolerance})"),
            ),
            Msg::AnswerNoProbability(choice) => pick(
                format!("no probability for the choice `{choice}`"),
                format!("nenhuma probabilidade para a escolha `{choice}`"),
            ),
            Msg::AnswerMissing => pick(
                "no answer for this item".into(),
                "nenhuma resposta para este item".into(),
            ),
            Msg::AnswerUnknown(name) => pick(
                format!("answer to unknown question `{name}`"),
                format!("resposta a uma pergunta desconhecida `{name}`"),
            ),
            Msg::DecideMinP => pick(
                "`--min-p` must be a number from 0 to 1".into(),
                "`--min-p` deve ser um número de 0 a 1".into(),
            ),
            Msg::DecideMinPHelp => pick(
                format!("With --decider: write a model decision only from this probability up (default {}).", crate::decider::MIN_P),
                format!("Com --decider: grava a decisão do modelo só a partir desta probabilidade (padrão {}).", crate::decider::MIN_P),
            ),
            Msg::DecideFailures(n) => pick(
                format!("{n} item(s) failed, see above; the other decisions were handled"),
                format!("{n} item(ns) falharam, veja acima; as demais decisões foram tratadas"),
            ),
            Msg::DeciderInvalid(var) => pick(
                format!("`{var}` is not valid"),
                format!("`{var}` não é válida"),
            ),
            Msg::DeciderProtocol => pick(
                "`MEADA_DECIDER_PROTOCOL` must be `decisions` or `chat`".into(),
                "`MEADA_DECIDER_PROTOCOL` deve ser `decisions` ou `chat`".into(),
            ),
            Msg::DeciderHttps => pick(
                "`MEADA_DECIDER_URL` must use https, or http to a loopback address".into(),
                "`MEADA_DECIDER_URL` deve usar https, ou http para um endereço de loopback".into(),
            ),
            Msg::DecisionPHuman => pick(
                "`p` is for a model, not for `human`".into(),
                "`p` é para um modelo, não para `human`".into(),
            ),
            Msg::DecisionPMissing => pick(
                "`p` is missing for a model".into(),
                "falta `p` para um modelo".into(),
            ),
            Msg::DecisionPRange => pick(
                "`p` must be between 0 and 1".into(),
                "`p` deve estar entre 0 e 1".into(),
            ),
            Msg::HttpStatus { status, message } => {
                let message = message.map(|m| format!(": {m}")).unwrap_or_default();
                pick(
                    format!("http status: {status}{message}"),
                    format!("status http: {status}{message}"),
                )
            }
            Msg::AnswerNoChoices => pick(
                "unreadable: no choices".into(),
                "ilegível: nenhuma escolha em `choices`".into(),
            ),
        }
    }
}

/// The `, confidence 0.80` part of a proposal line, in English and in pt-BR; empty without one.
fn confidence_of(confidence: Option<f64>) -> (String, String) {
    match confidence {
        Some(c) => (
            format!(", confidence {c:.2}"),
            format!(", confiança {c:.2}"),
        ),
        None => Default::default(),
    }
}
