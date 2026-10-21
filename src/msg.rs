//! The CLI's message catalog: pt-BR and English, one match arm per message. The language comes
//! from `LANG` (ADR 0008): pt-BR when it starts with `pt`, English otherwise. Only the CLI calls
//! `init`, so the MCP server and the library tests stay in English.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::decisions::Cause;
use crate::document::Format;

static PT: AtomicBool = AtomicBool::new(false);

/// Picks the language from the value of `LANG`; `None` is English.
pub fn init(lang: Option<&str>) {
    PT.store(lang.is_some_and(|l| l.starts_with("pt")), Ordering::Relaxed);
}

/// Where an index that cannot be read is fixed (`index::open_read`).
#[derive(Clone, Copy)]
pub enum Fix<'a> {
    /// `meada index --workspace <manifest>`.
    Index(&'a str),
    /// A node's generation (`--generations`, feature `snapshot`) that only a publish from this
    /// build and a pull replace.
    Generation(&'a str),
}

pub enum Msg<'a> {
    NotIndexed,
    OtherVersion,
    Indexed(i64),
    OtherVersionWorkspace,
    UnsafeIdentity,
    LocalIdentity,
    ListedTwice,
    NoMirror(&'a str),
    NotACheckout(&'a str),
    DubiousOwner(&'a str),
    IdentityMismatch {
        found: &'a str,
        repo: &'a str,
    },
    NotListed {
        repo: &'a str,
        served: &'a str,
    },
    IndexInMirror {
        index: &'a str,
        mirror: &'a str,
    },
    WorkspaceHelp,
    ListenHelp,
    RepoHelp,
    IgnoredBlock {
        block: &'a str,
        repo: &'a str,
    },
    IndexMissing {
        repo: &'a str,
        fix: Fix<'a>,
    },
    IndexOtherVersion {
        repo: &'a str,
        fix: Fix<'a>,
    },
    StateLocked,
    #[cfg(feature = "snapshot")]
    IndexSchemaObject {
        repo: &'a str,
        kind: &'a str,
        name: &'a str,
    },
    IndexInterrupted {
        repo: &'a str,
        fix: Fix<'a>,
    },
    IndexLocked {
        repo: &'a str,
        index: &'a str,
        timeout: std::time::Duration,
    },
    IndexNotReplaced {
        index: &'a str,
        error: &'a str,
    },
    RebuildLock {
        lock: &'a str,
        error: &'a str,
    },
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
    /// The missing reads of the repository's consumers against the routes they call (spec #23).
    Drift(usize),
    DocumentUnreadable {
        path: &'a str,
        line: usize,
        error: &'a str,
    },
    /// A document that counts: its format and version, and how many routes (OpenAPI) or
    /// channels (AsyncAPI) it lists.
    DocumentRead {
        path: &'a str,
        format: Format,
        version: &'a str,
        count: usize,
    },
    /// A path `[serve] documents` lists that is not a file.
    DocumentMissing {
        path: &'a str,
    },
    /// `found` is the format field and version the document has, such as `swagger 2.0`.
    DocumentFormat {
        path: &'a str,
        found: Option<&'a str>,
    },
    /// `found` is the format field and version the document has, such as `asyncapi 1.2.0`.
    DocumentAsyncFormat {
        path: &'a str,
        found: Option<&'a str>,
    },
    /// A channel on a server with this protocol, which has no broker kind: ignored.
    DocumentProtocol {
        path: &'a str,
        protocol: &'a str,
    },
    /// A channel name with parameters: ignored.
    DocumentParameters {
        path: &'a str,
        channel: &'a str,
    },
    /// An AsyncAPI document with channels and no root `servers`: its channels are ignored.
    DocumentNoServers {
        path: &'a str,
    },
    /// A server a channel uses has no `protocol`: its channels are ignored on it.
    DocumentNoProtocol {
        path: &'a str,
        server: &'a str,
    },
    /// A channel lists a server the document does not have.
    DocumentUnknownServer {
        path: &'a str,
        channel: &'a str,
        server: &'a str,
    },
    DuplicateRoute {
        route: &'a str,
        path: &'a str,
        first: &'a str,
    },
    ExternalRef {
        path: &'a str,
        reference: &'a str,
    },
    /// A path item that is a reference to another file or a URL.
    ExternalPathItem {
        path: &'a str,
        reference: &'a str,
    },
    KeyMissing(&'a str),
    NotInRepository(&'a str),
    KeyUnknown {
        key: &'a str,
        file: &'a str,
    },
    NotLowercase(&'a str),
    NoLeadingSlash(&'a str),
    Empty,
    NeedsFeature {
        section: &'a str,
        feature: &'a str,
    },
    #[cfg(feature = "valkey")]
    ValkeyUrlInvalid,
    #[cfg(feature = "valkey")]
    ValkeyUrlPassword,
    #[cfg(feature = "valkey")]
    ValkeyUrlPlain,
    #[cfg(feature = "valkey")]
    ValkeyUrlInsecure,
    #[cfg(feature = "valkey")]
    ValkeyClusterPlain,
    #[cfg(feature = "valkey")]
    ValkeyBoth,
    #[cfg(feature = "valkey")]
    ValkeyPrefixBraces,
    #[cfg(feature = "valkey")]
    ValkeyEnvUnset(&'a str),
    #[cfg(feature = "valkey")]
    ValkeyTimeout(u64),
    #[cfg(feature = "valkey")]
    ValkeyReachable,
    #[cfg(feature = "valkey")]
    ValkeyUnreachable(&'a str),
    #[cfg(feature = "snapshot")]
    SnapshotEndpointInvalid,
    #[cfg(feature = "snapshot")]
    SnapshotEndpointUser,
    #[cfg(feature = "snapshot")]
    SnapshotEndpointPlain,
    #[cfg(feature = "snapshot")]
    SnapshotPathStyle,
    #[cfg(feature = "snapshot")]
    SnapshotRegion,
    #[cfg(feature = "snapshot")]
    SnapshotBucket,
    #[cfg(feature = "snapshot")]
    SnapshotPrefix,
    #[cfg(feature = "snapshot")]
    SnapshotBoth(&'a str),
    #[cfg(feature = "snapshot")]
    SnapshotStaticOnly,
    #[cfg(feature = "snapshot")]
    SnapshotEnvUnset(&'a str),
    #[cfg(feature = "snapshot")]
    SnapshotNoSection(&'a str),
    #[cfg(feature = "snapshot")]
    SnapshotBehind {
        repo: &'a str,
        manifest: &'a str,
    },
    #[cfg(feature = "snapshot")]
    SnapshotDirty {
        repo: &'a str,
        mirror: &'a str,
    },
    #[cfg(feature = "snapshot")]
    SnapshotPodUrl,
    #[cfg(feature = "snapshot")]
    SnapshotRefused(&'a str),
    #[cfg(feature = "snapshot")]
    SnapshotObjectBad(&'a str),
    #[cfg(feature = "snapshot")]
    SnapshotChanged(&'a str),
    #[cfg(feature = "snapshot")]
    Snapshot,
    #[cfg(feature = "snapshot")]
    SnapshotPublish,
    #[cfg(feature = "snapshot")]
    SnapshotPull,
    #[cfg(feature = "snapshot")]
    SnapshotEvery,
    #[cfg(feature = "snapshot")]
    SnapshotNoLocalDir(&'a str),
    #[cfg(feature = "snapshot")]
    SnapshotPulling(&'a str),
    #[cfg(feature = "snapshot")]
    SnapshotNoCurrent(&'a str),
    #[cfg(feature = "snapshot")]
    SnapshotBadId(&'a str),
    #[cfg(feature = "snapshot")]
    SnapshotBadBundleKey(&'a str),
    #[cfg(feature = "snapshot")]
    SnapshotPointerBad(&'a str),
    #[cfg(feature = "snapshot")]
    SnapshotNotDocument(&'a str),
    #[cfg(feature = "snapshot")]
    SnapshotIdentityChars,
    #[cfg(feature = "snapshot")]
    SnapshotBytesCeiling(u64),
    #[cfg(feature = "snapshot")]
    SnapshotEntriesCeiling(u64),
    #[cfg(feature = "snapshot")]
    SnapshotEntryHeader(u64),
    #[cfg(feature = "snapshot")]
    SnapshotEntryPath,
    #[cfg(feature = "snapshot")]
    SnapshotEntryNested,
    #[cfg(feature = "snapshot")]
    SnapshotEntryLink,
    #[cfg(feature = "snapshot")]
    SnapshotEntryThrough,
    #[cfg(feature = "snapshot")]
    SnapshotEntrySpecial,
    #[cfg(feature = "snapshot")]
    SnapshotNoGit,
    #[cfg(feature = "snapshot")]
    SnapshotHead {
        found: &'a str,
        head: &'a str,
    },
    #[cfg(feature = "snapshot")]
    SnapshotNoneHeld(&'a str),
    #[cfg(feature = "snapshot")]
    SnapshotTimeout(u64),
    #[cfg(feature = "snapshot")]
    SnapshotStatusCurrent(Option<&'a str>),
    #[cfg(feature = "snapshot")]
    SnapshotStatusHeld(&'a [String]),
    #[cfg(feature = "snapshot")]
    SnapshotStoreReachable(&'a str),
    #[cfg(feature = "snapshot")]
    SnapshotStoreUnreachable(&'a str),
    #[cfg(feature = "snapshot")]
    GenerationsHelp,
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
            Msg::OtherVersionWorkspace => pick(
                "indexed by another version; rebuilt by meada index --workspace".into(),
                "indexado por outra versão; reconstruído por meada index --workspace".into(),
            ),
            Msg::UnsafeIdentity => pick(
                "not a repository identity: a segment is empty, `.` or `..`".into(),
                "não é uma identidade de repositório: um segmento está vazio ou é `.` ou `..`"
                    .into(),
            ),
            Msg::LocalIdentity => pick(
                "a local identity: server mode serves repositories by their remote".into(),
                "uma identidade local: o modo servidor serve repositórios pelo remoto".into(),
            ),
            Msg::ListedTwice => pick("listed twice".into(), "listado duas vezes".into()),
            Msg::NoMirror(path) => pick(
                format!("`{path}` does not exist"),
                format!("`{path}` não existe"),
            ),
            Msg::NotACheckout(path) => pick(
                format!("`{path}` is not the root of a checkout with a working tree"),
                format!("`{path}` não é a raiz de um checkout com árvore de trabalho"),
            ),
            Msg::DubiousOwner(path) => pick(
                format!(
                    "git refuses `{path}`, which another user owns: Meada needs git 2.38 or \
                     later to read it, or `safe.directory` set in the system git configuration"
                ),
                format!(
                    "o git recusa `{path}`, que pertence a outro usuário: o Meada precisa do git \
                     2.38 ou posterior para lê-lo, ou de `safe.directory` na configuração de \
                     sistema do git"
                ),
            ),
            Msg::IdentityMismatch { found, repo } => pick(
                format!("the mirror's remote is `{found}`, not `{repo}`"),
                format!("o remoto do espelho é `{found}`, não `{repo}`"),
            ),
            Msg::NotListed { repo, served } => pick(
                format!("`{repo}` is not listed in the workspace manifest; it serves {served}"),
                format!("`{repo}` não está listado no manifesto do workspace; ele serve {served}"),
            ),
            Msg::ListenHelp => pick(
                "Server mode: serve over MCP streamable HTTP at host:port, :port or port \
                 (loopback without a host), with --workspace"
                    .into(),
                "Modo servidor: serve por MCP streamable HTTP em host:porta, :porta ou porta \
                 (loopback sem host), com --workspace"
                    .into(),
            ),
            Msg::RepoHelp => pick(
                "Server mode: the identity of the repository to export".into(),
                "Modo servidor: a identidade do repositório a exportar".into(),
            ),
            Msg::IgnoredBlock { block, repo } => pick(
                format!(
                    "manifest block ignored: {block} of {repo}, whose mirror has its own meada.toml"
                ),
                format!(
                    "bloco do manifesto ignorado: {block} de {repo}, cujo espelho tem seu próprio \
                     meada.toml"
                ),
            ),
            Msg::IndexMissing { repo, fix } => match fix {
                Fix::Index(manifest) => pick(
                    format!("{repo}: not indexed; run meada index --workspace {manifest}"),
                    format!("{repo}: não indexado; rode meada index --workspace {manifest}"),
                ),
                Fix::Generation(id) => pick(
                    format!("{repo}: no index in generation {id}; run meada snapshot pull"),
                    format!("{repo}: sem índice na geração {id}; rode meada snapshot pull"),
                ),
            },
            Msg::IndexOtherVersion { repo, fix } => match fix {
                Fix::Index(manifest) => pick(
                    format!(
                        "{repo}: indexed by another version; run meada index --workspace \
                         {manifest}"
                    ),
                    format!(
                        "{repo}: indexado por outra versão; rode meada index --workspace \
                         {manifest}"
                    ),
                ),
                Fix::Generation(id) => pick(
                    format!(
                        "{repo}: indexed by another version in generation {id}; publish a \
                         generation from this build, then run meada snapshot pull"
                    ),
                    format!(
                        "{repo}: indexado por outra versão na geração {id}; publique uma geração \
                         deste build e rode meada snapshot pull"
                    ),
                ),
            },
            Msg::IndexInterrupted { repo, fix } => match fix {
                Fix::Index(manifest) => pick(
                    format!(
                        "{repo}: its index holds an interrupted commit; run meada index \
                         --workspace {manifest}"
                    ),
                    format!(
                        "{repo}: seu índice guarda um commit interrompido; rode meada index \
                         --workspace {manifest}"
                    ),
                ),
                Fix::Generation(id) => pick(
                    format!(
                        "{repo}: its index in generation {id} holds an interrupted commit; \
                         publish a new generation, then run meada snapshot pull"
                    ),
                    format!(
                        "{repo}: seu índice na geração {id} guarda um commit interrompido; \
                         publique uma nova geração e rode meada snapshot pull"
                    ),
                ),
            },
            #[cfg(feature = "snapshot")]
            Msg::IndexSchemaObject { repo, kind, name } => pick(
                format!(
                    "{repo}: index.db: the {kind} {name} is not one this build's schema creates"
                ),
                format!(
                    "{repo}: index.db: o {kind} {name} não é um dos que o esquema deste build cria"
                ),
            ),
            Msg::StateLocked => pick(
                "locked by a commit in progress past the busy timeout".into(),
                "travado por um commit em andamento além do tempo de espera".into(),
            ),
            Msg::IndexLocked {
                repo,
                index,
                timeout,
            } => pick(
                format!(
                    "{repo}: the index `{index}` stayed locked by a commit in progress for \
                     longer than {timeout:?}"
                ),
                format!(
                    "{repo}: o índice `{index}` ficou travado por um commit em andamento por \
                     mais de {timeout:?}"
                ),
            ),
            Msg::IndexNotReplaced { index, error } => pick(
                format!(
                    "the index `{index}` could not be replaced by its rebuild ({error}); it is \
                     left as it was"
                ),
                format!(
                    "o índice `{index}` não pôde ser substituído pela reconstrução ({error}); \
                     ficou como estava"
                ),
            ),
            Msg::RebuildLock { lock, error } => pick(
                format!("cannot take the rebuild lock `{lock}`: {error}"),
                format!("não foi possível pegar a trava de reconstrução `{lock}`: {error}"),
            ),
            Msg::IndexInMirror { index, mirror } => pick(
                format!("its index directory `{index}` would be inside the mirror `{mirror}`"),
                format!("seu diretório de índice `{index}` ficaria dentro do espelho `{mirror}`"),
            ),
            Msg::WorkspaceHelp => pick(
                "Server mode: every repository the workspace manifest at this path lists".into(),
                "Modo servidor: todo repositório que o manifesto do workspace neste caminho lista"
                    .into(),
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
            Msg::Drift(n) => pick(
                format!("shape drift, missing reads: {n}"),
                format!("drift de formato, leituras ausentes: {n}"),
            ),
            Msg::DocumentUnreadable { path, line, error } => pick(
                format!("{path}:{line}: unreadable document ({error}), ignored"),
                format!("{path}:{line}: documento ilegível ({error}), ignorado"),
            ),
            Msg::DocumentRead {
                path,
                format,
                version,
                count,
            } => {
                let name = format.name();
                match format {
                    Format::Asyncapi => pick(
                        format!("{path}: {name} {version}, channels: {count}"),
                        format!("{path}: {name} {version}, canais: {count}"),
                    ),
                    Format::Openapi => pick(
                        format!("{path}: {name} {version}, routes: {count}"),
                        format!("{path}: {name} {version}, rotas: {count}"),
                    ),
                }
            }
            Msg::DocumentMissing { path } => pick(
                format!("{path}: listed in `serve.documents`, not found"),
                format!("{path}: listado em `serve.documents`, não encontrado"),
            ),
            Msg::DocumentFormat { path, found } => pick(
                format!(
                    "{path}: not OpenAPI 3.0 or 3.1 ({}), ignored",
                    found.unwrap_or("no `openapi` field")
                ),
                format!(
                    "{path}: não é OpenAPI 3.0 nem 3.1 ({}), ignorado",
                    found.unwrap_or("sem o campo `openapi`")
                ),
            ),
            Msg::DocumentAsyncFormat { path, found } => pick(
                format!(
                    "{path}: not AsyncAPI 2.x or 3.0 ({}), ignored",
                    found.unwrap_or("no `asyncapi` field")
                ),
                format!(
                    "{path}: não é AsyncAPI 2.x nem 3.0 ({}), ignorado",
                    found.unwrap_or("sem o campo `asyncapi`")
                ),
            ),
            Msg::DocumentProtocol { path, protocol } => pick(
                format!("{path}: server protocol `{protocol}` has no broker kind, its channels are ignored"),
                format!("{path}: o protocolo `{protocol}` do servidor não tem tipo de broker, seus canais são ignorados"),
            ),
            Msg::DocumentParameters { path, channel } => pick(
                format!("{path}: channel `{channel}` has parameters, ignored"),
                format!("{path}: o canal `{channel}` tem parâmetros, ignorado"),
            ),
            Msg::DocumentNoServers { path } => pick(
                format!("{path}: no root `servers`, so no channel has a broker kind: its channels are ignored"),
                format!("{path}: sem `servers` na raiz, nenhum canal tem tipo de broker: seus canais são ignorados"),
            ),
            Msg::DocumentNoProtocol { path, server } => pick(
                format!("{path}: server `{server}` has no `protocol`, its channels are ignored"),
                format!("{path}: o servidor `{server}` não tem `protocol`, seus canais são ignorados"),
            ),
            Msg::DocumentUnknownServer {
                path,
                channel,
                server,
            } => pick(
                format!("{path}: channel `{channel}` names server `{server}`, which the document does not have, ignored there"),
                format!("{path}: o canal `{channel}` cita o servidor `{server}`, que o documento não tem, ignorado ali"),
            ),
            Msg::DuplicateRoute { route, path, first } => pick(
                format!("{route} in {path} is also in {first}: one route"),
                format!("{route} em {path} também está em {first}: uma rota só"),
            ),
            Msg::ExternalRef { path, reference } => pick(
                format!("{path}: external reference {reference} not followed, its part of the shape is open"),
                format!("{path}: referência externa {reference} não seguida, sua parte do shape fica aberta"),
            ),
            Msg::ExternalPathItem { path, reference } => pick(
                format!("{path}: external path item reference {reference} not followed, its routes are not read"),
                format!("{path}: referência externa de path item {reference} não seguida, suas rotas não são lidas"),
            ),
            Msg::KeyMissing(key) => pick(format!("`{key}` is missing"), format!("falta `{key}`")),
            Msg::NotInRepository(path) => pick(
                format!("`{path}` is not a path inside the repository"),
                format!("`{path}` não é um caminho dentro do repositório"),
            ),
            Msg::KeyUnknown { key, file } => pick(
                format!("`{key}` is not a {file} key"),
                format!("`{key}` não é uma chave do {file}"),
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
            Msg::NoLeadingSlash(value) => pick(
                format!("`{value}` must start with `/`"),
                format!("`{value}` deve começar com `/`"),
            ),
            Msg::Empty => pick("is empty".into(), "está vazio".into()),
            Msg::NeedsFeature { section, feature } => pick(
                format!("[{section}] needs a Meada built with the {feature} feature"),
                format!("[{section}] exige um Meada compilado com a feature {feature}"),
            ),
            #[cfg(feature = "valkey")]
            Msg::ValkeyUrlInvalid => pick(
                "is not a redis:// or rediss:// URL".into(),
                "não é uma URL redis:// ou rediss://".into(),
            ),
            #[cfg(feature = "valkey")]
            Msg::ValkeyUrlPassword => pick(
                "carries a password: name it in password_env or password_file".into(),
                "traz uma senha: nomeie-a em password_env ou password_file".into(),
            ),
            #[cfg(feature = "valkey")]
            Msg::ValkeyUrlPlain => pick(
                "must be a rediss:// URL: TLS is required unless the host is loopback".into(),
                "deve ser uma URL rediss://: TLS é obrigatório, salvo em loopback".into(),
            ),
            #[cfg(feature = "valkey")]
            Msg::ValkeyUrlInsecure => pick(
                "must verify the server certificate: no #insecure".into(),
                "deve verificar o certificado do servidor: sem #insecure".into(),
            ),
            #[cfg(feature = "valkey")]
            Msg::ValkeyClusterPlain => pick(
                "needs a rediss:// url: the nodes a cluster announces are reached over TLS".into(),
                "exige uma url rediss://: os nós que um cluster anuncia são alcançados por TLS".into(),
            ),
            #[cfg(feature = "valkey")]
            Msg::ValkeyBoth => pick(
                "is set together with password_env: use one of the two".into(),
                "está junto de password_env: use um dos dois".into(),
            ),
            #[cfg(feature = "valkey")]
            Msg::ValkeyPrefixBraces => pick(
                "must not contain `{` or `}`".into(),
                "não pode conter `{` nem `}`".into(),
            ),
            #[cfg(feature = "valkey")]
            Msg::ValkeyEnvUnset(var) => pick(
                format!("the variable {var} is not set"),
                format!("a variável {var} não está definida"),
            ),
            #[cfg(feature = "valkey")]
            Msg::ValkeyTimeout(ms) => pick(
                format!("no answer within {ms} ms"),
                format!("sem resposta em {ms} ms"),
            ),
            #[cfg(feature = "valkey")]
            Msg::ValkeyReachable => pick("valkey: reachable".into(), "valkey: alcançável".into()),
            #[cfg(feature = "valkey")]
            Msg::ValkeyUnreachable(reason) => pick(
                format!("valkey: unreachable ({reason})"),
                format!("valkey: inalcançável ({reason})"),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotEndpointInvalid => pick(
                "is not an http:// or https:// URL with a host and nothing after it".into(),
                "não é uma URL http:// ou https:// com um host e nada depois dele".into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotEndpointUser => pick(
                "carries a user or a password: credentials come from the credential keys".into(),
                "traz um usuário ou uma senha: as credenciais vêm das chaves de credencial".into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotEndpointPlain => pick(
                "must be an https:// URL: TLS is required unless the host is loopback".into(),
                "deve ser uma URL https://: TLS é obrigatório, salvo em loopback".into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotPathStyle => pick(
                "must be true for an http://, IP or localhost endpoint: the bucket cannot go in \
                 front of that host"
                    .into(),
                "deve ser true para um endpoint http://, IP ou localhost: o bucket não pode ir à \
                 frente desse host"
                    .into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotRegion => pick(
                "must be lowercase letters, digits and `-`".into(),
                "deve ter só letras minúsculas, dígitos e `-`".into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotBucket => pick(
                "must be 3 to 63 lowercase letters, digits, `.` and `-`, starting and ending with \
                 a letter or a digit"
                    .into(),
                "deve ter de 3 a 63 letras minúsculas, dígitos, `.` e `-`, começando e \
                 terminando com letra ou dígito"
                    .into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotPrefix => pick(
                "must be `/`-separated segments, none of them empty, `.` or `..`".into(),
                "deve ter segmentos separados por `/`, nenhum vazio, `.` ou `..`".into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotBoth(other) => pick(
                format!("is set together with {other}: use one of the two"),
                format!("está junto de {other}: use um dos dois"),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotStaticOnly => pick(
                "is read only with credentials = \"static\"".into(),
                "só é lida com credentials = \"static\"".into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotEnvUnset(var) => pick(
                format!("the variable {var} is not set"),
                format!("a variável {var} não está definida"),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotNoSection(manifest) => pick(
                format!("{manifest} has no [snapshot] section"),
                format!("{manifest} não tem a seção [snapshot]"),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotBehind { repo, manifest } => pick(
                format!(
                    "{repo}: its index differs from the mirror; run meada index --workspace \
                     {manifest}"
                ),
                format!(
                    "{repo}: seu índice difere do espelho; rode meada index --workspace {manifest}"
                ),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotDirty { repo, mirror } => pick(
                format!(
                    "{repo}: the mirror {mirror} is not clean: git status lists changed, \
                     untracked or ignored files"
                ),
                format!(
                    "{repo}: o espelho {mirror} não está limpo: git status lista arquivos \
                     alterados, não rastreados ou ignorados"
                ),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotPodUrl => pick(
                "AWS_CONTAINER_CREDENTIALS_FULL_URI must name the Pod Identity agent, \
                 http://169.254.170.23 or http://[fd00:ec2::23]"
                    .into(),
                "AWS_CONTAINER_CREDENTIALS_FULL_URI deve apontar para o agente do Pod Identity, \
                 http://169.254.170.23 ou http://[fd00:ec2::23]"
                    .into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotRefused(origin) => pick(
                format!("refused a request to {origin}: not a configured endpoint"),
                format!("recusada uma requisição a {origin}: não é um endpoint configurado"),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotObjectBad(key) => pick(
                format!("{key}: not stored with the size and hash it records"),
                format!("{key}: não está guardado com o tamanho e o hash que registra"),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotChanged(repo) => pick(
                format!("{repo}: the mirror or its index changed while it was published"),
                format!("{repo}: o espelho ou seu índice mudou durante a publicação"),
            ),
            #[cfg(feature = "snapshot")]
            Msg::Snapshot => pick(
                "Server mode: the snapshot store of generations (ADR 0012).".into(),
                "Modo servidor: o armazenamento de gerações (ADR 0012).".into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotPublish => pick(
                "Upload the workspace as a generation, once every index equals its mirror."
                    .into(),
                "Envia o workspace como uma geração, quando cada índice equivale ao seu espelho."
                    .into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotPull => pick(
                "Fetch the store's current generation into local_dir, verify it and make it the \
                 node's current one."
                    .into(),
                "Baixa a geração atual do store em local_dir, verifica-a e a torna a atual do nó."
                    .into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotEvery => pick(
                "Pull again every this many seconds.".into(),
                "Repete o pull a cada tantos segundos.".into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotNoLocalDir(manifest) => pick(
                format!("{manifest}: `snapshot.local_dir` is not set: the pull writes there"),
                format!("{manifest}: `snapshot.local_dir` não está definido: o pull grava lá"),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotPulling(dir) => pick(
                format!("a pull is running in {dir}"),
                format!("um pull está em andamento em {dir}"),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotNoCurrent(key) => pick(
                format!("{key}: not found: no generation is published there"),
                format!("{key}: não encontrado: nenhuma geração foi publicada lá"),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotBadId(key) => pick(
                format!("{key}: does not hold an id of 16 lowercase hex characters"),
                format!("{key}: não contém um id de 16 caracteres hexadecimais minúsculos"),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotBadBundleKey(key) => pick(
                format!(
                    "the bundle key `{key}` is not repos/<identity>/<id>.tar under the prefix, \
                     with an id of 16 lowercase hex characters"
                ),
                format!(
                    "a chave de bundle `{key}` não é repos/<identidade>/<id>.tar sob o prefixo, \
                     com um id de 16 caracteres hexadecimais minúsculos"
                ),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotPointerBad(path) => pick(
                format!(
                    "{path}: not `<generation id> <fence>`; delete it to take the store's \
                     current generation"
                ),
                format!(
                    "{path}: não é `<id da geração> <fence>`; apague-o para receber a geração \
                     atual do store"
                ),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotNotDocument(key) => pick(
                format!("{key}: not the generation document of its id"),
                format!("{key}: não é o documento de geração do seu id"),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotIdentityChars => pick(
                "not a repository identity: it holds whitespace, a quote, a backslash or a \
                 control character"
                    .into(),
                "não é uma identidade de repositório: contém espaço, aspas, barra invertida ou \
                 caractere de controle"
                    .into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotBytesCeiling(max) => pick(
                format!("past the ceiling of {max} bytes per generation"),
                format!("acima do teto de {max} bytes por geração"),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotEntriesCeiling(max) => pick(
                format!("past the ceiling of {max} archive entries per generation"),
                format!("acima do teto de {max} entradas de arquivo por geração"),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotEntryHeader(max) => pick(
                format!("a long name or a pax header past the ceiling of {max} bytes"),
                format!("um nome longo ou um cabeçalho pax acima do teto de {max} bytes"),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotEntryNested => pick(
                "a nested repository: a `.git` below the mirror's own, or under its `.git/modules`"
                    .into(),
                "um repositório aninhado: um `.git` abaixo do próprio do espelho, ou sob o seu \
                 `.git/modules`"
                    .into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotEntryPath => pick(
                "an absolute path, a `.` or `..` component, or a path other than index.db first \
                 and the mirror under mirror/"
                    .into(),
                "um caminho absoluto, um componente `.` ou `..`, ou um caminho que não é \
                 index.db primeiro e o espelho sob mirror/"
                    .into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotEntryLink => pick(
                "a link whose target leaves the repository directory".into(),
                "um link cujo alvo sai do diretório do repositório".into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotEntryThrough => pick(
                "written through a symbolic link or over an earlier entry".into(),
                "gravada através de um link simbólico ou sobre uma entrada anterior".into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotEntrySpecial => pick(
                "a device, a FIFO or another special file".into(),
                "um dispositivo, um FIFO ou outro arquivo especial".into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotNoGit => pick(
                "the mirror has no .git directory".into(),
                "o espelho não tem um diretório .git".into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotHead { found, head } => pick(
                format!("the mirror's HEAD is {found}, not the document's {head}"),
                format!("o HEAD do espelho é {found}, não o {head} do documento"),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotNoneHeld(dir) => pick(
                format!("{dir} holds no generation: run meada snapshot pull"),
                format!("{dir} não guarda nenhuma geração: rode meada snapshot pull"),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotTimeout(ms) => pick(
                format!("no answer within {ms} ms"),
                format!("sem resposta em {ms} ms"),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotStatusCurrent(id) => pick(
                format!("current: {}", id.unwrap_or("none")),
                format!("atual: {}", id.unwrap_or("nenhuma")),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotStatusHeld(ids) => match ids.is_empty() {
                true => pick("held: none".into(), "retidas: nenhuma".into()),
                false => pick(
                    format!("held: {}", ids.join(" ")),
                    format!("retidas: {}", ids.join(" ")),
                ),
            },
            #[cfg(feature = "snapshot")]
            Msg::SnapshotStoreReachable(id) => pick(
                format!("store: reachable (current {id})"),
                format!("store: alcançável (atual {id})"),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotStoreUnreachable(reason) => pick(
                format!("store: unreachable ({reason})"),
                format!("store: inalcançável ({reason})"),
            ),
            #[cfg(feature = "snapshot")]
            Msg::GenerationsHelp => pick(
                "Use the generation `meada snapshot pull` made current on this node, with \
                 the --workspace manifest's [snapshot] section"
                    .into(),
                "Usa a geração que `meada snapshot pull` tornou atual neste nó, com a seção \
                 [snapshot] do manifesto de --workspace"
                    .into(),
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
