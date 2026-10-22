//! The CLI's message catalog: pt-BR and English, one match arm per message. The language comes
//! from `LANG` (ADR 0008): pt-BR when it starts with `pt`, English otherwise. Only the CLI calls
//! `init`, so the MCP server and the library tests stay in English. The messages of the checks
//! `meada-core` holds are its catalog's, and the language is its flag: `init` sets it for both.

use crate::decisions::Cause;
use crate::document::Format;

/// Picks the language from the value of `LANG`; `None` is English.
pub fn init(lang: Option<&str>) {
    meada_core::msg::init(lang);
}

/// Where an index that cannot be read is fixed (`index::open_read`).
#[derive(Clone, Copy)]
pub enum Fix<'a> {
    /// `meada index --workspace <manifest>`.
    Index(&'a str),
    /// A node's generation `id` (`--generations`, feature `snapshot`), held in `dir`
    /// (`<local_dir>/<id>`): a pull of the generation already current changes nothing, so the
    /// fix removes `dir` before the pull, or publishes another generation.
    Generation { id: &'a str, dir: &'a str },
}

pub enum Msg<'a> {
    NotIndexed,
    OtherVersion,
    Indexed(i64),
    OtherVersionWorkspace,
    /// `meada_core::msg::Msg::ListedTwice`.
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
    /// A tool call on a served workspace that reached the server without passing admission.
    NotAdmitted,
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
    IndexNotRollback(&'a str),
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
    /// `meada_core::msg::Msg::KeyMissing`.
    KeyMissing(&'a str),
    /// `meada_core::msg::Msg::Empty`.
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
    #[cfg(any(feature = "generation-pointer", feature = "github-app"))]
    NeedsSections {
        section: &'a str,
        needed: &'a [&'a str],
    },
    #[cfg(any(
        feature = "generation-pointer",
        feature = "oidc",
        feature = "github-app"
    ))]
    AtLeast(i64),
    #[cfg(feature = "generation-pointer")]
    GenerationsBadPointer(&'a str),
    #[cfg(feature = "generation-pointer")]
    GenerationsBadEntry(&'a str),
    #[cfg(feature = "generation-pointer")]
    GenerationsEmpty(&'a str),
    /// A retained list (`key`) longer than `keep` plus the kept maximum (`max`).
    #[cfg(feature = "generation-pointer")]
    GenerationsTooLong {
        key: &'a str,
        max: u64,
    },
    /// The pointer script refused a fence not above `current`'s (`{<key_prefix>}:fence` lost).
    #[cfg(feature = "generation-pointer")]
    FenceRefused {
        fence: u64,
        current: &'a str,
        fence_key: &'a str,
    },
    /// A pull that would lower the node's fence.
    #[cfg(feature = "generation-pointer")]
    FenceBelowNode {
        current: &'a str,
        id: &'a str,
        fence: u64,
        held: u64,
        pointer: &'a str,
        fence_key: &'a str,
    },
    /// `status --generations` on a node whose fence is above Valkey's.
    #[cfg(feature = "generation-pointer")]
    FenceStuck {
        held: u64,
        pointer: &'a str,
        fence: u64,
        current: &'a str,
        fence_key: &'a str,
    },
    /// The lock no longer holds a publish's token.
    #[cfg(feature = "generation-pointer")]
    LockNotHeld(&'a str),
    /// The lock already held, and its holder's host, pid, start time and what holds it (`None`
    /// for a lock with no holder).
    #[cfg(feature = "refresh-lock")]
    LockHeld {
        key: &'a str,
        holder: Option<[&'a str; 4]>,
    },
    /// `pointer set` of a generation outside the retained list `key`.
    #[cfg(feature = "refresh-lock")]
    PointerNotRetained {
        id: &'a str,
        key: &'a str,
    },
    /// `pointer retain` past the kept maximum `max` of the kept set `key`.
    #[cfg(feature = "refresh-lock")]
    KeptFull {
        key: &'a str,
        max: u64,
    },
    /// `pointer drop` of the current generation, which the pointer `key` names.
    #[cfg(feature = "refresh-lock")]
    DropCurrent {
        id: &'a str,
        key: &'a str,
    },
    /// `pointer drop` of a generation outside the retained list `key`.
    #[cfg(feature = "refresh-lock")]
    DropNotRetained {
        id: &'a str,
        key: &'a str,
    },
    /// A generation id on the command line that is not one.
    #[cfg(feature = "refresh-lock")]
    BadGenerationArg(&'a str),
    /// `--at-least` that is not a decimal fence.
    #[cfg(feature = "refresh-lock")]
    BadFenceArg(&'a str),
    /// What `fence recover` read and could not count: the problem, named.
    #[cfg(feature = "refresh-lock")]
    FenceNotCounted(&'a str),
    /// A heartbeat key whose value is no heartbeat.
    #[cfg(feature = "refresh-lock")]
    HeartbeatBad(&'a str),
    /// A member of the node set (`key`) that is not a node name, shown escaped.
    #[cfg(feature = "refresh-lock")]
    NodeNameBad {
        key: &'a str,
        name: &'a str,
    },
    #[cfg(feature = "refresh-lock")]
    SnapshotPointer,
    #[cfg(feature = "refresh-lock")]
    SnapshotPointerSet,
    #[cfg(feature = "refresh-lock")]
    SnapshotPointerGeneration,
    #[cfg(feature = "refresh-lock")]
    SnapshotPointerRetain,
    #[cfg(feature = "refresh-lock")]
    SnapshotPointerRetainGeneration,
    #[cfg(feature = "refresh-lock")]
    SnapshotPointerDrop,
    #[cfg(feature = "refresh-lock")]
    SnapshotFence,
    #[cfg(feature = "refresh-lock")]
    SnapshotFenceRecover,
    #[cfg(feature = "refresh-lock")]
    SnapshotFenceAtLeast,
    #[cfg(feature = "refresh-lock")]
    LockLost(&'a str),
    #[cfg(feature = "refresh-lock")]
    LockBadFence(&'a str),
    #[cfg(feature = "refresh-lock")]
    LockNoSection(&'a str),
    #[cfg(feature = "refresh-lock")]
    LockEnv,
    #[cfg(feature = "refresh-lock")]
    SnapshotLock,
    #[cfg(feature = "refresh-lock")]
    SnapshotLockCommand,
    #[cfg(feature = "snapshot")]
    GenerationNotHeld(&'a str),
    /// An `[oidc]` URL, or `[ui] public_url`, that is not `https` (or `http` on loopback) with
    /// a host and no user, query or fragment; with `bare`, no path either.
    #[cfg(any(feature = "oidc", feature = "ui"))]
    OidcUrl {
        bare: bool,
    },
    /// A `jwks_uri` that is not `https` (or `http` on loopback) with a host and no user or
    /// fragment.
    #[cfg(feature = "oidc")]
    OidcJwksUrl,
    #[cfg(feature = "oidc")]
    OidcEmpty,
    /// An `[oidc]` section with neither `identities_claim` nor `groups_claim`.
    #[cfg(feature = "oidc")]
    OidcNoClaim,
    /// A configured scope holding a character no RFC 6749 scope-token carries.
    #[cfg(feature = "oidc")]
    OidcScope {
        scope: &'a str,
        bad: char,
    },
    /// None of the issuer's metadata documents could be used: each URL tried with why.
    #[cfg(feature = "oidc")]
    OidcMetadata(&'a str),
    #[cfg(feature = "oidc")]
    OidcIssuerMismatch(&'a str),
    #[cfg(feature = "oidc")]
    OidcJwksUri(&'a str),
    #[cfg(feature = "oidc")]
    OidcJwks {
        url: &'a str,
        why: &'a str,
    },
    #[cfg(feature = "oidc")]
    OidcJwksNoKey,
    #[cfg(feature = "oidc")]
    OidcRefreshFailed(&'a str),
    #[cfg(feature = "snapshot")]
    SnapshotRetainedSkipped {
        id: &'a str,
        why: &'a str,
    },
    #[cfg(feature = "snapshot")]
    GenerationsHelp,
    #[cfg(feature = "github-app")]
    GithubApp,
    #[cfg(feature = "github-app")]
    GithubAppCredential,
    #[cfg(feature = "github-app")]
    GithubAppAction,
    #[cfg(feature = "github-app")]
    GithubAppThrottled,
    #[cfg(feature = "github-app")]
    GithubAppRepository,
    #[cfg(feature = "github-app")]
    GithubAppRetryAfter,
    /// A `host` that is not a git host name with an optional port.
    #[cfg(feature = "github-app")]
    GithubAppHost,
    /// An `api` that is not `https` (or `http` on loopback) with a host and no user, query or
    /// fragment.
    #[cfg(feature = "github-app")]
    GithubAppApi,
    #[cfg(feature = "github-app")]
    GithubAppKeyBoth,
    #[cfg(feature = "github-app")]
    GithubAppKeyNone,
    #[cfg(feature = "github-app")]
    GithubAppTwice,
    /// The App's key, named by where it comes from, does not read as an RSA key in PEM.
    #[cfg(feature = "github-app")]
    GithubAppKey(&'a str),
    #[cfg(feature = "github-app")]
    GithubAppInput,
    #[cfg(feature = "github-app")]
    GithubAppPath,
    #[cfg(feature = "github-app")]
    GithubAppNoPath(&'a str),
    #[cfg(feature = "github-app")]
    GithubAppUncovered {
        host: &'a str,
        owner: &'a str,
    },
    #[cfg(feature = "github-app")]
    GithubAppAllThrottled(&'a str),
    /// A `get` whose every candidate's mint was throttled.
    #[cfg(feature = "github-app")]
    GithubAppNoToken(&'a str),
    #[cfg(feature = "github-app")]
    GithubAppStatus {
        url: &'a str,
        status: u16,
    },
    #[cfg(feature = "github-app")]
    GithubAppAnswer(&'a str),
    /// What a REST call answered 403 or 429 marked: an App or an installation, for `secs`.
    #[cfg(feature = "github-app")]
    GithubAppMarked {
        what: &'a str,
        secs: u64,
    },
    #[cfg(feature = "github-app")]
    GithubAppWaited {
        what: &'a str,
        secs: u64,
    },
    #[cfg(feature = "github-app")]
    GithubAppNotAnswered(&'a str),
    #[cfg(feature = "github-app")]
    GithubAppRepositoryUrl,
    /// `meada ui` (feature `ui`, ADR 0014).
    #[cfg(feature = "ui")]
    Ui,
    #[cfg(feature = "ui")]
    UiListenHelp,
    #[cfg(feature = "ui")]
    UiWorkspaceHelp,
    /// A bootstrap manifest given to `meada ui` without `[ui]`.
    #[cfg(feature = "ui")]
    UiNoSection,
    /// `[oidc]` in the console's bootstrap manifest, before the console admits tokens (#306).
    #[cfg(feature = "ui")]
    UiOidc,
    /// `[ui] dir` that is not a directory.
    #[cfg(feature = "ui")]
    UiDirMissing(&'a str),
    /// `[ui] dir` whose `meada-ui.json` names `found` (`None`: missing, oversized or
    /// unreadable) instead of this build's version.
    #[cfg(feature = "ui")]
    UiVersion {
        dir: &'a str,
        found: Option<&'a str>,
        expected: &'a str,
    },
    /// An `/api/v1/` path with no route.
    #[cfg(feature = "ui")]
    UiNoRoute,
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
        use meada_core::msg::Msg as Core;
        let pt = meada_core::msg::pt();
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
            Msg::ListedTwice => Core::ListedTwice.text(),
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
            Msg::NotAdmitted => pick(
                "the request was not admitted".into(),
                "a requisição não foi admitida".into(),
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
                Fix::Generation { id, dir } => pick(
                    format!(
                        "{repo}: no index in generation {id}; remove {dir} and run meada snapshot \
                         pull again, or publish a new generation"
                    ),
                    format!(
                        "{repo}: sem índice na geração {id}; remova {dir} e rode meada snapshot \
                         pull de novo, ou publique uma nova geração"
                    ),
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
                Fix::Generation { id, .. } => pick(
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
                Fix::Generation { id, dir } => pick(
                    format!(
                        "{repo}: its index in generation {id} holds an interrupted commit; \
                         remove {dir} and run meada snapshot pull again, or publish a new \
                         generation"
                    ),
                    format!(
                        "{repo}: seu índice na geração {id} guarda um commit interrompido; \
                         remova {dir} e rode meada snapshot pull de novo, ou publique uma nova \
                         geração"
                    ),
                ),
            },
            #[cfg(feature = "snapshot")]
            Msg::IndexNotRollback(repo) => pick(
                format!("{repo}: index.db: not a database in rollback journal mode"),
                format!("{repo}: index.db: não é um banco em modo rollback journal"),
            ),
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
            Msg::KeyMissing(key) => Core::KeyMissing(key).text(),
            Msg::Empty => Core::Empty.text(),
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
            #[cfg(any(feature = "generation-pointer", feature = "github-app"))]
            Msg::NeedsSections { section, needed } => {
                let needed: Vec<String> = needed.iter().map(|s| format!("[{s}]")).collect();
                pick(
                    format!("[{section}] needs {}", needed.join(" and ")),
                    format!("[{section}] exige {}", needed.join(" e ")),
                )
            }
            #[cfg(any(feature = "generation-pointer", feature = "oidc", feature = "github-app"))]
            Msg::AtLeast(min) => pick(
                format!("must be at least {min}"),
                format!("deve ser pelo menos {min}"),
            ),
            #[cfg(feature = "generation-pointer")]
            Msg::GenerationsBadPointer(key) => pick(
                format!(
                    "{key}: not a hash of `id`, a generation id of 16 lowercase hex characters, \
                     and `fence`, a decimal fence"
                ),
                format!(
                    "{key}: não é um hash de `id`, um id de geração de 16 caracteres hexadecimais \
                     minúsculos, e `fence`, um fence decimal"
                ),
            ),
            #[cfg(feature = "generation-pointer")]
            Msg::GenerationsBadEntry(key) => pick(
                format!(
                    "{key}: an entry is not `<generation id>:<fence>`, an id of 16 lowercase hex \
                     characters and a decimal fence"
                ),
                format!(
                    "{key}: uma entrada não é `<id da geração>:<fence>`, um id de 16 caracteres \
                     hexadecimais minúsculos e um fence decimal"
                ),
            ),
            #[cfg(feature = "generation-pointer")]
            Msg::GenerationsEmpty(key) => pick(
                format!("{key}: empty: no generation is retained there"),
                format!("{key}: vazia: nenhuma geração está retida lá"),
            ),
            #[cfg(feature = "generation-pointer")]
            Msg::GenerationsTooLong { key, max } => pick(
                format!(
                    "{key}: holds more than {max} entries, `keep` plus the {kept} kept \
                     generations allowed",
                    kept = crate::snapshot::generations::KEPT_MAX
                ),
                format!(
                    "{key}: tem mais de {max} entradas, `keep` mais as {kept} gerações mantidas \
                     permitidas",
                    kept = crate::snapshot::generations::KEPT_MAX
                ),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotRetainedSkipped { id, why } => pick(
                format!("retained generation {id} not pulled, the next pull retries: {why}"),
                format!("geração retida {id} não baixada, o próximo pull tenta de novo: {why}"),
            ),
            #[cfg(feature = "generation-pointer")]
            Msg::FenceRefused {
                fence,
                current,
                fence_key,
            } => pick(
                format!(
                    "{current}: refused fence {fence}, not above the fence it holds: \
                     {fence_key} was lowered or lost; raise it with meada snapshot fence recover \
                     (--at-least for a fence read in a node's <local_dir>/current), or delete a \
                     node's <local_dir>/current so it takes the current generation whatever its \
                     fence"
                ),
                format!(
                    "{current}: recusado o fence {fence}, não maior que o que ele guarda: \
                     {fence_key} foi rebaixado ou perdido; eleve-o com meada snapshot fence \
                     recover (--at-least para um fence lido no <local_dir>/current de um nó), ou \
                     apague o <local_dir>/current de um nó para que ele tome a geração atual \
                     seja qual for o fence"
                ),
            ),
            #[cfg(feature = "generation-pointer")]
            Msg::FenceBelowNode {
                current,
                id,
                fence,
                held,
                pointer,
                fence_key,
            } => pick(
                format!(
                    "{current}: generation {id} has fence {fence}, below fence {held} in \
                     {pointer}: not swapped; raise {fence_key} with meada snapshot fence recover \
                     --at-least {held} and publish again, or delete {pointer} (with \
                     [refresh_lock] turned off every fence is 0, so only deleting it helps)"
                ),
                format!(
                    "{current}: a geração {id} tem fence {fence}, abaixo do fence {held} em \
                     {pointer}: não trocada; eleve {fence_key} com meada snapshot fence recover \
                     --at-least {held} e publique de novo, ou apague {pointer} (com \
                     [refresh_lock] desligado todo fence é 0, e só apagá-lo resolve)"
                ),
            ),
            #[cfg(feature = "generation-pointer")]
            Msg::FenceStuck {
                held,
                pointer,
                fence,
                current,
                fence_key,
            } => pick(
                format!(
                    "fence: stuck: {held} in {pointer} is above {fence} in {current}; raise \
                     {fence_key} with meada snapshot fence recover --at-least {held} and publish \
                     again, or delete {pointer}"
                ),
                format!(
                    "fence: travado: {held} em {pointer} está acima de {fence} em {current}; \
                     eleve {fence_key} com meada snapshot fence recover --at-least {held} e \
                     publique de novo, ou apague {pointer}"
                ),
            ),
            #[cfg(feature = "generation-pointer")]
            Msg::LockNotHeld(key) => pick(
                format!(
                    "{key}: the refresh lock no longer holds this job's token: nothing more \
                     uploaded and no pointer written"
                ),
                format!(
                    "{key}: o lock de refresh não guarda mais o token deste job: nada mais \
                     enviado e nenhum ponteiro gravado"
                ),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::LockHeld {
                key,
                holder: Some([host, pid, started, holds]),
            } => pick(
                format!(
                    "{key}: held by host {host}, process {pid}, since {started}, for {holds}: \
                     nothing run"
                ),
                format!(
                    "{key}: em posse do host {host}, processo {pid}, desde {started}, para \
                     {holds}: nada executado"
                ),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::LockHeld { key, holder: None } => pick(
                format!("{key}: held by an unknown holder: nothing run"),
                format!("{key}: em posse de um detentor desconhecido: nada executado"),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::PointerNotRetained { id, key } => pick(
                format!("{key}: generation {id} is not retained: the pointer did not move"),
                format!("{key}: a geração {id} não está retida: o ponteiro não se moveu"),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::KeptFull { key, max } => pick(
                format!(
                    "{key}: already keeps {max} generations, the most it may: drop one first \
                     (meada snapshot pointer drop)"
                ),
                format!(
                    "{key}: já mantém {max} gerações, o máximo permitido: remova uma antes \
                     (meada snapshot pointer drop)"
                ),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::DropCurrent { id, key } => pick(
                format!("{key}: generation {id} is the current one: nothing dropped"),
                format!("{key}: a geração {id} é a atual: nada removido"),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::DropNotRetained { id, key } => pick(
                format!("{key}: generation {id} is not retained: nothing dropped"),
                format!("{key}: a geração {id} não está retida: nada removido"),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::BadGenerationArg(text) => pick(
                format!("{text}: not a generation id of 16 lowercase hex characters"),
                format!("{text}: não é um id de geração de 16 caracteres hexadecimais minúsculos"),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::BadFenceArg(text) => pick(
                format!("--at-least {text}: not a decimal fence"),
                format!("--at-least {text}: não é um fence decimal"),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::FenceNotCounted(problem) => pick(
                format!("{problem}: not counted"),
                format!("{problem}: não contado"),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::HeartbeatBad(key) => pick(
                format!("{key}: not a heartbeat, a JSON object whose fence is a decimal or null"),
                format!(
                    "{key}: não é um heartbeat, um objeto JSON cujo fence é um decimal ou null"
                ),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::NodeNameBad { key, name } => pick(
                format!(
                    "{key}: \"{name}\" is not a node name (lowercase letters, digits, . and -, 1 \
                     to 63 characters)"
                ),
                format!(
                    "{key}: \"{name}\" não é um nome de nó (letras minúsculas, dígitos, . e -, \
                     de 1 a 63 caracteres)"
                ),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::SnapshotPointer => pick(
                "Move the workspace's generation pointer under the refresh lock.".into(),
                "Move o ponteiro de geração do workspace sob o lock de refresh.".into(),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::SnapshotPointerSet => pick(
                "Point the fleet at a retained generation: a rollback, or forward again.".into(),
                "Aponta a frota para uma geração retida: um rollback, ou adiante de novo.".into(),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::SnapshotPointerGeneration => pick(
                "The generation id, one of the retained set.".into(),
                "O id da geração, um dos retidos.".into(),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::SnapshotPointerRetain => pick(
                "Keep a generation on every node, outside the keep trim, until it is dropped."
                    .into(),
                "Mantém uma geração em todo nó, fora do corte de keep, até ser removida.".into(),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::SnapshotPointerRetainGeneration => pick(
                "The generation id, one whose document the store holds.".into(),
                "O id da geração, uma cujo documento o store guarda.".into(),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::SnapshotPointerDrop => pick(
                "Stop retaining a generation that is not current; the next pull deletes it."
                    .into(),
                "Deixa de reter uma geração que não é a atual; o próximo pull a apaga.".into(),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::SnapshotFence => pick(
                "Recover the refresh lock's fence under the lock.".into(),
                "Recupera o fence do lock de refresh sob o lock.".into(),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::SnapshotFenceRecover => pick(
                "Raise the fence to the highest one recorded in Valkey, never lowering it."
                    .into(),
                "Eleva o fence ao maior registrado no Valkey, sem nunca baixá-lo.".into(),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::SnapshotFenceAtLeast => pick(
                "A floor for the fence, such as one read in a node's <local_dir>/current."
                    .into(),
                "Um piso para o fence, como um lido no <local_dir>/current de um nó.".into(),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::LockLost(key) => pick(
                format!("{key}: the refresh lock was lost: the command was terminated"),
                format!("{key}: o lock de refresh foi perdido: o comando foi encerrado"),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::LockBadFence(key) => pick(
                format!(
                    "{key}: not a positive decimal fence: set it at or above the highest fence \
                     recorded"
                ),
                format!(
                    "{key}: não é um fence decimal positivo: defina-o igual ou acima do maior \
                     fence registrado"
                ),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::LockNoSection(manifest) => pick(
                format!("{manifest}: no [refresh_lock] section: the lock has no TTL"),
                format!("{manifest}: sem seção [refresh_lock]: o lock não tem TTL"),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::LockEnv => pick(
                "[refresh_lock] needs MEADA_REFRESH_LOCK and MEADA_REFRESH_FENCE: run publish \
                 under meada snapshot lock"
                    .into(),
                "[refresh_lock] exige MEADA_REFRESH_LOCK e MEADA_REFRESH_FENCE: rode o publish \
                 sob meada snapshot lock"
                    .into(),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::SnapshotLock => pick(
                "Take the workspace's refresh lock, run the command as its child and release \
                 the lock."
                    .into(),
                "Toma o lock de refresh do workspace, roda o comando como filho e libera o lock."
                    .into(),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::SnapshotLockCommand => pick(
                "The refresh to run: fetch, meada index --workspace, meada snapshot publish."
                    .into(),
                "O refresh a rodar: fetch, meada index --workspace, meada snapshot publish."
                    .into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::GenerationNotHeld(id) => pick(
                format!("generation {id} is not held here"),
                format!("a geração {id} não está retida aqui"),
            ),
            #[cfg(any(feature = "oidc", feature = "ui"))]
            Msg::OidcUrl { bare: false } => pick(
                "must be an https:// URL with a host and no user, query or fragment (http:// \
                 only on loopback)"
                    .into(),
                "deve ser uma URL https:// com um host e sem usuário, query ou fragmento \
                 (http:// só em loopback)"
                    .into(),
            ),
            #[cfg(any(feature = "oidc", feature = "ui"))]
            Msg::OidcUrl { bare: true } => pick(
                "must be an https:// URL with a host and nothing after it (http:// only on \
                 loopback)"
                    .into(),
                "deve ser uma URL https:// com um host e nada depois dele (http:// só em \
                 loopback)"
                    .into(),
            ),
            #[cfg(feature = "oidc")]
            Msg::OidcJwksUrl => pick(
                "must be an https:// URL with a host and no user or fragment (http:// only on \
                 loopback)"
                    .into(),
                "deve ser uma URL https:// com um host e sem usuário ou fragmento (http:// só \
                 em loopback)"
                    .into(),
            ),
            #[cfg(feature = "oidc")]
            Msg::OidcEmpty => pick("must not be empty".into(), "não pode ser vazio".into()),
            #[cfg(feature = "oidc")]
            Msg::OidcNoClaim => pick(
                "set `identities_claim`, `groups_claim` or both".into(),
                "defina `identities_claim`, `groups_claim` ou os dois".into(),
            ),
            #[cfg(feature = "oidc")]
            Msg::OidcScope { scope, bad } => pick(
                format!("{scope:?} holds {bad:?}, which a scope cannot carry (RFC 6749)"),
                format!("{scope:?} contém {bad:?}, que um escopo não pode ter (RFC 6749)"),
            ),
            #[cfg(feature = "oidc")]
            Msg::OidcMetadata(tried) => pick(
                format!("oidc: the issuer's metadata could not be read: {tried}"),
                format!("oidc: os metadados do issuer não puderam ser lidos: {tried}"),
            ),
            #[cfg(feature = "oidc")]
            Msg::OidcIssuerMismatch(found) => pick(
                format!("names the issuer {found:?}, not the configured one"),
                format!("nomeia o issuer {found:?}, não o configurado"),
            ),
            #[cfg(feature = "oidc")]
            Msg::OidcJwksUri(why) => pick(
                format!("its jwks_uri {why}"),
                format!("o seu jwks_uri {why}"),
            ),
            #[cfg(feature = "oidc")]
            Msg::OidcJwks { url, why } => pick(
                format!("oidc: the JWKS at {url} could not be read: {why}"),
                format!("oidc: o JWKS em {url} não pôde ser lido: {why}"),
            ),
            #[cfg(feature = "oidc")]
            Msg::OidcJwksNoKey => pick(
                "it holds no signing key Meada accepts (RSA, or EC on P-256 or P-384)".into(),
                "ele não tem nenhuma chave de assinatura que o Meada aceite (RSA, ou EC em P-256 \
                 ou P-384)"
                    .into(),
            ),
            #[cfg(feature = "oidc")]
            Msg::OidcRefreshFailed(why) => pick(
                format!("meada: {why}; keeping the cached keys"),
                format!("meada: {why}; as chaves em cache continuam"),
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
            #[cfg(feature = "github-app")]
            Msg::GithubApp => pick(
                "git credentials from GitHub App installation tokens, for the indexing job."
                    .into(),
                "Credenciais do git a partir de tokens de instalação de GitHub App, para o job \
                 de indexação."
                    .into(),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppCredential => pick(
                "git's credential helper: answer get with an installation token, ignore store, \
                 drop the cached token on erase."
                    .into(),
                "Credential helper do git: responde get com um token de instalação, ignora \
                 store, descarta o token em cache no erase."
                    .into(),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppAction => pick(
                "The operation git asks for: get, store or erase.".into(),
                "A operação que o git pede: get, store ou erase.".into(),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppThrottled => pick(
                "Mark the installation that last answered for a repository as throttled, after \
                 a fetch that failed with 403 or 429."
                    .into(),
                "Marca como limitada a instalação que respondeu por último para um repositório, \
                 depois de um fetch que falhou com 403 ou 429."
                    .into(),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppRepository => pick(
                "The https URL of the repository whose fetch failed.".into(),
                "A URL https do repositório cujo fetch falhou.".into(),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppRetryAfter => pick(
                "The Retry-After of the failed fetch, in seconds; without it, 1 minute, doubled \
                 on each repeat."
                    .into(),
                "O Retry-After do fetch que falhou, em segundos; sem ele, 1 minuto, dobrado a \
                 cada repetição."
                    .into(),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppHost => pick(
                "must be a git host name, with an optional port".into(),
                "deve ser o nome de um host git, com uma porta opcional".into(),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppApi => pick(
                "must be an https:// URL with a host and no user, query or fragment (http:// \
                 only on loopback)"
                    .into(),
                "deve ser uma URL https:// com um host e sem usuário, query ou fragmento \
                 (http:// só em loopback)"
                    .into(),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppKeyBoth => pick(
                "is set together with private_key_file: use one of the two".into(),
                "está junto de private_key_file: use um dos dois".into(),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppKeyNone => pick(
                "needs private_key_file or private_key_env".into(),
                "exige private_key_file ou private_key_env".into(),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppTwice => pick(
                "names an App another [[github_app]] block already lists for this host".into(),
                "nomeia um App que outro bloco [[github_app]] já lista para este host".into(),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppKey(source) => pick(
                format!("{source}: not an RSA private key in PEM"),
                format!("{source}: não é uma chave privada RSA em PEM"),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppInput => pick(
                "git's request is not key=value lines within 64 KiB".into(),
                "o pedido do git não são linhas key=value em até 64 KiB".into(),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppPath => pick(
                "git's path is not <owner>/<repo>".into(),
                "o path do git não é <owner>/<repo>".into(),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppNoPath(host) => pick(
                format!("{host}: git sent no path; set credential.useHttpPath = true"),
                format!("{host}: o git não mandou path; defina credential.useHttpPath = true"),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppUncovered { host, owner } => pick(
                format!("no GitHub App installation covers {host}/{owner}"),
                format!("nenhuma instalação de GitHub App cobre {host}/{owner}"),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppAllThrottled(owner) => pick(
                format!("every installation covering {owner} is throttled; trying them anyway"),
                format!("toda instalação que cobre {owner} está limitada; tentando assim mesmo"),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppNoToken(owner) => pick(
                format!("no installation covering {owner} gave a token: each mint was throttled"),
                format!("nenhuma instalação que cobre {owner} deu um token: cada mint foi limitado"),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppStatus { url, status } => pick(
                format!("{url}: http status {status}"),
                format!("{url}: status http {status}"),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppAnswer(url) => pick(
                format!("{url}: not an answer GitHub gives"),
                format!("{url}: não é uma resposta que o GitHub dá"),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppMarked { what, secs } => pick(
                format!("{what}: throttled for {secs} s"),
                format!("{what}: limitado por {secs} s"),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppWaited { what, secs } => pick(
                format!("{what}: another run's mint did not finish within {secs} s"),
                format!("{what}: o mint de outra execução não terminou em {secs} s"),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppNotAnswered(repo) => pick(
                format!("no installation answered for {repo} within the last hour; nothing marked"),
                format!("nenhuma instalação respondeu por {repo} na última hora; nada marcado"),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppRepositoryUrl => pick(
                "the repository URL is not https://<host>/<owner>/<repo>".into(),
                "a URL do repositório não é https://<host>/<owner>/<repo>".into(),
            ),
            #[cfg(feature = "ui")]
            Msg::Ui => pick(
                "Serve the management console, read only, at host:port, :port or port (loopback \
                 without a host)."
                    .into(),
                "Serve o console de gestão, só leitura, em host:porta, :porta ou porta (loopback \
                 sem host)."
                    .into(),
            ),
            #[cfg(feature = "ui")]
            Msg::UiListenHelp => pick(
                "Where the console listens: host:port, :port or port (loopback without a host)"
                    .into(),
                "Onde o console escuta: host:porta, :porta ou porta (loopback sem host)".into(),
            ),
            #[cfg(feature = "ui")]
            Msg::UiWorkspaceHelp => pick(
                "The bootstrap manifest, with its [ui] section".into(),
                "O manifesto de bootstrap, com a seção [ui]".into(),
            ),
            #[cfg(feature = "ui")]
            Msg::UiNoSection => pick(
                "the bootstrap manifest has no [ui] section".into(),
                "o manifesto de bootstrap não tem seção [ui]".into(),
            ),
            #[cfg(feature = "ui")]
            Msg::UiOidc => pick(
                "[oidc] is not yet supported by the console: remove it and deploy the console \
                 behind a proxy that admits its users"
                    .into(),
                "[oidc] ainda não é suportado pelo console: remova-o e implante o console atrás \
                 de um proxy que admita seus usuários"
                    .into(),
            ),
            #[cfg(feature = "ui")]
            Msg::UiDirMissing(dir) => pick(
                format!("`{dir}` is not a directory"),
                format!("`{dir}` não é um diretório"),
            ),
            #[cfg(feature = "ui")]
            Msg::UiVersion {
                dir,
                found: Some(found),
                expected,
            } => pick(
                format!("`{dir}` holds files built for Meada {found}, not {expected}"),
                format!("`{dir}` tem arquivos feitos para o Meada {found}, não {expected}"),
            ),
            #[cfg(feature = "ui")]
            Msg::UiVersion {
                dir,
                found: None,
                expected,
            } => pick(
                format!(
                    "`{dir}` has no readable meada-ui.json naming its version; this is \
                     Meada {expected}"
                ),
                format!(
                    "`{dir}` não tem um meada-ui.json legível com a versão; este é o \
                     Meada {expected}"
                ),
            ),
            #[cfg(feature = "ui")]
            Msg::UiNoRoute => pick(
                "no API route at this path".into(),
                "nenhuma rota da API neste caminho".into(),
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
