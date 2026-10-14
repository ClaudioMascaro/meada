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
    About,
    Index,
    Status,
    Export,
    Mcp,
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
        }
    }
}
