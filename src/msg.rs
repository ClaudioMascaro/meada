//! The CLI's message catalog: pt-BR and English, one match arm per message. The language comes
//! from `LANG` (ADR 0008): pt-BR when it starts with `pt`, English otherwise. Only the CLI calls
//! `init`, so the MCP server and the library tests stay in English.

use std::sync::atomic::{AtomicBool, Ordering};

static PT: AtomicBool = AtomicBool::new(false);

/// Picks the language from the value of `LANG`; `None` is English.
pub fn init(lang: Option<&str>) {
    PT.store(lang.is_some_and(|l| l.starts_with("pt")), Ordering::Relaxed);
}

pub enum Msg<'a> {
    NotIndexed,
    OtherVersion,
    Indexed(i64),
    MissingPeer { repo: &'a str, named_by: &'a str },
    TooManyRepos { found: usize, max: usize },
    KeyMissing(&'a str),
    KeyLater { key: &'a str, release: &'a str },
    KeyUnknown { key: &'a str, file: &'a str },
    TrackA,
    About,
    Index,
    Status,
    Export,
    Mcp,
    Decide,
    DecideItemHelp,
    DecideDryRunHelp,
    DecideListHelp,
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
    DecideTooMany { count: usize, max: usize },
    DecideFailed(&'a str),
    DeciderInvalid(&'a str),
    DeciderProtocol,
    DeciderHttps,
    DeciderLater,
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
            Msg::TooManyRepos { found, max } => pick(
                format!("the workspace has {found} repositories; v0.1 reads at most {max}"),
                format!("o workspace tem {found} repositórios; a v0.1 lê no máximo {max}"),
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
                "track A adds it (resources and gateway)".into(),
                "a trilha A a adiciona (recursos e gateway)".into(),
            ),
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
                "Work on this item only (a call-site id, as `meada search` prints it).".into(),
                "Trabalha só neste item (o id da chamada, como o `meada search` imprime).".into(),
            ),
            Msg::DecideDryRunHelp => pick(
                "Print the decisions the run would write and leave the file unchanged.".into(),
                "Imprime as decisões que a execução gravaria e deixa o arquivo como está.".into(),
            ),
            Msg::DecideListHelp => pick(
                "Print the open items; neither ask nor write.".into(),
                "Imprime os itens abertos; não pergunta nem grava.".into(),
            ),
            Msg::DecideNoItems => pick("no open items".into(), "nenhum item aberto".into()),
            Msg::DecideUnknownItem(id) => pick(
                format!("`{id}` is not an open item of this repository"),
                format!("`{id}` não é um item aberto deste repositório"),
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
            Msg::DeciderLater => pick(
                "the `decisions` protocol of `MEADA_DECIDER_PROTOCOL` is not available yet; \
                 use `chat`"
                    .into(),
                "o protocolo `decisions` de `MEADA_DECIDER_PROTOCOL` ainda não está disponível; \
                 use `chat`"
                    .into(),
            ),
        }
    }
}
