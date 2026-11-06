//! The CLI's message catalog: pt-BR, English and Spanish, one match arm per message. The language
//! comes from `LANG` (ADR 0008): pt-BR when it starts with `pt`, Spanish with `es`, English
//! otherwise. Only the CLI calls `init`, so the MCP server and the library tests stay in English.
//! The messages of the checks `meada-core` holds are its catalog's, and the language is its
//! setting: `init` sets it for both.

use crate::decisions::Cause;
use crate::document::Format;
use meada_core::msg::Lang;

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

const REPORT_URL: &str =
    "https://github.com/ClaudioMascaro/meada/issues/new?template=inference.yml";

pub enum Msg<'a> {
    NotIndexed,
    OtherVersion,
    Indexed(i64),
    OtherVersionWorkspace,
    /// The last line of a failed `meada index --workspace`: the stage and how many of its
    /// repositories, each named on its own line before it, failed (#316).
    IndexFailed(usize),
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
    ViaUnmatched {
        contract: &'a str,
        via: &'a str,
    },
    ViaFallback(&'a str),
    UnlinkedResource(&'a str),
    OrphanProvisioner {
        contract: &'a str,
        place: &'a str,
    },
    OrphanUse {
        contract: &'a str,
        place: &'a str,
    },
    /// The missing reads of the repository's consumers against the routes they call (spec #23).
    Drift(usize),
    /// The config files of a repository (#639): read, skipped as lockfiles or as unreadable,
    /// and the values they hold and redact.
    ConfigFiles {
        read: usize,
        lockfiles: usize,
        unreadable: usize,
        values: usize,
        redacted: usize,
    },
    /// The Kustomize entries of a repository that were not followed (#651): in all, and the
    /// remote ones among them.
    KustomizeUnresolved {
        count: usize,
        remote: usize,
    },
    /// The line for the human at the end of a repository's status when it holds a drift or an
    /// `inferred` link: where a wrong conclusion goes.
    ReportHint,
    /// A source file whose qualified names passed a pack's budget: no symbols, no calls.
    FileTooBroad {
        path: &'a str,
        limit: usize,
    },
    /// A source file of a project that gives its files more usings than a pack takes: no
    /// symbols, no calls.
    FileTooManyUsings {
        path: &'a str,
        limit: usize,
    },
    /// A source file the grammar misreads (a Kotlin declaration read as an infix call): no
    /// symbols, no calls.
    FileMisparsed {
        path: &'a str,
    },
    /// A Backstage plugin whose `pluginId` Meada cannot read (#615): its routers stay at the root.
    BackstageUnmounted {
        site: &'a str,
    },
    /// A Backstage proxy endpoint (#616) whose `target` is neither `${VAR}` nor an `http(s)://`
    /// URL, or whose path to the target is not read (a `pathRewrite` that does more than strip
    /// the endpoint, `ignorePath`, `prependPath`, files that disagree): its `calls` stay unlinked.
    BackstageProxyTarget {
        endpoint: &'a str,
        calls: usize,
    },
    /// A Backstage `$include` of an `app-config*.yaml` or of a file it includes (#689) that is not
    /// followed: the file it is written in, the include as written and why. The keys under it
    /// stay unread.
    BackstageInclude {
        file: &'a str,
        include: &'a str,
        reason: crate::http::IncludeReason,
    },
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
    #[cfg(feature = "postgres")]
    AtMost(u64),
    #[cfg(feature = "postgres")]
    PostgresBoth,
    #[cfg(feature = "postgres")]
    PostgresSchema,
    /// A connection URL that is no `postgres://` URL with a TCP host.
    #[cfg(feature = "postgres")]
    PostgresUrlInvalid,
    #[cfg(feature = "postgres")]
    PostgresUrlQuery,
    #[cfg(feature = "postgres")]
    PostgresUrlUnset(&'a str),
    #[cfg(feature = "postgres")]
    PostgresTimeout(u64),
    /// A schema below the binary's version, or absent.
    #[cfg(feature = "postgres")]
    PostgresVersionLower {
        schema: &'a str,
        stored: i32,
        known: i32,
    },
    /// A schema above the binary's version.
    #[cfg(feature = "postgres")]
    PostgresVersionHigher {
        schema: &'a str,
        stored: i32,
        known: i32,
    },
    #[cfg(feature = "postgres")]
    PostgresReachable {
        schema: &'a str,
        version: i32,
    },
    #[cfg(any(feature = "postgres", feature = "ui"))]
    PostgresUnreachable(&'a str),
    /// `meada postgres migrate` with a manifest that has no `[postgres]`.
    #[cfg(feature = "postgres")]
    PostgresMissing(&'a str),
    #[cfg(feature = "postgres")]
    Postgres,
    #[cfg(feature = "postgres")]
    PostgresMigrate,
    #[cfg(feature = "postgres")]
    PostgresWorkspaceHelp,
    /// `[directory]` naming both `token_env` and `token_file`.
    #[cfg(feature = "directory")]
    DirectoryBoth,
    /// A SCIM token shorter than the least it may be, never echoing it.
    #[cfg(feature = "directory")]
    DirectoryTokenShort(usize),
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
    SnapshotNodeName,
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
    /// A generation document (`key`) whose `workspace` field is not a version id.
    #[cfg(feature = "snapshot")]
    SnapshotBadWorkspace {
        key: &'a str,
        version: &'a str,
    },
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
    #[cfg(any(
        feature = "generation-pointer",
        feature = "github-app",
        feature = "directory",
        all(feature = "oidc", feature = "postgres")
    ))]
    NeedsSections {
        section: &'a str,
        needed: &'a [&'a str],
    },
    /// A section that needs the GitHub grant provider, in a build with local accounts (#562):
    /// `[oidc.github]` or `[accounts.github]`.
    #[cfg(all(feature = "workspaces", feature = "accounts"))]
    NeedsGithubProvider(&'a str),
    #[cfg(any(
        feature = "generation-pointer",
        feature = "oidc",
        feature = "github-app",
        feature = "postgres"
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
    /// An operation would leave no usable fence under Valkey's signed INCR ceiling.
    #[cfg(feature = "refresh-lock")]
    FenceLimit {
        key: &'a str,
        max: u64,
    },
    /// What `fence recover` read and could not count: the problem, named.
    #[cfg(feature = "refresh-lock")]
    FenceNotCounted(&'a str),
    /// A heartbeat key whose value is no heartbeat.
    #[cfg(feature = "generation-pointer")]
    HeartbeatBad(&'a str),
    /// The host name is no node name, and `[snapshot] node_name` is not set.
    #[cfg(feature = "generation-pointer")]
    NodeNameHost(&'a str),
    /// A round's heartbeat that could not be written: the key and why.
    #[cfg(feature = "generation-pointer")]
    HeartbeatNotWritten {
        key: &'a str,
        reason: &'a str,
    },
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
    /// A pinned generation that records no such workspace for the caller (#342).
    #[cfg(feature = "snapshot")]
    GenerationNoContext(&'a str),
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
    #[cfg(any(feature = "oidc", feature = "directory"))]
    OidcEmpty,
    /// An `[oidc]` section with neither `identities_claim` nor `groups_claim`.
    #[cfg(feature = "oidc")]
    OidcNoClaim,
    /// An `[oidc]` key whose value is none of these, never echoing it (`type`, `overage`).
    #[cfg(any(feature = "oidc", feature = "directory"))]
    OidcOneOf(&'a [&'a str]),
    /// The table of an identity provider `type` under another `type`: the one it belongs to.
    #[cfg(feature = "oidc")]
    OidcTypeTable(&'a str),
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
    /// A JWKS that is not that JSON document: where it fails, never what it holds.
    #[cfg(feature = "oidc")]
    OidcJwksShape {
        line: usize,
        column: usize,
    },
    #[cfg(feature = "oidc")]
    OidcRefreshFailed(&'a str),
    /// A poll of the newest permissions set that failed (#332): why, never the URL.
    #[cfg(all(feature = "oidc", feature = "postgres"))]
    PermissionsUnreadable(&'a str),
    /// A schema version this binary does not know, found by a poll: written once.
    #[cfg(all(feature = "oidc", feature = "postgres"))]
    PermissionsVersion(&'a str),
    /// A permissions or directory row or set that fails its checks, naming the table and never
    /// its values.
    #[cfg(all(feature = "oidc", feature = "postgres"))]
    PermissionsBadRow(&'a str),
    /// A lookup of an overage token's groups that failed (#336): why, never the URL or a claim.
    #[cfg(all(feature = "oidc", feature = "directory"))]
    OverageUnreadable(&'a str),
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
    /// `meada github-app roles` (#331).
    #[cfg(feature = "github-app")]
    GithubAppRoles,
    /// A `[oidc.github] host` that is not a lowercase host name.
    #[cfg(feature = "oidc")]
    OidcGithubHost,
    /// `[oidc.github] roles` that is not a non-empty array of distinct names of 1 to this many
    /// characters with no control character.
    #[cfg(feature = "oidc")]
    OidcGithubRoles(usize),
    /// `[oidc.github]` without a `[[github_app]]` block for its host, for the role check.
    #[cfg(feature = "github-app")]
    RolesNoApp(&'a str),
    /// A manifest without `[oidc.github]` given to the role check.
    #[cfg(feature = "github-app")]
    RolesNoGithub(&'a str),
    /// `[refresh_lock]` without a positive `MEADA_REFRESH_FENCE` for the role check.
    #[cfg(feature = "github-app")]
    RolesFenceEnv,
    /// A permissions set with a higher fence than this run's.
    #[cfg(feature = "github-app")]
    RolesFenceBelow {
        fence: u64,
        held: i64,
    },
    /// The run's set was replaced before the exclusions file was written.
    #[cfg(feature = "github-app")]
    RolesReplaced,
    /// An identity on the host that is not `<host>/<owner>/<repo>`.
    #[cfg(feature = "github-app")]
    RolesNotRepository(&'a str),
    #[cfg(feature = "github-app")]
    RolesUncovered(&'a str),
    #[cfg(feature = "github-app")]
    RolesNotFound(&'a str),
    #[cfg(feature = "github-app")]
    RolesForbidden(&'a str),
    /// A collaborator listed without a valid login or role, never echoing either.
    #[cfg(feature = "github-app")]
    RolesSkipped(&'a str),
    /// A listing that fails the run: the identity and why (a status, a timeout, a bad page).
    #[cfg(feature = "github-app")]
    RolesFailed {
        identity: &'a str,
        why: &'a str,
    },
    /// Every installation covering an owner throttled, with the last status answered, if any.
    #[cfg(feature = "github-app")]
    RolesThrottled(Option<u16>),
    /// A `Link` header's next page off the repository's collaborators listing, never echoed.
    #[cfg(feature = "github-app")]
    RolesLinkForeign,
    #[cfg(feature = "github-app")]
    RolesLinkCycle,
    #[cfg(feature = "github-app")]
    RolesPages(u32),
    /// An addition left out, with the reason and never the login.
    #[cfg(feature = "github-app")]
    RolesExcluded {
        identity: &'a str,
        listed: bool,
    },
    /// An addition whose `added_by` has no time or was written by the first permissions set
    /// (#454).
    #[cfg(feature = "github-app")]
    RolesUnattributed(&'a str),
    /// The role check's line on success.
    #[cfg(feature = "github-app")]
    RolesSet {
        id: &'a str,
        repositories: usize,
        rows: usize,
        exclusions: usize,
    },
    /// `[catalog] allow` that is not a non-empty array of distinct lowercase patterns on this
    /// host (#341).
    #[cfg(feature = "workspaces")]
    CatalogAllow(&'a str),
    /// An installation listing that fails the role check (#341): the installation and why.
    #[cfg(feature = "workspaces")]
    RolesCatalogFailed {
        installation: u64,
        why: &'a str,
    },
    /// An App whose installations could not be listed for the catalog: the App id and why.
    #[cfg(feature = "workspaces")]
    RolesCatalogInstallations {
        app: u64,
        why: &'a str,
    },
    /// An installation throttled while the catalog was listed, with the status answered, if any.
    #[cfg(feature = "workspaces")]
    RolesCatalogThrottled(Option<u16>),
    /// An App with no installations held and no recent listing, after waiting for another
    /// run's listing for the lock's lifetime.
    #[cfg(feature = "workspaces")]
    RolesCatalogUnlisted,
    /// More pages of one installation's repositories than the role check lists.
    #[cfg(feature = "workspaces")]
    RolesCatalogPages(u32),
    /// More candidates, besides the selected repositories, than `[catalog] max_repositories`.
    #[cfg(feature = "workspaces")]
    RolesCatalogCap {
        found: usize,
        max: usize,
    },
    /// The role check's line on success under `[catalog]`.
    #[cfg(feature = "workspaces")]
    RolesSetCatalog {
        id: &'a str,
        repositories: usize,
        catalog: usize,
        rows: usize,
        exclusions: usize,
    },
    /// A catalog query the console refuses: its parameters, never echoed.
    #[cfg(feature = "workspaces")]
    CatalogQuery(usize),
    /// A catalog check whose body is not `{"identities": [...]}` of identities.
    #[cfg(feature = "workspaces")]
    CatalogCheckBody(usize),
    /// An entry of a named workspace left out before the fetch because no operator of the
    /// workspace holds an accepted role on it (#342): the workspace and the identity.
    #[cfg(feature = "workspaces")]
    RolesNoSponsor {
        context: &'a str,
        identity: &'a str,
    },
    /// `[workspaces] mirrors` that is not an absolute path without `..` (#342).
    #[cfg(feature = "workspaces")]
    WorkspacesMirrors,
    /// `[workspaces]` without `[ui] operations = true` (#342).
    #[cfg(feature = "workspaces")]
    WorkspacesNeedsOperations,
    /// `[workspaces]` without `[workspace] source = "store"` (#342).
    #[cfg(feature = "workspaces")]
    WorkspacesNeedsStore,
    /// The contexts file a reader needs, missing (#342): the pull that writes it.
    #[cfg(feature = "workspaces")]
    ContextsMissing(&'a str),
    /// A contexts file that does not check (#342).
    #[cfg(feature = "workspaces")]
    ContextsBad(&'a str),
    /// The contexts file the exclusions file was not written for (#342).
    #[cfg(feature = "workspaces")]
    ContextsOther(&'a str),
    /// A named version document that does not check (#342).
    #[cfg(feature = "workspaces")]
    WorkspacesDocumentBad(&'a str),
    /// A creation body that is not `{name, description?, entries, message}` (#342).
    #[cfg(feature = "workspaces")]
    WorkspacesBody,
    /// A workspace that is not found, hidden or deleted: one answer for all (#342).
    #[cfg(feature = "workspaces")]
    WorkspacesNotFound,
    /// The store or Postgres did not answer a workspace write or read (#342).
    #[cfg(feature = "workspaces")]
    WorkspacesUnavailable,
    /// A creation refused because some selected repository is not eligible (#342): how many.
    #[cfg(feature = "workspaces")]
    WorkspacesRefused(usize),
    /// A creation or selection while the permissions set cannot decide (#342): the reason.
    #[cfg(feature = "workspaces")]
    WorkspacesPending(crate::workspaces::Reason),
    /// More workspaces than `[workspaces] max_workspaces` (#342).
    #[cfg(feature = "workspaces")]
    WorkspacesLimit(usize),
    /// The same triple created and then deleted (#342).
    #[cfg(feature = "workspaces")]
    WorkspaceDeleted,
    /// A list query the console refuses (#342): its parameters, never echoed.
    #[cfg(feature = "workspaces")]
    WorkspacesQuery(usize),
    /// The line `meada workspace pull` prints for the contexts it wrote (#342).
    #[cfg(feature = "workspaces")]
    ContextsWritten(usize),
    /// A named workspace's entry names an identity outside the catalog (#342).
    #[cfg(feature = "workspaces")]
    WorkspacesReservedSegment(&'a str),
    /// `meada workspaces` and its subcommands' descriptions (#343).
    #[cfg(feature = "workspaces")]
    WorkspacesCmd,
    #[cfg(feature = "workspaces")]
    WorkspacesCmdList,
    #[cfg(feature = "workspaces")]
    WorkspacesCmdShow,
    #[cfg(feature = "workspaces")]
    WorkspacesCmdVersions,
    #[cfg(feature = "workspaces")]
    WorkspacesCmdDiff,
    #[cfg(feature = "workspaces")]
    WorkspacesHelpAgainst,
    #[cfg(feature = "workspaces")]
    WorkspacesCmdCreate,
    #[cfg(feature = "workspaces")]
    WorkspacesCmdSave,
    #[cfg(feature = "workspaces")]
    WorkspacesCmdApply,
    #[cfg(feature = "workspaces")]
    WorkspacesCmdRename,
    #[cfg(feature = "workspaces")]
    WorkspacesCmdDelete,
    #[cfg(feature = "workspaces")]
    WorkspacesCmdOperators,
    #[cfg(feature = "workspaces")]
    WorkspacesHelpId,
    #[cfg(feature = "workspaces")]
    WorkspacesHelpBase,
    #[cfg(feature = "workspaces")]
    WorkspacesHelpName,
    #[cfg(feature = "workspaces")]
    WorkspacesHelpDescription,
    #[cfg(feature = "workspaces")]
    WorkspacesHelpAdd,
    #[cfg(feature = "workspaces")]
    WorkspacesHelpRemove,
    /// A lifecycle body that is not the shape its route takes (#343): the shape.
    #[cfg(feature = "workspaces")]
    WorkspacesOpBody(&'a str),
    /// A write whose `base` is not the workspace's `seq` any more (#343): the current one.
    #[cfg(feature = "workspaces")]
    WorkspacesStale(i64),
    /// A write by someone who neither operates the workspace nor administers the platform (#343).
    #[cfg(feature = "workspaces")]
    WorkspacesNotOperator,
    /// The operators' route, which only the platform administrator may use (#343).
    #[cfg(feature = "workspaces")]
    WorkspacesNotAdministrator,
    /// A version that is not one of the workspace's (#343).
    #[cfg(feature = "workspaces")]
    WorkspacesUnknownVersion,
    /// An apply of the version that is current (#343).
    #[cfg(feature = "workspaces")]
    WorkspacesAlreadyCurrent,
    /// `meada workspaces mcp` and its arguments' help (ADR 0022).
    #[cfg(feature = "proposals")]
    ProposalsCmd,
    #[cfg(feature = "proposals")]
    ProposalsListenHelp,
    #[cfg(feature = "proposals")]
    ProposalsWorkspaceHelp,
    /// A bootstrap manifest without `[proposals]` given to `meada workspaces mcp` (ADR 0022).
    #[cfg(feature = "proposals")]
    ProposalsNoSection,
    /// A tool's argument past its bound (ADR 0022): what the argument takes, never its value.
    #[cfg(feature = "proposals")]
    ProposalsArgument(&'a str),
    /// A tool's answer past the output cap (ADR 0022): the cap in bytes.
    #[cfg(feature = "proposals")]
    ProposalsTooLarge(usize),
    /// A call while the most calls are in flight (ADR 0022).
    #[cfg(feature = "proposals")]
    ProposalsBusy,
    /// A call that reached a tool without the admission the listener gives (ADR 0022).
    #[cfg(feature = "proposals")]
    ProposalsNotAdmitted,
    /// An operator list that would hold none or too many (#343): the reason.
    #[cfg(feature = "workspaces")]
    WorkspacesOperators(&'a str),
    /// A versions or history query the console refuses (#343): its cursor bound, never echoed.
    #[cfg(feature = "workspaces")]
    WorkspacesPageQuery(usize),
    /// A save whose stored version would hold no entry (#343).
    #[cfg(feature = "workspaces")]
    WorkspacesEmptyVersion,
    /// The exclusions file a reader of the pulled version needs (#331).
    #[cfg(feature = "generation-pointer")]
    ExcludedMissing(&'a str),
    #[cfg(feature = "generation-pointer")]
    ExcludedOther {
        file: &'a str,
        named: &'a str,
        pulled: &'a str,
    },
    #[cfg(feature = "generation-pointer")]
    ExcludedBad {
        file: &'a str,
        line: usize,
    },
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
    /// A request to the console with no valid bearer token (feature `oidc`).
    #[cfg(all(feature = "ui", feature = "oidc"))]
    UiUnauthorized,
    /// A valid token to the console without every configured scope.
    #[cfg(all(feature = "ui", feature = "oidc"))]
    UiScope,
    /// A token check whose body is no `{"token": "<JWT>"}`.
    #[cfg(all(feature = "ui", feature = "oidc"))]
    UiCheckBody,
    /// A request body past its route's cap, in bytes.
    #[cfg(feature = "ui")]
    UiTooLarge(usize),
    /// A playground query whose body is not a query of one of the tools.
    #[cfg(feature = "ui")]
    UiQueryBody,
    /// A repository the console does not serve, or one outside the viewer's grant.
    #[cfg(feature = "ui")]
    UiNoRepository(&'a str),
    /// A generation the console does not hold, and the ones it holds.
    #[cfg(feature = "ui")]
    UiNotHeld {
        id: &'a str,
        held: &'a [String],
    },
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
    /// A complete operational read, including permit wait and blocking I/O, timed out.
    #[cfg(feature = "ui")]
    UiReadTimeout(u64),
    /// Console operator configuration requires OIDC.
    #[cfg(feature = "ui")]
    UiNeedsOidc,
    /// Console writes need an admission that names an operator: `[oidc]` or `[accounts]`.
    #[cfg(all(feature = "ui", feature = "accounts"))]
    UiNeedsAdmission,
    /// Enabled writes require one operator admission key.
    #[cfg(feature = "ui")]
    UiOperatorNeeded,
    /// Mutually exclusive console operator keys.
    #[cfg(feature = "ui")]
    UiOperatorBoth,
    /// A refused operation before admission or body consumption.
    #[cfg(feature = "ui")]
    UiWritesDisabled,
    /// A write needs an admitted operator with an authenticated subject.
    #[cfg(feature = "ui")]
    UiOperatorRequired,
    /// A team operator asking for an action that stays the global operator's (ADR 0015).
    #[cfg(feature = "ui")]
    UiGlobalOperatorRequired,
    /// A fixed audit refusal that never records invalid editor identities or text.
    #[cfg(feature = "ui")]
    UiEntriesInvalid,
    /// A fixed body error that never echoes untrusted JSON.
    #[cfg(feature = "ui")]
    UiWriteBody,
    /// A CLI twin of a console write whose audit record cannot be written (#328).
    #[cfg(any(feature = "ui", feature = "generation-pointer"))]
    AuditCli(&'a str),
    /// The console's 503 `audit_unavailable`: no write without its record (#328).
    #[cfg(feature = "ui")]
    AuditUnavailable,
    /// An audit row that fails its checks, naming the table and never its values.
    #[cfg(feature = "ui")]
    AuditBadRecord(&'a str),
    /// `GET audit` with a query other than `before=<id>` and `limit=<1 to 100>`.
    #[cfg(feature = "ui")]
    UiAuditQuery,
    /// A subject the identity provider deactivated (#335): the 403 `deprovisioned`.
    #[cfg(all(feature = "ui", feature = "directory"))]
    UiDeprovisioned,
    /// `[accounts]` with both keys of a pair: the other key's name (ADR 0024).
    #[cfg(feature = "accounts")]
    AccountsBoth(&'a str),
    /// A variable `[accounts]` names that is not set.
    #[cfg(feature = "accounts")]
    AccountsUnset(&'a str),
    /// A variable `[accounts]` names whose value is not UTF-8.
    #[cfg(feature = "accounts")]
    AccountsNotUtf8,
    /// `[accounts]` naming no public or private key, at the console's start.
    #[cfg(feature = "accounts")]
    AccountsKeyMissing {
        env: &'a str,
        file: &'a str,
        private: bool,
    },
    /// A key of `[accounts]` that is no Ed25519 key in PEM.
    #[cfg(feature = "accounts")]
    AccountsNotEd25519 {
        private: bool,
    },
    /// A private key that does not sign what the public key verifies.
    #[cfg(feature = "accounts")]
    AccountsKeyMismatch,
    /// A node with `[accounts]` and no `[oidc]` whose manifest has no `[ui]`: its `public_url` is
    /// the issuer of every agent token (#560).
    #[cfg(feature = "accounts")]
    AccountsNodeIssuer,
    /// `POST account/tokens` whose body does not parse (#560).
    #[cfg(feature = "accounts")]
    AccountsTokenBody(u64),
    /// An agent token for a surface the console does not issue for; whether it issues for the
    /// proposal server's.
    #[cfg(feature = "accounts")]
    AccountsTokenSurface(bool),
    /// An agent token's validity outside its bounds, in hours.
    #[cfg(feature = "accounts")]
    AccountsTokenHours(u64),
    /// An agent token for a suspended or unknown account: the role refusal of the write.
    #[cfg(feature = "accounts")]
    AccountsTokenRefused,
    /// The audit message of an agent token issued.
    #[cfg(feature = "accounts")]
    AccountsTokenIssued,
    /// `POST account/github` whose body does not parse (#561).
    #[cfg(feature = "accounts")]
    AccountsGithubBody,
    /// A pasted personal access token that is no header value.
    #[cfg(feature = "accounts")]
    AccountsGithubToken,
    /// A verified login change of a suspended or unknown account: the role refusal of the write.
    #[cfg(feature = "accounts")]
    AccountsGithubForbidden,
    /// GitHub refused the pasted personal access token.
    #[cfg(feature = "accounts")]
    AccountsGithubRefused,
    /// GitHub's `login` that is no login.
    #[cfg(feature = "accounts")]
    AccountsGithubLogin,
    /// The stderr line of GitHub out of reach, with the reason (never the token nor the body).
    #[cfg(feature = "accounts")]
    AccountsGithubUnreachable(&'a str),
    /// The answer of GitHub out of reach.
    #[cfg(feature = "accounts")]
    AccountsGithubUnavailable,
    /// A login another account holds.
    #[cfg(feature = "accounts")]
    AccountsLoginTaken,
    /// The audit message of a verified login stored.
    #[cfg(feature = "accounts")]
    AccountsGithubVerified,
    /// The audit message of a verified login removed.
    #[cfg(feature = "accounts")]
    AccountsGithubRemoved,
    /// `[accounts]` beside `[oidc]` without `break_glass = true`: local sign-in is off once an
    /// identity provider is configured.
    #[cfg(feature = "accounts")]
    AccountsBesideOidc,
    /// `[accounts] break_glass` without `[oidc]`: it keeps the default account beside an identity
    /// provider, and there is none.
    #[cfg(feature = "accounts")]
    AccountsBreakGlassNeedsOidc,
    /// A table of `[accounts]` (`groups`, `github`) under `break_glass`: the break-glass account is
    /// the global operator and needs no grant. The table's dotted name.
    #[cfg(feature = "accounts")]
    AccountsUnderBreakGlass(&'a str),
    /// `[oidc] issuer` equal to `[ui] public_url` under break glass: a token's issuer could not
    /// tell the identity provider's tokens from the console's own.
    #[cfg(all(feature = "ui", feature = "accounts"))]
    AccountsBreakGlassIssuer,
    /// The password policy's rules, each named, never the password.
    #[cfg(feature = "accounts")]
    AccountsPasswordShort(usize),
    #[cfg(feature = "accounts")]
    AccountsPasswordLong(usize),
    #[cfg(feature = "accounts")]
    AccountsPasswordUsername,
    #[cfg(feature = "accounts")]
    AccountsPasswordCurrent,
    #[cfg(feature = "accounts")]
    AccountsPasswordCommon,
    /// `meada ui reset-admin` on a manifest without `[accounts]`: the manifest.
    #[cfg(all(feature = "ui", feature = "accounts"))]
    AccountsResetNoSection(&'a str),
    /// The audit message of `meada ui reset-admin`: whether it created `admin`.
    #[cfg(all(feature = "ui", feature = "accounts"))]
    AccountsResetAdmin {
        created: bool,
    },
    /// `meada ui reset-admin` done, on stderr: whether it created `admin`, and the key whose
    /// password it set (none: the generated one, on stdout).
    #[cfg(all(feature = "ui", feature = "accounts"))]
    AccountsResetDone {
        created: bool,
        from: Option<&'a str>,
    },
    /// `meada ui reset-admin`'s description and its `--workspace`.
    #[cfg(all(feature = "ui", feature = "accounts"))]
    UiResetAdmin,
    #[cfg(all(feature = "ui", feature = "accounts"))]
    UiResetAdminWorkspaceHelp,
    /// The default account could not be made at the console's start: Postgres's reason.
    #[cfg(feature = "accounts")]
    AccountsBootstrap(&'a str),
    /// The 503 `busy` of a hash past the console's bound.
    #[cfg(feature = "accounts")]
    AccountsBusy,
    /// The one 401 `invalid_credentials` of a sign-in or a password change.
    #[cfg(feature = "accounts")]
    AccountsInvalid,
    /// A request to the console with no valid session token.
    #[cfg(feature = "accounts")]
    AccountsUnauthorized,
    /// A session token with the change-required mark, on any route but the password change.
    #[cfg(feature = "accounts")]
    AccountsChangeRequired,
    /// A sign-in whose body is no `{"username", "password"}`.
    #[cfg(feature = "accounts")]
    AccountsLoginBody,
    /// A password change whose body is no `{"current", "new"}`.
    #[cfg(feature = "accounts")]
    AccountsPasswordBody,
    /// An accounts row that fails its checks, naming the table and never its values.
    #[cfg(feature = "accounts")]
    AccountsBadRow(&'a str),
    /// The audit message of an accepted sign-in.
    #[cfg(feature = "accounts")]
    AccountsSignedIn,
    /// The audit message of a password change.
    #[cfg(feature = "accounts")]
    AccountsPasswordChanged,
    /// The accounts table could not be written.
    #[cfg(feature = "accounts")]
    AccountsUnavailable,
    /// A session token the console could not sign.
    #[cfg(feature = "accounts")]
    AccountsUnsigned,
    /// Account administration by anyone but the global operator (#557).
    #[cfg(feature = "accounts")]
    AccountsOperatorRequired,
    /// An account creation whose body is no `{"username", "role", "password"}`.
    #[cfg(feature = "accounts")]
    AccountsCreateBody,
    /// An account change whose body is no one action with its fields.
    #[cfg(feature = "accounts")]
    AccountsChangeBody,
    /// A username that is not 1 to 64 of the allowed characters.
    #[cfg(feature = "accounts")]
    AccountsUsernameInvalid,
    /// An account's group that is no name an entry's owner may be (#558).
    #[cfg(feature = "accounts")]
    AccountsGroupInvalid,
    /// A creation's username another account holds.
    #[cfg(feature = "accounts")]
    AccountsTaken,
    /// No account of that username.
    #[cfg(feature = "accounts")]
    AccountsUnknown,
    /// A suspension or a demotion of the last active operator.
    #[cfg(feature = "accounts")]
    AccountsLastOperator,
    /// The audit message of an account created.
    #[cfg(feature = "accounts")]
    AccountsCreated,
    /// The audit message of an account's role changed.
    #[cfg(feature = "accounts")]
    AccountsRoleChanged,
    /// The audit message of an account's groups changed (#558).
    #[cfg(feature = "accounts")]
    AccountsGroupsChanged,
    /// The audit message of an account suspended.
    #[cfg(feature = "accounts")]
    AccountsSuspended,
    /// The audit message of an account reactivated.
    #[cfg(feature = "accounts")]
    AccountsReactivated,
    /// The audit message of an account's password reset.
    #[cfg(feature = "accounts")]
    AccountsPasswordReset,
    /// The directory could not be read: the stderr line, naming the directory and never a claim.
    #[cfg(all(feature = "ui", feature = "directory"))]
    DirectoryUnreadable(&'a str),
    /// The 503 of a route that answers from the directory alone.
    #[cfg(all(feature = "ui", feature = "directory"))]
    DirectoryUnavailable,
    /// The directory's state row (migration 4) is missing: why the directory cannot be read.
    #[cfg(all(feature = "ui", feature = "directory"))]
    DirectoryStateMissing,
    /// `GET directory/groups` with a query other than one `prefix` of at most `OWNER_MAX`
    /// characters.
    #[cfg(all(feature = "ui", feature = "directory"))]
    UiDirectoryQuery(usize),
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
    /// `[workspace] source = "store"` in a build without `generation-pointer`.
    #[cfg(not(feature = "generation-pointer"))]
    WorkspaceStoreNeedsFeature,
    /// `[[repository]]` entries beside `[workspace] source = "store"`.
    #[cfg(feature = "generation-pointer")]
    WorkspaceStoreBeside,
    /// `owner` in the `[[repository]]` entries of a bootstrap manifest (`source = "file"`).
    WorkspaceOwnerStatic,
    /// `added_by` in the `[[repository]]` entries of a bootstrap manifest (`source = "file"`).
    WorkspaceAddedByStatic,
    /// `meada mcp --listen` without `--generations`, given a `source = "store"` manifest.
    #[cfg(feature = "generation-pointer")]
    WorkspaceStoreRefused(&'a str),
    /// A command that reads the pulled workspace version, with none under `dir`.
    #[cfg(feature = "generation-pointer")]
    WorkspaceNotPulled {
        manifest: &'a str,
        dir: &'a str,
    },
    /// The pulled version's current file (`<index_dir>/workspace/current`), malformed.
    #[cfg(feature = "generation-pointer")]
    WorkspaceCurrentBad(&'a str),
    /// A pull whose pointer's fence is not above the current file's.
    #[cfg(feature = "generation-pointer")]
    WorkspaceFenceBelow {
        key: &'a str,
        id: &'a str,
        fence: u64,
        held: u64,
        file: &'a str,
        fence_key: &'a str,
    },
    /// `meada workspace pull` when the current file already names the pointer's version.
    #[cfg(feature = "generation-pointer")]
    WorkspaceAlreadyCurrent(&'a str),
    #[cfg(feature = "generation-pointer")]
    WorkspacePull,
    #[cfg(feature = "refresh-lock")]
    WorkspaceApply,
    #[cfg(feature = "refresh-lock")]
    WorkspaceApplyVersion,
    #[cfg(feature = "generation-pointer")]
    Workspace,
    #[cfg(feature = "generation-pointer")]
    WorkspaceImport,
    #[cfg(feature = "generation-pointer")]
    WorkspaceList,
    #[cfg(feature = "generation-pointer")]
    WorkspaceShow,
    #[cfg(feature = "generation-pointer")]
    WorkspaceImportFile,
    #[cfg(feature = "generation-pointer")]
    WorkspaceImportMessage,
    #[cfg(feature = "generation-pointer")]
    WorkspaceImportAuthor,
    #[cfg(feature = "generation-pointer")]
    WorkspaceShowVersion,
    #[cfg(feature = "generation-pointer")]
    WorkspaceShowPulled,
    #[cfg(feature = "generation-pointer")]
    WorkspaceBootstrapHelp,
    /// A bootstrap manifest given to `meada workspace` without `[generations]`.
    #[cfg(feature = "generation-pointer")]
    WorkspaceNoSections(&'a str),
    #[cfg(feature = "generation-pointer")]
    BadVersionArg(&'a str),
    #[cfg(feature = "generation-pointer")]
    WorkspaceNoCurrent(&'a str),
    #[cfg(feature = "generation-pointer")]
    WorkspaceBadPointer(&'a str),
    #[cfg(feature = "generation-pointer")]
    WorkspaceNotVersion(&'a str),
    #[cfg(feature = "generation-pointer")]
    WorkspaceNoVersion(&'a str),
    /// How many entries a version's apply may not change (ADR 0015); the console answers its
    /// English text.
    #[cfg(feature = "refresh-lock")]
    WorkspaceRefused(usize),
    /// A team operator whose groups own no entry it reads in the current version any more.
    #[cfg(feature = "refresh-lock")]
    WorkspaceRoleLost,
    /// A serialized import that its bounded reader could not read back.
    #[cfg(feature = "generation-pointer")]
    WorkspaceVersionTooLarge {
        key: &'a str,
        max: u64,
    },
    /// A key or section of an imported file that is not a `[[repository]]` entry.
    #[cfg(feature = "generation-pointer")]
    WorkspaceNotImported,
    /// An import whose entries are stored already: the version as first stored.
    #[cfg(feature = "generation-pointer")]
    WorkspaceStored {
        id: &'a str,
        author: &'a str,
        time: &'a str,
        message: &'a str,
    },
}

impl Msg<'_> {
    pub fn text(&self) -> String {
        use meada_core::msg::Msg as Core;
        let lang = meada_core::msg::lang();
        // `Lang` is non_exhaustive: a language this catalog lacks reads English.
        let pick = |en: String, pt_br: String, es: String| match lang {
            Lang::Pt => pt_br,
            Lang::Es => es,
            _ => en,
        };
        match self {
            Msg::NotIndexed => pick(
                "not indexed".into(),
                "não indexado".into(),
                "no indexado".into(),
            ),
            Msg::OtherVersion => pick(
                "indexed by another version; rebuilt on next use".into(),
                "indexado por outra versão; reconstruído no próximo uso".into(),
                "indexado por otra versión; reconstruido en el próximo uso".into(),
            ),
            Msg::Indexed(n) => pick(
                format!("indexed, {n} files"),
                format!("indexado, {n} arquivos"),
                format!("indexado, {n} archivos"),
            ),
            Msg::OtherVersionWorkspace => pick(
                "indexed by another version; rebuilt by meada index --workspace".into(),
                "indexado por outra versão; reconstruído por meada index --workspace".into(),
                "indexado por otra versión; reconstruido por meada index --workspace".into(),
            ),
            Msg::IndexFailed(1) => pick(
                "index failed for 1 repository".into(),
                "a indexação falhou em 1 repositório".into(),
                "la indexación falló en 1 repositorio".into(),
            ),
            Msg::IndexFailed(n) => pick(
                format!("index failed for {n} repositories"),
                format!("a indexação falhou em {n} repositórios"),
                format!("la indexación falló en {n} repositorios"),
            ),
            Msg::ListedTwice => Core::ListedTwice.text(),
            Msg::NoMirror(path) => pick(
                format!("`{path}` does not exist"),
                format!("`{path}` não existe"),
                format!("`{path}` no existe"),
            ),
            Msg::NotACheckout(path) => pick(
                format!("`{path}` is not the root of a checkout with a working tree"),
                format!("`{path}` não é a raiz de um checkout com árvore de trabalho"),
                format!("`{path}` no es la raíz de un checkout con árbol de trabajo"),
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
                format!(
                    "git rechaza `{path}`, que pertenece a otro usuario: Meada necesita git 2.38 \
                     o posterior para leerlo, o `safe.directory` en la configuración de sistema \
                     de git"
                ),
            ),
            Msg::IdentityMismatch { found, repo } => pick(
                format!("the mirror's remote is `{found}`, not `{repo}`"),
                format!("o remoto do espelho é `{found}`, não `{repo}`"),
                format!("el remoto del espejo es `{found}`, no `{repo}`"),
            ),
            Msg::NotListed { repo, served } => pick(
                format!("`{repo}` is not listed in the workspace manifest; it serves {served}"),
                format!("`{repo}` não está listado no manifesto do workspace; ele serve {served}"),
                format!("`{repo}` no está listado en el manifiesto del workspace; sirve {served}"),
            ),
            Msg::ListenHelp => pick(
                "Server mode: serve over MCP streamable HTTP at host:port, :port or port \
                 (loopback without a host), with --workspace"
                    .into(),
                "Modo servidor: serve por MCP streamable HTTP em host:porta, :porta ou porta \
                 (loopback sem host), com --workspace"
                    .into(),
                "Modo servidor: sirve por MCP streamable HTTP en host:puerto, :puerto o puerto \
                 (loopback sin host), con --workspace"
                    .into(),
            ),
            Msg::NotAdmitted => pick(
                "the request was not admitted".into(),
                "a requisição não foi admitida".into(),
                "la solicitud no fue admitida".into(),
            ),
            Msg::RepoHelp => pick(
                "Server mode: the identity of the repository to export".into(),
                "Modo servidor: a identidade do repositório a exportar".into(),
                "Modo servidor: la identidad del repositorio a exportar".into(),
            ),
            Msg::IgnoredBlock { block, repo } => pick(
                format!(
                    "manifest block ignored: {block} of {repo}, whose mirror has its own meada.toml"
                ),
                format!(
                    "bloco do manifesto ignorado: {block} de {repo}, cujo espelho tem seu próprio \
                     meada.toml"
                ),
                format!(
                    "bloque del manifiesto ignorado: {block} de {repo}, cuyo espejo tiene su \
                     propio meada.toml"
                ),
            ),
            Msg::IndexMissing { repo, fix } => match fix {
                Fix::Index(manifest) => pick(
                    format!("{repo}: not indexed; run meada index --workspace {manifest}"),
                    format!("{repo}: não indexado; rode meada index --workspace {manifest}"),
                    format!("{repo}: no indexado; ejecuta meada index --workspace {manifest}"),
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
                    format!(
                        "{repo}: sin índice en la generación {id}; elimina {dir} y vuelve a \
                         ejecutar meada snapshot pull, o publica una nueva generación"
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
                    format!(
                        "{repo}: indexado por otra versión; ejecuta meada index --workspace \
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
                    format!(
                        "{repo}: indexado por otra versión en la generación {id}; publica una \
                         generación de este build y ejecuta meada snapshot pull"
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
                    format!(
                        "{repo}: su índice guarda un commit interrumpido; ejecuta meada index \
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
                    format!(
                        "{repo}: su índice en la generación {id} guarda un commit interrumpido; \
                         elimina {dir} y vuelve a ejecutar meada snapshot pull, o publica una \
                         nueva generación"
                    ),
                ),
            },
            #[cfg(feature = "snapshot")]
            Msg::IndexNotRollback(repo) => pick(
                format!("{repo}: index.db: not a database in rollback journal mode"),
                format!("{repo}: index.db: não é um banco em modo rollback journal"),
                format!("{repo}: index.db: no es una base de datos en modo rollback journal"),
            ),
            #[cfg(feature = "snapshot")]
            Msg::IndexSchemaObject { repo, kind, name } => pick(
                format!(
                    "{repo}: index.db: the {kind} {name} is not one this build's schema creates"
                ),
                format!(
                    "{repo}: index.db: o {kind} {name} não é um dos que o esquema deste build cria"
                ),
                format!(
                    "{repo}: index.db: el {kind} {name} no es uno de los que crea el esquema de \
                     este build"
                ),
            ),
            Msg::StateLocked => pick(
                "locked by a commit in progress past the busy timeout".into(),
                "travado por um commit em andamento além do tempo de espera".into(),
                "bloqueado por un commit en curso más allá del tiempo de espera".into(),
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
                format!(
                    "{repo}: el índice `{index}` quedó bloqueado por un commit en curso durante \
                     más de {timeout:?}"
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
                format!(
                    "el índice `{index}` no pudo ser reemplazado por su reconstrucción ({error}); \
                     quedó como estaba"
                ),
            ),
            Msg::RebuildLock { lock, error } => pick(
                format!("cannot take the rebuild lock `{lock}`: {error}"),
                format!("não foi possível pegar a trava de reconstrução `{lock}`: {error}"),
                format!("no se pudo tomar el bloqueo de reconstrucción `{lock}`: {error}"),
            ),
            Msg::IndexInMirror { index, mirror } => pick(
                format!("its index directory `{index}` would be inside the mirror `{mirror}`"),
                format!("seu diretório de índice `{index}` ficaria dentro do espelho `{mirror}`"),
                format!("su directorio de índice `{index}` quedaría dentro del espejo `{mirror}`"),
            ),
            Msg::WorkspaceHelp => pick(
                "Server mode: every repository the workspace manifest at this path lists".into(),
                "Modo servidor: todo repositório que o manifesto do workspace neste caminho lista"
                    .into(),
                "Modo servidor: todo repositorio que lista el manifiesto del workspace en esta ruta"
                    .into(),
            ),
            Msg::MissingPeer { repo, named_by } => pick(
                format!("missing peer: {repo}, named by {named_by}"),
                format!("par não encontrado: {repo}, citado por {named_by}"),
                format!("par no encontrado: {repo}, citado por {named_by}"),
            ),
            Msg::StaleDecision { call, target, cause } => {
                let (en, pt_br, es) = match cause {
                    Cause::CallGone => ("call site gone", "chamada removida", "llamada eliminada"),
                    Cause::ItemNotOpen => (
                        "item no longer open",
                        "item não está mais aberto",
                        "el ítem ya no está abierto",
                    ),
                    Cause::TargetGone => ("target gone", "alvo removido", "destino eliminado"),
                    Cause::NotCandidate => (
                        "target no longer a candidate",
                        "alvo não é mais candidato",
                        "el destino ya no es candidato",
                    ),
                };
                pick(
                    format!("stale decision: {call} -> {target} ({en})"),
                    format!("decisão obsoleta: {call} -> {target} ({pt_br})"),
                    format!("decisión obsoleta: {call} -> {target} ({es})"),
                )
            }
            Msg::WaitingDecision { call, target, peer } => pick(
                format!("decision waiting on missing peer {peer}: {call} -> {target}"),
                format!("decisão aguardando o par ausente {peer}: {call} -> {target}"),
                format!("decisión a la espera del par ausente {peer}: {call} -> {target}"),
            ),
            Msg::ViaUnmatched { contract, via } => pick(
                format!("{contract}: `via` {via} matches no symbol, ignored"),
                format!("{contract}: `via` {via} não corresponde a nenhum símbolo, ignorado"),
                format!("{contract}: `via` {via} no corresponde a ningún símbolo, ignorado"),
            ),
            Msg::ViaFallback(contract) => pick(
                format!("{contract}: no `via` matches a symbol, so the repository as a whole is the side"),
                format!("{contract}: nenhum `via` corresponde a um símbolo, então o repositório inteiro é o lado"),
                format!("{contract}: ningún `via` corresponde a un símbolo, así que el repositorio completo es el lado"),
            ),
            Msg::UnlinkedResource(contract) => pick(
                format!("{contract} is unlinked: no writer and reader in different repositories"),
                format!("{contract} sem ligação: nenhum escritor e leitor em repositórios diferentes"),
                format!("{contract} sin vínculo: ningún escritor y lector en repositorios diferentes"),
            ),
            Msg::OrphanProvisioner { contract, place } => pick(
                format!("orphan {contract}: no use; {place}"),
                format!("órfão {contract}: nenhum uso; {place}"),
                format!("huérfano {contract}: ningún uso; {place}"),
            ),
            Msg::OrphanUse { contract, place } => pick(
                format!("orphan {contract}: no provisioner; {place}"),
                format!("órfão {contract}: nenhum provisionador; {place}"),
                format!("huérfano {contract}: ningún provisionador; {place}"),
            ),
            Msg::Drift(n) => pick(
                format!("shape drift, missing reads: {n}"),
                format!("drift de formato, leituras ausentes: {n}"),
                format!("drift de formato, lecturas ausentes: {n}"),
            ),
            Msg::ConfigFiles {
                read,
                lockfiles,
                unreadable,
                values,
                redacted,
            } => {
                let skipped = lockfiles + unreadable;
                pick(
                    format!(
                        "config files: {read} read, {skipped} skipped (lockfiles: {lockfiles}, \
                         unreadable: {unreadable}); values: {values}, redacted: {redacted}"
                    ),
                    format!(
                        "arquivos de configuração: {read} lidos, {skipped} ignorados \
                         (lockfiles: {lockfiles}, ilegíveis: {unreadable}); valores: {values}, \
                         ocultados: {redacted}"
                    ),
                    format!(
                        "archivos de configuración: {read} leídos, {skipped} omitidos \
                         (lockfiles: {lockfiles}, ilegibles: {unreadable}); valores: {values}, \
                         ocultados: {redacted}"
                    ),
                )
            }
            Msg::KustomizeUnresolved { count, remote } => pick(
                format!("kustomize entries not followed: {count} (remote, never fetched: {remote})"),
                format!("entradas do kustomize não seguidas: {count} (remotas, nunca baixadas: {remote})"),
                format!("entradas de kustomize no seguidas: {count} (remotas, nunca descargadas: {remote})"),
            ),
            Msg::ReportHint => pick(
                format!("wrong conclusion? report it: {REPORT_URL}"),
                format!("conclusão errada? relate: {REPORT_URL}"),
                format!("¿conclusión errónea? repórtala: {REPORT_URL}"),
            ),
            Msg::FileTooBroad { path, limit } => pick(
                format!("{path}: too broad, its qualified names passed the {limit}-byte budget: no symbols or calls"),
                format!("{path}: abrangente demais, seus nomes qualificados passaram do orçamento de {limit} bytes: sem símbolos nem chamadas"),
                format!("{path}: demasiado amplio, sus nombres calificados superaron el presupuesto de {limit} bytes: sin símbolos ni llamadas"),
            ),
            Msg::FileTooManyUsings { path, limit } => pick(
                format!("{path}: its project gives more than {limit} usings: no symbols or calls"),
                format!("{path}: seu projeto dá mais de {limit} usings: sem símbolos nem chamadas"),
                format!("{path}: su proyecto da más de {limit} usings: sin símbolos ni llamadas"),
            ),
            Msg::FileMisparsed { path } => pick(
                format!("{path}: left out, the grammar misreads it: no symbols or calls"),
                format!("{path}: deixado de fora, a gramática o lê errado: sem símbolos nem chamadas"),
                format!("{path}: dejado fuera, la gramática lo lee mal: sin símbolos ni llamadas"),
            ),
            Msg::BackstageUnmounted { site } => pick(
                format!("{site}: Backstage plugin whose pluginId is neither a string literal nor a const of its file bound to one: its routes stay at the root, not under /api/<pluginId>"),
                format!("{site}: plugin do Backstage cujo pluginId não é literal de texto nem const do seu arquivo ligada a um: suas rotas ficam na raiz, não sob /api/<pluginId>"),
                format!("{site}: plugin de Backstage cuyo pluginId no es literal de texto ni const de su archivo ligada a uno: sus rutas quedan en la raíz, no bajo /api/<pluginId>"),
            ),
            Msg::BackstageProxyTarget { endpoint, calls } => pick(
                format!("Backstage proxy endpoint {endpoint}: its target or the path it gets is not read (a target neither ${{VAR}} nor an http(s) URL, a pathRewrite that does more than strip the endpoint, ignorePath, prependPath, or app-config files that disagree), so {calls} call(s) through it stay unlinked"),
                format!("endpoint de proxy do Backstage {endpoint}: seu target ou o caminho que ele recebe não é lido (um target que não é ${{VAR}} nem uma URL http(s), um pathRewrite que faz mais que tirar o endpoint, ignorePath, prependPath, ou arquivos app-config que divergem), então {calls} chamada(s) por ele ficam sem ligação"),
                format!("endpoint de proxy de Backstage {endpoint}: su target o la ruta que recibe no se lee (un target que no es ${{VAR}} ni una URL http(s), un pathRewrite que hace más que quitar el endpoint, ignorePath, prependPath, o archivos app-config que difieren), así que {calls} llamada(s) por él quedan sin enlace"),
            ),
            Msg::BackstageInclude {
                file,
                include,
                reason,
            } => {
                use crate::http::IncludeReason as R;
                let (depth, count) = (crate::http::INCLUDE_DEPTH, crate::http::INCLUDES);
                let mib = crate::lang::MAX_BYTES >> 20;
                let words = |(en, pt, es): (&str, &str, &str)| (en.into(), pt.into(), es.into());
                let (en, pt, es): (String, String, String) = match reason {
                    R::Absolute => words(("an absolute path", "caminho absoluto", "ruta absoluta")),
                    R::Outside => words((
                        "a path that leaves the repository",
                        "caminho que sai do repositório",
                        "ruta que sale del repositorio",
                    )),
                    R::Variable => words(("a ${ in the path", "${ no caminho", "${ en la ruta")),
                    R::Extension => words((
                        "not a .json, .yaml or .yml file",
                        "não é arquivo .json, .yaml ou .yml",
                        "no es archivo .json, .yaml o .yml",
                    )),
                    R::Symlink => words(("a symbolic link", "link simbólico", "enlace simbólico")),
                    R::Missing => words((
                        "no such file, or one ignored or hidden",
                        "arquivo inexistente, ignorado ou oculto",
                        "archivo inexistente, ignorado u oculto",
                    )),
                    R::Unreadable => words((
                        "a file that does not parse",
                        "arquivo que não se lê",
                        "archivo que no se lee",
                    )),
                    R::KeyPath => words((
                        "a key path the file does not hold",
                        "caminho de chave que o arquivo não tem",
                        "ruta de clave que el archivo no tiene",
                    )),
                    R::Sibling => words((
                        "another key beside $include",
                        "outra chave ao lado de $include",
                        "otra clave junto a $include",
                    )),
                    R::NotPath => words((
                        "a value that is not a path",
                        "valor que não é caminho",
                        "valor que no es ruta",
                    )),
                    R::Cycle => words(("a cycle", "ciclo", "ciclo")),
                    R::Depth => (
                        format!("deeper than {depth} includes"),
                        format!("mais fundo que {depth} includes"),
                        format!("más hondo que {depth} includes"),
                    ),
                    R::Count => (
                        format!("more than {count} includes from one root"),
                        format!("mais de {count} includes a partir de uma raiz"),
                        format!("más de {count} includes desde una raíz"),
                    ),
                    R::Size => (
                        format!("past {mib} MiB of included files"),
                        format!("mais de {mib} MiB de arquivos incluídos"),
                        format!("más de {mib} MiB de archivos incluidos"),
                    ),
                };
                pick(
                    format!("{file}: Backstage $include {include} not followed ({en}): the keys under it stay unread"),
                    format!("{file}: $include do Backstage {include} não seguido ({pt}): as chaves sob ele ficam sem leitura"),
                    format!("{file}: $include de Backstage {include} no seguido ({es}): las claves bajo él quedan sin leer"),
                )
            }
            Msg::DocumentUnreadable { path, line, error } => pick(
                format!("{path}:{line}: unreadable document ({error}), ignored"),
                format!("{path}:{line}: documento ilegível ({error}), ignorado"),
                format!("{path}:{line}: documento ilegible ({error}), ignorado"),
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
                        format!("{path}: {name} {version}, canales: {count}"),
                    ),
                    Format::Openapi => pick(
                        format!("{path}: {name} {version}, routes: {count}"),
                        format!("{path}: {name} {version}, rotas: {count}"),
                        format!("{path}: {name} {version}, rutas: {count}"),
                    ),
                }
            }
            Msg::DocumentMissing { path } => pick(
                format!("{path}: listed in `serve.documents`, not found"),
                format!("{path}: listado em `serve.documents`, não encontrado"),
                format!("{path}: listado en `serve.documents`, no encontrado"),
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
                format!(
                    "{path}: no es OpenAPI 3.0 ni 3.1 ({}), ignorado",
                    found.unwrap_or("sin el campo `openapi`")
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
                format!(
                    "{path}: no es AsyncAPI 2.x ni 3.0 ({}), ignorado",
                    found.unwrap_or("sin el campo `asyncapi`")
                ),
            ),
            Msg::DocumentProtocol { path, protocol } => pick(
                format!("{path}: server protocol `{protocol}` has no broker kind, its channels are ignored"),
                format!("{path}: o protocolo `{protocol}` do servidor não tem tipo de broker, seus canais são ignorados"),
                format!("{path}: el protocolo `{protocol}` del servidor no tiene tipo de broker, sus canales se ignoran"),
            ),
            Msg::DocumentParameters { path, channel } => pick(
                format!("{path}: channel `{channel}` has parameters, ignored"),
                format!("{path}: o canal `{channel}` tem parâmetros, ignorado"),
                format!("{path}: el canal `{channel}` tiene parámetros, ignorado"),
            ),
            Msg::DocumentNoServers { path } => pick(
                format!("{path}: no root `servers`, so no channel has a broker kind: its channels are ignored"),
                format!("{path}: sem `servers` na raiz, nenhum canal tem tipo de broker: seus canais são ignorados"),
                format!("{path}: sin `servers` en la raíz, ningún canal tiene tipo de broker: sus canales se ignoran"),
            ),
            Msg::DocumentNoProtocol { path, server } => pick(
                format!("{path}: server `{server}` has no `protocol`, its channels are ignored"),
                format!("{path}: o servidor `{server}` não tem `protocol`, seus canais são ignorados"),
                format!("{path}: el servidor `{server}` no tiene `protocol`, sus canales se ignoran"),
            ),
            Msg::DocumentUnknownServer {
                path,
                channel,
                server,
            } => pick(
                format!("{path}: channel `{channel}` names server `{server}`, which the document does not have, ignored there"),
                format!("{path}: o canal `{channel}` cita o servidor `{server}`, que o documento não tem, ignorado ali"),
                format!("{path}: el canal `{channel}` cita el servidor `{server}`, que el documento no tiene, ignorado allí"),
            ),
            Msg::DuplicateRoute { route, path, first } => pick(
                format!("{route} in {path} is also in {first}: one route"),
                format!("{route} em {path} também está em {first}: uma rota só"),
                format!("{route} en {path} también está en {first}: una sola ruta"),
            ),
            Msg::ExternalRef { path, reference } => pick(
                format!("{path}: external reference {reference} not followed, its part of the shape is open"),
                format!("{path}: referência externa {reference} não seguida, sua parte do shape fica aberta"),
                format!("{path}: referencia externa {reference} no seguida, su parte del shape queda abierta"),
            ),
            Msg::ExternalPathItem { path, reference } => pick(
                format!("{path}: external path item reference {reference} not followed, its routes are not read"),
                format!("{path}: referência externa de path item {reference} não seguida, suas rotas não são lidas"),
                format!("{path}: referencia externa de path item {reference} no seguida, sus rutas no se leen"),
            ),
            Msg::KeyMissing(key) => Core::KeyMissing(key).text(),
            Msg::Empty => Core::Empty.text(),
            Msg::NeedsFeature { section, feature } => pick(
                format!("[{section}] needs a Meada built with the {feature} feature"),
                format!("[{section}] exige um Meada compilado com a feature {feature}"),
                format!("[{section}] requiere un Meada compilado con la feature {feature}"),
            ),
            #[cfg(feature = "valkey")]
            Msg::ValkeyUrlInvalid => pick(
                "is not a redis:// or rediss:// URL".into(),
                "não é uma URL redis:// ou rediss://".into(),
                "no es una URL redis:// o rediss://".into(),
            ),
            #[cfg(feature = "valkey")]
            Msg::ValkeyUrlPassword => pick(
                "carries a password: name it in password_env or password_file".into(),
                "traz uma senha: nomeie-a em password_env ou password_file".into(),
                "contiene una contraseña: nómbrala en password_env o password_file".into(),
            ),
            #[cfg(feature = "valkey")]
            Msg::ValkeyUrlPlain => pick(
                "must be a rediss:// URL: TLS is required unless the host is loopback".into(),
                "deve ser uma URL rediss://: TLS é obrigatório, salvo em loopback".into(),
                "debe ser una URL rediss://: TLS es obligatorio, salvo en loopback".into(),
            ),
            #[cfg(feature = "valkey")]
            Msg::ValkeyUrlInsecure => pick(
                "must verify the server certificate: no #insecure".into(),
                "deve verificar o certificado do servidor: sem #insecure".into(),
                "debe verificar el certificado del servidor: sin #insecure".into(),
            ),
            #[cfg(feature = "valkey")]
            Msg::ValkeyClusterPlain => pick(
                "needs a rediss:// url: the nodes a cluster announces are reached over TLS".into(),
                "exige uma url rediss://: os nós que um cluster anuncia são alcançados por TLS".into(),
                "requiere una url rediss://: los nodos que un cluster anuncia se alcanzan por TLS".into(),
            ),
            #[cfg(feature = "valkey")]
            Msg::ValkeyBoth => pick(
                "is set together with password_env: use one of the two".into(),
                "está junto de password_env: use um dos dois".into(),
                "está junto con password_env: usa uno de los dos".into(),
            ),
            #[cfg(feature = "valkey")]
            Msg::ValkeyPrefixBraces => pick(
                "must not contain `{` or `}`".into(),
                "não pode conter `{` nem `}`".into(),
                "no puede contener `{` ni `}`".into(),
            ),
            #[cfg(feature = "valkey")]
            Msg::ValkeyEnvUnset(var) => pick(
                format!("the variable {var} is not set"),
                format!("a variável {var} não está definida"),
                format!("la variable {var} no está definida"),
            ),
            #[cfg(feature = "valkey")]
            Msg::ValkeyTimeout(ms) => pick(
                format!("no answer within {ms} ms"),
                format!("sem resposta em {ms} ms"),
                format!("sin respuesta en {ms} ms"),
            ),
            #[cfg(feature = "valkey")]
            Msg::ValkeyReachable => pick(
                "valkey: reachable".into(),
                "valkey: alcançável".into(),
                "valkey: alcanzable".into(),
            ),
            #[cfg(feature = "valkey")]
            Msg::ValkeyUnreachable(reason) => pick(
                format!("valkey: unreachable ({reason})"),
                format!("valkey: inalcançável ({reason})"),
                format!("valkey: inalcanzable ({reason})"),
            ),
            #[cfg(feature = "postgres")]
            Msg::AtMost(max) => pick(
                format!("must be at most {max}"),
                format!("deve ser no máximo {max}"),
                format!("debe ser como máximo {max}"),
            ),
            #[cfg(feature = "postgres")]
            Msg::PostgresBoth => pick(
                "is set together with url_env: use one of the two".into(),
                "está junto de url_env: use um dos dois".into(),
                "está junto con url_env: usa uno de los dos".into(),
            ),
            #[cfg(feature = "postgres")]
            Msg::PostgresSchema => pick(
                "must be 1 to 63 lowercase letters, digits or `_`, starting with a letter, not with `pg_` (reserved by Postgres)".into(),
                "deve ter de 1 a 63 letras minúsculas, dígitos ou `_`, começando por uma letra, sem começar por `pg_` (reservado pelo Postgres)"
                    .into(),
                "debe tener de 1 a 63 letras minúsculas, dígitos o `_`, empezando por una letra, sin empezar por `pg_` (reservado por Postgres)"
                    .into(),
            ),
            #[cfg(feature = "postgres")]
            Msg::PostgresUrlInvalid => pick(
                "does not hold a postgres:// or postgresql:// URL with a TCP host".into(),
                "não traz uma URL postgres:// ou postgresql:// com um host TCP".into(),
                "no contiene una URL postgres:// o postgresql:// con un host TCP".into(),
            ),
            #[cfg(feature = "postgres")]
            Msg::PostgresUrlQuery => pick(
                "holds a URL with a query: no parameter is accepted, TLS is decided by the host"
                    .into(),
                "traz uma URL com query: nenhum parâmetro é aceito, o TLS é decidido pelo host"
                    .into(),
                "contiene una URL con query: no se acepta ningún parámetro, el TLS lo decide el host"
                    .into(),
            ),
            #[cfg(feature = "postgres")]
            Msg::PostgresUrlUnset(var) => pick(
                format!("the variable {var} is not set"),
                format!("a variável {var} não está definida"),
                format!("la variable {var} no está definida"),
            ),
            #[cfg(feature = "postgres")]
            Msg::PostgresTimeout(ms) => pick(
                format!("no answer within {ms} ms"),
                format!("sem resposta em {ms} ms"),
                format!("sin respuesta en {ms} ms"),
            ),
            #[cfg(feature = "postgres")]
            Msg::PostgresVersionLower {
                schema,
                stored,
                known,
            } => pick(
                format!(
                    "the schema {schema} is at version {stored}, below this Meada's {known}: run \
                     meada postgres migrate"
                ),
                format!(
                    "o schema {schema} está na versão {stored}, abaixo da {known} deste Meada: \
                     rode meada postgres migrate"
                ),
                format!(
                    "el schema {schema} está en la versión {stored}, por debajo de la {known} de \
                     este Meada: ejecuta meada postgres migrate"
                ),
            ),
            #[cfg(feature = "postgres")]
            Msg::PostgresVersionHigher {
                schema,
                stored,
                known,
            } => pick(
                format!(
                    "the schema {schema} is at version {stored}, above this Meada's {known}: \
                     upgrade Meada"
                ),
                format!(
                    "o schema {schema} está na versão {stored}, acima da {known} deste Meada: \
                     atualize o Meada"
                ),
                format!(
                    "el schema {schema} está en la versión {stored}, por encima de la {known} de \
                     este Meada: actualiza Meada"
                ),
            ),
            #[cfg(feature = "postgres")]
            Msg::PostgresReachable { schema, version } => pick(
                format!("postgres: reachable, schema {schema} version {version}"),
                format!("postgres: alcançável, schema {schema} versão {version}"),
                format!("postgres: alcanzable, schema {schema} versión {version}"),
            ),
            #[cfg(any(feature = "postgres", feature = "ui"))]
            Msg::PostgresUnreachable(reason) => pick(
                format!("postgres: unreachable ({reason})"),
                format!("postgres: inalcançável ({reason})"),
                format!("postgres: inalcanzable ({reason})"),
            ),
            #[cfg(feature = "postgres")]
            Msg::PostgresMissing(manifest) => pick(
                format!("{manifest}: no [postgres] section: meada postgres migrate needs one"),
                format!("{manifest}: sem seção [postgres]: meada postgres migrate precisa de uma"),
                format!("{manifest}: sin sección [postgres]: meada postgres migrate necesita una"),
            ),
            #[cfg(feature = "postgres")]
            Msg::Postgres => pick(
                "The data layer in Postgres (the [postgres] section).".into(),
                "A camada de dados no Postgres (a seção [postgres]).".into(),
                "La capa de datos en Postgres (la sección [postgres]).".into(),
            ),
            #[cfg(feature = "postgres")]
            Msg::PostgresMigrate => pick(
                "Create the schema [postgres] names, or bring it to this Meada's version.".into(),
                "Cria o schema que [postgres] nomeia, ou o leva à versão deste Meada.".into(),
                "Crea el schema que [postgres] nombra, o lo lleva a la versión de este Meada.".into(),
            ),
            #[cfg(feature = "postgres")]
            Msg::PostgresWorkspaceHelp => pick(
                "The bootstrap manifest, with its [postgres] section".into(),
                "O manifesto de bootstrap, com a seção [postgres]".into(),
                "El manifiesto de bootstrap, con su sección [postgres]".into(),
            ),
            #[cfg(feature = "directory")]
            Msg::DirectoryBoth => pick(
                "is set together with token_env: use one of the two".into(),
                "está junto de token_env: use um dos dois".into(),
                "está junto con token_env: usa uno de los dos".into(),
            ),
            #[cfg(feature = "directory")]
            Msg::DirectoryTokenShort(min) => pick(
                format!("holds a token shorter than {min} characters"),
                format!("traz um token com menos de {min} caracteres"),
                format!("contiene un token de menos de {min} caracteres"),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotEndpointInvalid => pick(
                "is not an http:// or https:// URL with a host and nothing after it".into(),
                "não é uma URL http:// ou https:// com um host e nada depois dele".into(),
                "no es una URL http:// o https:// con un host y nada después de él".into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotEndpointUser => pick(
                "carries a user or a password: credentials come from the credential keys".into(),
                "traz um usuário ou uma senha: as credenciais vêm das chaves de credencial".into(),
                "contiene un usuario o una contraseña: las credenciales vienen de las claves de credencial".into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotEndpointPlain => pick(
                "must be an https:// URL: TLS is required unless the host is loopback".into(),
                "deve ser uma URL https://: TLS é obrigatório, salvo em loopback".into(),
                "debe ser una URL https://: TLS es obligatorio, salvo en loopback".into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotPathStyle => pick(
                "must be true for an http://, IP or localhost endpoint: the bucket cannot go in \
                 front of that host"
                    .into(),
                "deve ser true para um endpoint http://, IP ou localhost: o bucket não pode ir à \
                 frente desse host"
                    .into(),
                "debe ser true para un endpoint http://, IP o localhost: el bucket no puede ir \
                 delante de ese host"
                    .into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotRegion => pick(
                "must be lowercase letters, digits and `-`".into(),
                "deve ter só letras minúsculas, dígitos e `-`".into(),
                "debe tener solo letras minúsculas, dígitos y `-`".into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotBucket => pick(
                "must be 3 to 63 lowercase letters, digits, `.` and `-`, starting and ending with \
                 a letter or a digit"
                    .into(),
                "deve ter de 3 a 63 letras minúsculas, dígitos, `.` e `-`, começando e \
                 terminando com letra ou dígito"
                    .into(),
                "debe tener de 3 a 63 letras minúsculas, dígitos, `.` y `-`, empezando y \
                 terminando con letra o dígito"
                    .into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotNodeName => pick(
                "must be 1 to 63 lowercase letters, digits, `.` and `-`".into(),
                "deve ter de 1 a 63 letras minúsculas, dígitos, `.` e `-`".into(),
                "debe tener de 1 a 63 letras minúsculas, dígitos, `.` y `-`".into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotPrefix => pick(
                "must be `/`-separated segments, none of them empty, `.` or `..`".into(),
                "deve ter segmentos separados por `/`, nenhum vazio, `.` ou `..`".into(),
                "debe tener segmentos separados por `/`, ninguno vacío, `.` o `..`".into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotBoth(other) => pick(
                format!("is set together with {other}: use one of the two"),
                format!("está junto de {other}: use um dos dois"),
                format!("está junto con {other}: usa uno de los dos"),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotStaticOnly => pick(
                "is read only with credentials = \"static\"".into(),
                "só é lida com credentials = \"static\"".into(),
                "solo se lee con credentials = \"static\"".into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotEnvUnset(var) => pick(
                format!("the variable {var} is not set"),
                format!("a variável {var} não está definida"),
                format!("la variable {var} no está definida"),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotNoSection(manifest) => pick(
                format!("{manifest} has no [snapshot] section"),
                format!("{manifest} não tem a seção [snapshot]"),
                format!("{manifest} no tiene la sección [snapshot]"),
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
                format!(
                    "{repo}: su índice difiere del espejo; ejecuta meada index --workspace \
                     {manifest}"
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
                format!(
                    "{repo}: el espejo {mirror} no está limpio: git status lista archivos \
                     modificados, sin seguimiento o ignorados"
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
                "AWS_CONTAINER_CREDENTIALS_FULL_URI debe apuntar al agente del Pod Identity, \
                 http://169.254.170.23 o http://[fd00:ec2::23]"
                    .into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotRefused(origin) => pick(
                format!("refused a request to {origin}: not a configured endpoint"),
                format!("recusada uma requisição a {origin}: não é um endpoint configurado"),
                format!("rechazada una solicitud a {origin}: no es un endpoint configurado"),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotObjectBad(key) => pick(
                format!("{key}: not stored with the size and hash it records"),
                format!("{key}: não está guardado com o tamanho e o hash que registra"),
                format!("{key}: no está guardado con el tamaño y el hash que registra"),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotChanged(repo) => pick(
                format!("{repo}: the mirror or its index changed while it was published"),
                format!("{repo}: o espelho ou seu índice mudou durante a publicação"),
                format!("{repo}: el espejo o su índice cambió durante la publicación"),
            ),
            #[cfg(feature = "snapshot")]
            Msg::Snapshot => pick(
                "Server mode: the snapshot store of generations (ADR 0012).".into(),
                "Modo servidor: o armazenamento de gerações (ADR 0012).".into(),
                "Modo servidor: el almacenamiento de generaciones (ADR 0012).".into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotPublish => pick(
                "Upload the workspace as a generation, once every index equals its mirror."
                    .into(),
                "Envia o workspace como uma geração, quando cada índice equivale ao seu espelho."
                    .into(),
                "Envía el workspace como una generación, cuando cada índice equivale a su espejo."
                    .into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotPull => pick(
                "Fetch the store's current generation into local_dir, verify it and make it the \
                 node's current one."
                    .into(),
                "Baixa a geração atual do store em local_dir, verifica-a e a torna a atual do nó."
                    .into(),
                "Descarga la generación actual del store en local_dir, la verifica y la convierte \
                 en la actual del nodo."
                    .into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotEvery => pick(
                "Pull again every this many seconds.".into(),
                "Repete o pull a cada tantos segundos.".into(),
                "Repite el pull cada tantos segundos.".into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotNoLocalDir(manifest) => pick(
                format!("{manifest}: `snapshot.local_dir` is not set: the pull writes there"),
                format!("{manifest}: `snapshot.local_dir` não está definido: o pull grava lá"),
                format!("{manifest}: `snapshot.local_dir` no está definido: el pull escribe allí"),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotPulling(dir) => pick(
                format!("a pull is running in {dir}"),
                format!("um pull está em andamento em {dir}"),
                format!("un pull está en curso en {dir}"),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotNoCurrent(key) => pick(
                format!("{key}: not found: no generation is published there"),
                format!("{key}: não encontrado: nenhuma geração foi publicada lá"),
                format!("{key}: no encontrado: ninguna generación fue publicada allí"),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotBadId(key) => pick(
                format!("{key}: does not hold an id of 16 lowercase hex characters"),
                format!("{key}: não contém um id de 16 caracteres hexadecimais minúsculos"),
                format!("{key}: no contiene un id de 16 caracteres hexadecimales minúsculos"),
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
                format!(
                    "la clave de bundle `{key}` no es repos/<identidad>/<id>.tar bajo el prefijo, \
                     con un id de 16 caracteres hexadecimales minúsculos"
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
                format!(
                    "{path}: no es `<id de la generación> <fence>`; bórralo para recibir la \
                     generación actual del store"
                ),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotNotDocument(key) => pick(
                format!("{key}: not the generation document of its id"),
                format!("{key}: não é o documento de geração do seu id"),
                format!("{key}: no es el documento de generación de su id"),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotBadWorkspace { key, version } => pick(
                format!(
                    "{key}: `workspace` {version}: not a workspace version id of 16 lowercase \
                     hex characters"
                ),
                format!(
                    "{key}: `workspace` {version}: não é um id de versão do workspace de 16 \
                     caracteres hexadecimais minúsculos"
                ),
                format!(
                    "{key}: `workspace` {version}: no es un id de versión del workspace de 16 \
                     caracteres hexadecimales minúsculos"
                ),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotIdentityChars => pick(
                "not a repository identity: it holds whitespace, a quote, a backslash or a \
                 control character"
                    .into(),
                "não é uma identidade de repositório: contém espaço, aspas, barra invertida ou \
                 caractere de controle"
                    .into(),
                "no es una identidad de repositorio: contiene espacio, comillas, barra \
                 invertida o carácter de control"
                    .into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotBytesCeiling(max) => pick(
                format!("past the ceiling of {max} bytes per generation"),
                format!("acima do teto de {max} bytes por geração"),
                format!("por encima del tope de {max} bytes por generación"),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotEntriesCeiling(max) => pick(
                format!("past the ceiling of {max} archive entries per generation"),
                format!("acima do teto de {max} entradas de arquivo por geração"),
                format!("por encima del tope de {max} entradas de archivo por generación"),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotEntryHeader(max) => pick(
                format!("a long name or a pax header past the ceiling of {max} bytes"),
                format!("um nome longo ou um cabeçalho pax acima do teto de {max} bytes"),
                format!("un nombre largo o un encabezado pax por encima del tope de {max} bytes"),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotEntryNested => pick(
                "a nested repository: a `.git` below the mirror's own, or under its `.git/modules`"
                    .into(),
                "um repositório aninhado: um `.git` abaixo do próprio do espelho, ou sob o seu \
                 `.git/modules`"
                    .into(),
                "un repositorio anidado: un `.git` por debajo del propio del espejo, o bajo su \
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
                "una ruta absoluta, un componente `.` o `..`, o una ruta que no es index.db \
                 primero y el espejo bajo mirror/"
                    .into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotEntryLink => pick(
                "a link whose target leaves the repository directory".into(),
                "um link cujo alvo sai do diretório do repositório".into(),
                "un enlace cuyo destino sale del directorio del repositorio".into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotEntryThrough => pick(
                "written through a symbolic link or over an earlier entry".into(),
                "gravada através de um link simbólico ou sobre uma entrada anterior".into(),
                "escrita a través de un enlace simbólico o sobre una entrada anterior".into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotEntrySpecial => pick(
                "a device, a FIFO or another special file".into(),
                "um dispositivo, um FIFO ou outro arquivo especial".into(),
                "un dispositivo, un FIFO u otro archivo especial".into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotNoGit => pick(
                "the mirror has no .git directory".into(),
                "o espelho não tem um diretório .git".into(),
                "el espejo no tiene un directorio .git".into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotHead { found, head } => pick(
                format!("the mirror's HEAD is {found}, not the document's {head}"),
                format!("o HEAD do espelho é {found}, não o {head} do documento"),
                format!("el HEAD del espejo es {found}, no el {head} del documento"),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotNoneHeld(dir) => pick(
                format!("{dir} holds no generation: run meada snapshot pull"),
                format!("{dir} não guarda nenhuma geração: rode meada snapshot pull"),
                format!("{dir} no guarda ninguna generación: ejecuta meada snapshot pull"),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotTimeout(ms) => pick(
                format!("no answer within {ms} ms"),
                format!("sem resposta em {ms} ms"),
                format!("sin respuesta en {ms} ms"),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotStatusCurrent(id) => pick(
                format!("current: {}", id.unwrap_or("none")),
                format!("atual: {}", id.unwrap_or("nenhuma")),
                format!("actual: {}", id.unwrap_or("ninguna")),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotStatusHeld(ids) => match ids.is_empty() {
                true => pick(
                    "held: none".into(),
                    "retidas: nenhuma".into(),
                    "retenidas: ninguna".into(),
                ),
                false => pick(
                    format!("held: {}", ids.join(" ")),
                    format!("retidas: {}", ids.join(" ")),
                    format!("retenidas: {}", ids.join(" ")),
                ),
            },
            #[cfg(feature = "snapshot")]
            Msg::SnapshotStoreReachable(id) => pick(
                format!("store: reachable (current {id})"),
                format!("store: alcançável (atual {id})"),
                format!("store: alcanzable (actual {id})"),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotStoreUnreachable(reason) => pick(
                format!("store: unreachable ({reason})"),
                format!("store: inalcançável ({reason})"),
                format!("store: inalcanzable ({reason})"),
            ),
            #[cfg(all(feature = "workspaces", feature = "accounts"))]
            Msg::NeedsGithubProvider(section) => pick(
                format!("[{section}] needs [oidc.github] or [accounts.github]"),
                format!("[{section}] exige [oidc.github] ou [accounts.github]"),
                format!("[{section}] requiere [oidc.github] o [accounts.github]"),
            ),
            #[cfg(any(
                feature = "generation-pointer",
                feature = "github-app",
                feature = "directory",
                all(feature = "oidc", feature = "postgres")
            ))]
            Msg::NeedsSections { section, needed } => {
                let needed: Vec<String> = needed.iter().map(|s| format!("[{s}]")).collect();
                pick(
                    format!("[{section}] needs {}", needed.join(" and ")),
                    format!("[{section}] exige {}", needed.join(" e ")),
                    format!("[{section}] requiere {}", needed.join(" y ")),
                )
            }
            #[cfg(any(
                feature = "generation-pointer",
                feature = "oidc",
                feature = "github-app",
                feature = "postgres"
            ))]
            Msg::AtLeast(min) => pick(
                format!("must be at least {min}"),
                format!("deve ser pelo menos {min}"),
                format!("debe ser al menos {min}"),
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
                format!(
                    "{key}: no es un hash de `id`, un id de generación de 16 caracteres \
                     hexadecimales en minúsculas, y `fence`, un fence decimal"
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
                format!(
                    "{key}: una entrada no es `<id de la generación>:<fence>`, un id de 16 \
                     caracteres hexadecimales en minúsculas y un fence decimal"
                ),
            ),
            #[cfg(feature = "generation-pointer")]
            Msg::GenerationsEmpty(key) => pick(
                format!("{key}: empty: no generation is retained there"),
                format!("{key}: vazia: nenhuma geração está retida lá"),
                format!("{key}: vacía: ninguna generación está retenida allí"),
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
                format!(
                    "{key}: tiene más de {max} entradas, `keep` más las {kept} generaciones \
                     mantenidas permitidas",
                    kept = crate::snapshot::generations::KEPT_MAX
                ),
            ),
            #[cfg(feature = "snapshot")]
            Msg::SnapshotRetainedSkipped { id, why } => pick(
                format!("retained generation {id} not pulled, the next pull retries: {why}"),
                format!("geração retida {id} não baixada, o próximo pull tenta de novo: {why}"),
                format!("generación retenida {id} no descargada, el próximo pull reintenta: {why}"),
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
                format!(
                    "{current}: fence {fence} rechazado, no mayor que el que guarda: \
                     {fence_key} fue rebajado o perdido; elévalo con meada snapshot fence \
                     recover (--at-least para un fence leído en el <local_dir>/current de un \
                     nodo), o elimina el <local_dir>/current de un nodo para que tome la \
                     generación actual sea cual sea su fence"
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
                format!(
                    "{current}: la generación {id} tiene fence {fence}, por debajo del fence \
                     {held} en {pointer}: no intercambiada; eleva {fence_key} con meada snapshot \
                     fence recover --at-least {held} y publica de nuevo, o elimina {pointer} (con \
                     [refresh_lock] desactivado todo fence es 0, y solo eliminarlo lo resuelve)"
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
                format!(
                    "fence: atascado: {held} en {pointer} está por encima de {fence} en \
                     {current}; eleva {fence_key} con meada snapshot fence recover --at-least \
                     {held} y publica de nuevo, o elimina {pointer}"
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
                format!(
                    "{key}: el lock de refresh ya no guarda el token de este job: nada más \
                     enviado y ningún puntero escrito"
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
                format!(
                    "{key}: en posesión del host {host}, proceso {pid}, desde {started}, durante \
                     {holds}: nada ejecutado"
                ),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::LockHeld { key, holder: None } => pick(
                format!("{key}: held by an unknown holder: nothing run"),
                format!("{key}: em posse de um detentor desconhecido: nada executado"),
                format!("{key}: en posesión de un titular desconocido: nada ejecutado"),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::PointerNotRetained { id, key } => pick(
                format!("{key}: generation {id} is not retained: the pointer did not move"),
                format!("{key}: a geração {id} não está retida: o ponteiro não se moveu"),
                format!("{key}: la generación {id} no está retenida: el puntero no se movió"),
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
                format!(
                    "{key}: ya mantiene {max} generaciones, el máximo permitido: elimina una \
                     antes (meada snapshot pointer drop)"
                ),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::DropCurrent { id, key } => pick(
                format!("{key}: generation {id} is the current one: nothing dropped"),
                format!("{key}: a geração {id} é a atual: nada removido"),
                format!("{key}: la generación {id} es la actual: nada eliminado"),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::DropNotRetained { id, key } => pick(
                format!("{key}: generation {id} is not retained: nothing dropped"),
                format!("{key}: a geração {id} não está retida: nada removido"),
                format!("{key}: la generación {id} no está retenida: nada eliminado"),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::BadGenerationArg(text) => pick(
                format!("{text}: not a generation id of 16 lowercase hex characters"),
                format!("{text}: não é um id de geração de 16 caracteres hexadecimais minúsculos"),
                format!("{text}: no es un id de generación de 16 caracteres hexadecimales en minúsculas"),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::BadFenceArg(text) => pick(
                format!("--at-least {text}: not a decimal fence"),
                format!("--at-least {text}: não é um fence decimal"),
                format!("--at-least {text}: no es un fence decimal"),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::FenceLimit { key, max } => pick(
                format!("{key}: operation would exhaust the fence counter; the maximum usable fence is {max} (Valkey INCR limit)"),
                format!("{key}: a operação esgotaria o contador de fences; o maior fence utilizável é {max} (limite de INCR do Valkey)"),
                format!("{key}: la operación agotaría el contador de fences; el mayor fence utilizable es {max} (límite de INCR de Valkey)"),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::FenceNotCounted(problem) => pick(
                format!("{problem}: not counted"),
                format!("{problem}: não contado"),
                format!("{problem}: no contado"),
            ),
            #[cfg(feature = "generation-pointer")]
            Msg::HeartbeatBad(key) => pick(
                format!("{key}: not a heartbeat, a JSON object whose fence is a decimal or null"),
                format!(
                    "{key}: não é um heartbeat, um objeto JSON cujo fence é um decimal ou null"
                ),
                format!(
                    "{key}: no es un heartbeat, un objeto JSON cuyo fence es un decimal o null"
                ),
            ),
            #[cfg(feature = "generation-pointer")]
            Msg::NodeNameHost(host) => pick(
                format!(
                    "the host name \"{host}\" is not a node name (lowercase letters, digits, . and \
                     -, 1 to 63 characters): set [snapshot] node_name"
                ),
                format!(
                    "o nome do host \"{host}\" não é um nome de nó (letras minúsculas, dígitos, . e \
                     -, de 1 a 63 caracteres): defina [snapshot] node_name"
                ),
                format!(
                    "el nombre del host \"{host}\" no es un nombre de nodo (letras minúsculas, \
                     dígitos, . y -, de 1 a 63 caracteres): define [snapshot] node_name"
                ),
            ),
            #[cfg(feature = "generation-pointer")]
            Msg::HeartbeatNotWritten { key, reason } => pick(
                format!("heartbeat not written to {key}: {reason}"),
                format!("heartbeat não gravado em {key}: {reason}"),
                format!("heartbeat no escrito en {key}: {reason}"),
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
                format!(
                    "{key}: \"{name}\" no es un nombre de nodo (letras minúsculas, dígitos, . y \
                     -, de 1 a 63 caracteres)"
                ),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::SnapshotPointer => pick(
                "Move the workspace's generation pointer under the refresh lock.".into(),
                "Move o ponteiro de geração do workspace sob o lock de refresh.".into(),
                "Mueve el puntero de generación del workspace bajo el lock de refresh.".into(),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::SnapshotPointerSet => pick(
                "Point the fleet at a retained generation: a rollback, or forward again.".into(),
                "Aponta a frota para uma geração retida: um rollback, ou adiante de novo.".into(),
                "Apunta la flota a una generación retenida: un rollback, o hacia adelante de \
                 nuevo."
                    .into(),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::SnapshotPointerGeneration => pick(
                "The generation id, one of the retained set.".into(),
                "O id da geração, um dos retidos.".into(),
                "El id de la generación, uno de los retenidos.".into(),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::SnapshotPointerRetain => pick(
                "Keep a generation on every node, outside the keep trim, until it is dropped."
                    .into(),
                "Mantém uma geração em todo nó, fora do corte de keep, até ser removida.".into(),
                "Mantiene una generación en cada nodo, fuera del recorte de keep, hasta que se \
                 elimine."
                    .into(),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::SnapshotPointerRetainGeneration => pick(
                "The generation id, one whose document the store holds.".into(),
                "O id da geração, uma cujo documento o store guarda.".into(),
                "El id de la generación, una cuyo documento guarda el store.".into(),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::SnapshotPointerDrop => pick(
                "Stop retaining a generation that is not current; the next pull deletes it."
                    .into(),
                "Deixa de reter uma geração que não é a atual; o próximo pull a apaga.".into(),
                "Deja de retener una generación que no es la actual; el próximo pull la elimina."
                    .into(),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::SnapshotFence => pick(
                "Recover the refresh lock's fence under the lock.".into(),
                "Recupera o fence do lock de refresh sob o lock.".into(),
                "Recupera el fence del lock de refresh bajo el lock.".into(),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::SnapshotFenceRecover => pick(
                "Raise the fence to the highest one recorded in Valkey, never lowering it."
                    .into(),
                "Eleva o fence ao maior registrado no Valkey, sem nunca baixá-lo.".into(),
                "Eleva el fence al mayor registrado en Valkey, sin bajarlo nunca.".into(),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::SnapshotFenceAtLeast => pick(
                "A floor for the fence, such as one read in a node's <local_dir>/current."
                    .into(),
                "Um piso para o fence, como um lido no <local_dir>/current de um nó.".into(),
                "Un mínimo para el fence, como uno leído en el <local_dir>/current de un nodo."
                    .into(),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::LockLost(key) => pick(
                format!("{key}: the refresh lock was lost: the command was terminated"),
                format!("{key}: o lock de refresh foi perdido: o comando foi encerrado"),
                format!("{key}: el lock de refresh se perdió: el comando fue finalizado"),
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
                format!(
                    "{key}: no es un fence decimal positivo: defínelo igual o por encima del \
                     mayor fence registrado"
                ),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::LockNoSection(manifest) => pick(
                format!("{manifest}: no [refresh_lock] section: the lock has no TTL"),
                format!("{manifest}: sem seção [refresh_lock]: o lock não tem TTL"),
                format!("{manifest}: sin sección [refresh_lock]: el lock no tiene TTL"),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::LockEnv => pick(
                "[refresh_lock] needs MEADA_REFRESH_LOCK and MEADA_REFRESH_FENCE: run publish \
                 under meada snapshot lock"
                    .into(),
                "[refresh_lock] exige MEADA_REFRESH_LOCK e MEADA_REFRESH_FENCE: rode o publish \
                 sob meada snapshot lock"
                    .into(),
                "[refresh_lock] requiere MEADA_REFRESH_LOCK y MEADA_REFRESH_FENCE: ejecuta \
                 publish bajo meada snapshot lock"
                    .into(),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::SnapshotLock => pick(
                "Take the workspace's refresh lock, run the command as its child and release \
                 the lock."
                    .into(),
                "Toma o lock de refresh do workspace, roda o comando como filho e libera o lock."
                    .into(),
                "Toma el lock de refresh del workspace, ejecuta el comando como hijo y libera \
                 el lock."
                    .into(),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::SnapshotLockCommand => pick(
                "The refresh to run: fetch, meada index --workspace, meada snapshot publish."
                    .into(),
                "O refresh a rodar: fetch, meada index --workspace, meada snapshot publish."
                    .into(),
                "El refresh a ejecutar: fetch, meada index --workspace, meada snapshot publish."
                    .into(),
            ),
            #[cfg(feature = "snapshot")]
            Msg::GenerationNoContext(id) => pick(
                format!("generation {id} records no such workspace"),
                format!("a geração {id} não registra tal workspace"),
                format!("la generación {id} no registra tal workspace"),
            ),
            #[cfg(feature = "snapshot")]
            Msg::GenerationNotHeld(id) => pick(
                format!("generation {id} is not held here"),
                format!("a geração {id} não está retida aqui"),
                format!("la generación {id} no está retenida aquí"),
            ),
            #[cfg(any(feature = "oidc", feature = "ui"))]
            Msg::OidcUrl { bare: false } => pick(
                "must be an https:// URL with a host and no user, query or fragment (http:// \
                 only on loopback)"
                    .into(),
                "deve ser uma URL https:// com um host e sem usuário, query ou fragmento \
                 (http:// só em loopback)"
                    .into(),
                "debe ser una URL https:// con un host y sin usuario, query ni fragmento \
                 (http:// solo en loopback)"
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
                "debe ser una URL https:// con un host y nada después (http:// solo en \
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
                "debe ser una URL https:// con un host y sin usuario ni fragmento (http:// solo \
                 en loopback)"
                    .into(),
            ),
            #[cfg(any(feature = "oidc", feature = "directory"))]
            Msg::OidcEmpty => pick(
                "must not be empty".into(),
                "não pode ser vazio".into(),
                "no puede estar vacío".into(),
            ),
            #[cfg(feature = "oidc")]
            Msg::OidcNoClaim => pick(
                "set `identities_claim`, `groups_claim` or both".into(),
                "defina `identities_claim`, `groups_claim` ou os dois".into(),
                "define `identities_claim`, `groups_claim` o ambos".into(),
            ),
            #[cfg(any(feature = "oidc", feature = "directory"))]
            Msg::OidcOneOf(accepted) => {
                let mut quoted: Vec<String> = accepted.iter().map(|a| format!("`{a}`")).collect();
                let last = quoted.pop().unwrap_or_default();
                let rest = quoted.join(", ");
                match rest.is_empty() {
                    true => pick(
                        format!("must be {last}"),
                        format!("deve ser {last}"),
                        format!("debe ser {last}"),
                    ),
                    false => pick(
                        format!("must be one of {rest} or {last}"),
                        format!("deve ser um de {rest} ou {last}"),
                        format!("debe ser uno de {rest} o {last}"),
                    ),
                }
            }
            #[cfg(feature = "oidc")]
            Msg::OidcTypeTable(kind) => pick(
                format!("needs `oidc.type = \"{kind}\"`"),
                format!("exige `oidc.type = \"{kind}\"`"),
                format!("requiere `oidc.type = \"{kind}\"`"),
            ),
            #[cfg(feature = "oidc")]
            Msg::OidcScope { scope, bad } => pick(
                format!("{scope:?} holds {bad:?}, which a scope cannot carry (RFC 6749)"),
                format!("{scope:?} contém {bad:?}, que um escopo não pode ter (RFC 6749)"),
                format!("{scope:?} contiene {bad:?}, que un alcance no puede tener (RFC 6749)"),
            ),
            #[cfg(feature = "oidc")]
            Msg::OidcMetadata(tried) => pick(
                format!("oidc: the issuer's metadata could not be read: {tried}"),
                format!("oidc: os metadados do issuer não puderam ser lidos: {tried}"),
                format!("oidc: los metadatos del issuer no pudieron leerse: {tried}"),
            ),
            #[cfg(feature = "oidc")]
            Msg::OidcIssuerMismatch(found) => pick(
                format!("names the issuer {found:?}, not the configured one"),
                format!("nomeia o issuer {found:?}, não o configurado"),
                format!("nombra al issuer {found:?}, no al configurado"),
            ),
            #[cfg(feature = "oidc")]
            Msg::OidcJwksUri(why) => pick(
                format!("its jwks_uri {why}"),
                format!("o seu jwks_uri {why}"),
                format!("su jwks_uri {why}"),
            ),
            #[cfg(feature = "oidc")]
            Msg::OidcJwks { url, why } => pick(
                format!("oidc: the JWKS at {url} could not be read: {why}"),
                format!("oidc: o JWKS em {url} não pôde ser lido: {why}"),
                format!("oidc: el JWKS en {url} no pudo leerse: {why}"),
            ),
            #[cfg(feature = "oidc")]
            Msg::OidcJwksNoKey => pick(
                "it holds no signing key Meada accepts (RSA, or EC on P-256 or P-384)".into(),
                "ele não tem nenhuma chave de assinatura que o Meada aceite (RSA, ou EC em P-256 \
                 ou P-384)"
                    .into(),
                "no tiene ninguna clave de firma que Meada acepte (RSA, o EC en P-256 o P-384)"
                    .into(),
            ),
            #[cfg(feature = "oidc")]
            Msg::OidcJwksShape { line, column } => pick(
                format!("it is not a JSON Web Key Set (line {line}, column {column})"),
                format!("ele não é um JSON Web Key Set (linha {line}, coluna {column})"),
                format!("no es un JSON Web Key Set (línea {line}, columna {column})"),
            ),
            #[cfg(feature = "oidc")]
            Msg::OidcRefreshFailed(why) => pick(
                format!("meada: {why}; keeping the cached keys"),
                format!("meada: {why}; as chaves em cache continuam"),
                format!("meada: {why}; se conservan las claves en caché"),
            ),
            #[cfg(all(feature = "oidc", feature = "postgres"))]
            Msg::PermissionsUnreadable(why) => pick(
                format!("meada: the permissions set cannot be read ({why}); the GitHub grant ages"),
                format!(
                    "meada: o conjunto de permissões não pode ser lido ({why}); o grant do \
                     GitHub envelhece"
                ),
                format!(
                    "meada: el conjunto de permisos no puede leerse ({why}); el grant de GitHub \
                     envejece"
                ),
            ),
            #[cfg(all(feature = "oidc", feature = "postgres"))]
            Msg::PermissionsVersion(why) => pick(
                format!("meada: {why}; Postgres counts as unreachable and the GitHub grant ages"),
                format!(
                    "meada: {why}; o Postgres conta como inalcançável e o grant do GitHub \
                     envelhece"
                ),
                format!(
                    "meada: {why}; Postgres cuenta como inalcanzable y el grant de GitHub \
                     envejece"
                ),
            ),
            #[cfg(all(feature = "oidc", feature = "postgres"))]
            Msg::PermissionsBadRow(table) => pick(
                format!("{table}: a row fails its checks and is skipped"),
                format!("{table}: uma linha falha nas verificações e é ignorada"),
                format!("{table}: una fila falla sus comprobaciones y se omite"),
            ),
            #[cfg(all(feature = "oidc", feature = "directory"))]
            Msg::OverageUnreadable(why) => pick(
                format!("meada: the directory cannot be read for a token in overage ({why}); it has no groups"),
                format!(
                    "meada: o diretório não pode ser lido para um token em overage ({why}); ele \
                     fica sem grupos"
                ),
                format!(
                    "meada: el directorio no puede leerse para un token en overage ({why}); \
                     queda sin grupos"
                ),
            ),
            #[cfg(feature = "snapshot")]
            Msg::GenerationsHelp => pick(
                "Use the generation `meada snapshot pull` made current on this node, with \
                 the --workspace manifest's [snapshot] section"
                    .into(),
                "Usa a geração que `meada snapshot pull` tornou atual neste nó, com a seção \
                 [snapshot] do manifesto de --workspace"
                    .into(),
                "Usa la generación que `meada snapshot pull` dejó como actual en este nodo, con \
                 la sección [snapshot] del manifiesto de --workspace"
                    .into(),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubApp => pick(
                "git credentials from GitHub App installation tokens, for the indexing job."
                    .into(),
                "Credenciais do git a partir de tokens de instalação de GitHub App, para o job \
                 de indexação."
                    .into(),
                "Credenciales de git a partir de tokens de instalación de GitHub App, para el \
                 job de indexación."
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
                "Credential helper de git: responde get con un token de instalación, ignora \
                 store, descarta el token en caché en erase."
                    .into(),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppAction => pick(
                "The operation git asks for: get, store or erase.".into(),
                "A operação que o git pede: get, store ou erase.".into(),
                "La operación que pide git: get, store o erase.".into(),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppThrottled => pick(
                "Mark the installation that last answered for a repository as throttled, after \
                 a fetch that failed with 403 or 429."
                    .into(),
                "Marca como limitada a instalação que respondeu por último para um repositório, \
                 depois de um fetch que falhou com 403 ou 429."
                    .into(),
                "Marca como limitada la instalación que respondió por última vez para un \
                 repositorio, después de un fetch que falló con 403 o 429."
                    .into(),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppRepository => pick(
                "The https URL of the repository whose fetch failed.".into(),
                "A URL https do repositório cujo fetch falhou.".into(),
                "La URL https del repositorio cuyo fetch falló.".into(),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppRetryAfter => pick(
                "The Retry-After of the failed fetch, in seconds; without it, 1 minute, doubled \
                 on each repeat."
                    .into(),
                "O Retry-After do fetch que falhou, em segundos; sem ele, 1 minuto, dobrado a \
                 cada repetição."
                    .into(),
                "El Retry-After del fetch que falló, en segundos; sin él, 1 minuto, duplicado en \
                 cada repetición."
                    .into(),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppHost => pick(
                "must be a git host name, with an optional port".into(),
                "deve ser o nome de um host git, com uma porta opcional".into(),
                "debe ser el nombre de un host git, con un puerto opcional".into(),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppApi => pick(
                "must be an https:// URL with a host and no user, query or fragment (http:// \
                 only on loopback)"
                    .into(),
                "deve ser uma URL https:// com um host e sem usuário, query ou fragmento \
                 (http:// só em loopback)"
                    .into(),
                "debe ser una URL https:// con un host y sin usuario, query ni fragmento \
                 (http:// solo en loopback)"
                    .into(),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppKeyBoth => pick(
                "is set together with private_key_file: use one of the two".into(),
                "está junto de private_key_file: use um dos dois".into(),
                "está junto con private_key_file: usa uno de los dos".into(),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppKeyNone => pick(
                "needs private_key_file or private_key_env".into(),
                "exige private_key_file ou private_key_env".into(),
                "requiere private_key_file o private_key_env".into(),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppTwice => pick(
                "names an App another [[github_app]] block already lists for this host".into(),
                "nomeia um App que outro bloco [[github_app]] já lista para este host".into(),
                "nombra una App que otro bloque [[github_app]] ya lista para este host".into(),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppKey(source) => pick(
                format!("{source}: not an RSA private key in PEM"),
                format!("{source}: não é uma chave privada RSA em PEM"),
                format!("{source}: no es una clave privada RSA en PEM"),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppInput => pick(
                "git's request is not key=value lines within 64 KiB".into(),
                "o pedido do git não são linhas key=value em até 64 KiB".into(),
                "la solicitud de git no son líneas key=value de hasta 64 KiB".into(),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppPath => pick(
                "git's path is not <owner>/<repo>".into(),
                "o path do git não é <owner>/<repo>".into(),
                "el path de git no es <owner>/<repo>".into(),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppNoPath(host) => pick(
                format!("{host}: git sent no path; set credential.useHttpPath = true"),
                format!("{host}: o git não mandou path; defina credential.useHttpPath = true"),
                format!("{host}: git no envió path; define credential.useHttpPath = true"),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppUncovered { host, owner } => pick(
                format!("no GitHub App installation covers {host}/{owner}"),
                format!("nenhuma instalação de GitHub App cobre {host}/{owner}"),
                format!("ninguna instalación de GitHub App cubre {host}/{owner}"),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppAllThrottled(owner) => pick(
                format!("every installation covering {owner} is throttled; trying them anyway"),
                format!("toda instalação que cobre {owner} está limitada; tentando assim mesmo"),
                format!("toda instalación que cubre {owner} está limitada; probándolas de todos modos"),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppNoToken(owner) => pick(
                format!("no installation covering {owner} gave a token: each mint was throttled"),
                format!("nenhuma instalação que cobre {owner} deu um token: cada mint foi limitado"),
                format!("ninguna instalación que cubre {owner} dio un token: cada mint fue limitado"),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppStatus { url, status } => pick(
                format!("{url}: http status {status}"),
                format!("{url}: status http {status}"),
                format!("{url}: status http {status}"),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppAnswer(url) => pick(
                format!("{url}: not an answer GitHub gives"),
                format!("{url}: não é uma resposta que o GitHub dá"),
                format!("{url}: no es una respuesta que da GitHub"),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppMarked { what, secs } => pick(
                format!("{what}: throttled for {secs} s"),
                format!("{what}: limitado durante {secs} s"),
                format!("{what}: limitado por {secs} s"),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppWaited { what, secs } => pick(
                format!("{what}: another run's mint did not finish within {secs} s"),
                format!("{what}: o mint de outra execução não terminou em {secs} s"),
                format!("{what}: el mint de otra ejecución no terminó en {secs} s"),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppNotAnswered(repo) => pick(
                format!("no installation answered for {repo} within the last hour; nothing marked"),
                format!("nenhuma instalação respondeu por {repo} na última hora; nada marcado"),
                format!("ninguna instalación respondió para {repo} en la última hora; nada marcado"),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppRepositoryUrl => pick(
                "the repository URL is not https://<host>/<owner>/<repo>".into(),
                "a URL do repositório não é https://<host>/<owner>/<repo>".into(),
                "la URL del repositorio no es https://<host>/<owner>/<repo>".into(),
            ),
            #[cfg(feature = "github-app")]
            Msg::GithubAppRoles => pick(
                "List each GitHub repository's collaborators with their roles into the \
                 permissions set in Postgres, and write the exclusions file of the pulled version."
                    .into(),
                "Lista os colaboradores de cada repositório do GitHub com seus papéis no conjunto \
                 de permissões no Postgres, e grava o arquivo de exclusões da versão baixada."
                    .into(),
                "Lista los colaboradores de cada repositorio de GitHub con sus roles en el \
                 conjunto de permisos en Postgres, y escribe el archivo de exclusiones de la \
                 versión descargada."
                    .into(),
            ),
            #[cfg(feature = "oidc")]
            Msg::OidcGithubHost => pick(
                "must be a lowercase host name".into(),
                "deve ser um nome de host em minúsculas".into(),
                "debe ser un nombre de host en minúsculas".into(),
            ),
            #[cfg(feature = "oidc")]
            Msg::OidcGithubRoles(max) => pick(
                format!(
                    "must be a non-empty array of distinct role names of 1 to {max} characters \
                     with no control character"
                ),
                format!(
                    "deve ser um array não vazio de nomes de papel distintos, de 1 a {max} \
                     caracteres e sem caractere de controle"
                ),
                format!(
                    "debe ser un array no vacío de nombres de rol distintos, de 1 a {max} \
                     caracteres y sin carácter de control"
                ),
            ),
            #[cfg(feature = "github-app")]
            Msg::RolesNoApp(host) => pick(
                format!("meada github-app roles needs a [[github_app]] block for host {host}"),
                format!("meada github-app roles exige um bloco [[github_app]] para o host {host}"),
                format!("meada github-app roles requiere un bloque [[github_app]] para el host {host}"),
            ),
            #[cfg(feature = "github-app")]
            Msg::RolesNoGithub(manifest) => pick(
                format!("{manifest}: meada github-app roles needs [oidc.github] or [accounts.github]"),
                format!("{manifest}: meada github-app roles exige [oidc.github] ou [accounts.github]"),
                format!("{manifest}: meada github-app roles requiere [oidc.github] o [accounts.github]"),
            ),
            #[cfg(feature = "github-app")]
            Msg::RolesFenceEnv => pick(
                "[refresh_lock] needs MEADA_REFRESH_FENCE: run meada github-app roles under meada \
                 snapshot lock"
                    .into(),
                "[refresh_lock] exige MEADA_REFRESH_FENCE: rode o meada github-app roles sob meada \
                 snapshot lock"
                    .into(),
                "[refresh_lock] requiere MEADA_REFRESH_FENCE: ejecuta meada github-app roles bajo \
                 meada snapshot lock"
                    .into(),
            ),
            #[cfg(feature = "github-app")]
            Msg::RolesFenceBelow { fence, held } => pick(
                format!(
                    "the newest permissions set has fence {held}, above this run's {fence}: a \
                     newer refresh wrote it; nothing was written"
                ),
                format!(
                    "o conjunto de permissões mais novo tem fence {held}, acima do {fence} desta \
                     execução: um refresh mais novo o gravou; nada foi gravado"
                ),
                format!(
                    "el conjunto de permisos más nuevo tiene fence {held}, por encima del {fence} \
                     de esta ejecución: un refresh más nuevo lo escribió; no se escribió nada"
                ),
            ),
            #[cfg(feature = "github-app")]
            Msg::RolesReplaced => pick(
                "a newer permissions set replaced this run's before the exclusions file was \
                 written; run meada github-app roles again"
                    .into(),
                "um conjunto de permissões mais novo substituiu o desta execução antes de o \
                 arquivo de exclusões ser gravado; rode o meada github-app roles de novo"
                    .into(),
                "un conjunto de permisos más nuevo reemplazó al de esta ejecución antes de que \
                 se escribiera el archivo de exclusiones; ejecuta meada github-app roles de \
                 nuevo"
                    .into(),
            ),
            #[cfg(feature = "github-app")]
            Msg::RolesNotRepository(identity) => pick(
                format!("{identity}: not <host>/<owner>/<repo> on GitHub; it gets no rows"),
                format!("{identity}: não é <host>/<owner>/<repo> no GitHub; não recebe linhas"),
                format!("{identity}: no es <host>/<owner>/<repo> en GitHub; no recibe filas"),
            ),
            #[cfg(feature = "github-app")]
            Msg::RolesUncovered(identity) => pick(
                format!("{identity}: no GitHub App installation covers its owner; it gets no rows"),
                format!(
                    "{identity}: nenhuma instalação de GitHub App cobre o dono; não recebe linhas"
                ),
                format!(
                    "{identity}: ninguna instalación de GitHub App cubre al dueño; no recibe filas"
                ),
            ),
            #[cfg(feature = "github-app")]
            Msg::RolesForbidden(identity) => pick(
                format!(
                    "{identity}: GitHub answers 403 (permission, not a rate limit) for its collaborators; it gets no rows"
                ),
                format!(
                    "{identity}: o GitHub responde 403 (permissão, não limite de taxa) para os colaboradores; não recebe linhas"
                ),
                format!(
                    "{identity}: GitHub responde 403 (permiso, no límite de tasa) para los colaboradores; no recibe filas"
                ),
            ),
            #[cfg(feature = "github-app")]
            Msg::RolesNotFound(identity) => pick(
                format!("{identity}: GitHub answers 404 for its collaborators; it gets no rows"),
                format!(
                    "{identity}: o GitHub responde 404 para os colaboradores; não recebe linhas"
                ),
                format!(
                    "{identity}: GitHub responde 404 para los colaboradores; no recibe filas"
                ),
            ),
            #[cfg(feature = "github-app")]
            Msg::RolesSkipped(identity) => pick(
                format!("{identity}: a collaborator GitHub listed has no valid login or role; skipped"),
                format!(
                    "{identity}: um colaborador que o GitHub listou não tem login ou papel \
                     válido; ignorado"
                ),
                format!(
                    "{identity}: un colaborador que GitHub listó no tiene login o rol válido; \
                     omitido"
                ),
            ),
            #[cfg(feature = "github-app")]
            Msg::RolesFailed { identity, why } => pick(
                format!("{identity}: the role check failed: {why}; nothing was written"),
                format!("{identity}: a checagem de papéis falhou: {why}; nada foi gravado"),
                format!("{identity}: la comprobación de roles falló: {why}; no se escribió nada"),
            ),
            #[cfg(feature = "github-app")]
            Msg::RolesThrottled(None) => pick(
                "every installation covering its owner is throttled".into(),
                "toda instalação que cobre o dono está limitada".into(),
                "toda instalación que cubre al dueño está limitada".into(),
            ),
            #[cfg(feature = "github-app")]
            Msg::RolesThrottled(Some(status)) => pick(
                format!("every installation covering its owner is throttled (http status {status})"),
                format!("toda instalação que cobre o dono está limitada (status http {status})"),
                format!("toda instalación que cubre a su dueño está limitada (status http {status})"),
            ),
            #[cfg(feature = "github-app")]
            Msg::RolesLinkForeign => pick(
                "a Link header's next page is not this repository's collaborators listing".into(),
                "a próxima página de um cabeçalho Link não é a listagem de colaboradores deste \
                 repositório"
                    .into(),
                "la página siguiente de un encabezado Link no es el listado de colaboradores de \
                 este repositorio"
                    .into(),
            ),
            #[cfg(feature = "github-app")]
            Msg::RolesLinkCycle => pick(
                "a Link header's next page was listed already".into(),
                "a próxima página de um cabeçalho Link já foi listada".into(),
                "la página siguiente de un encabezado Link ya fue listada".into(),
            ),
            #[cfg(feature = "github-app")]
            Msg::RolesPages(max) => pick(
                format!("more than {max} pages of collaborators"),
                format!("mais de {max} páginas de colaboradores"),
                format!("más de {max} páginas de colaboradores"),
            ),
            #[cfg(feature = "github-app")]
            Msg::RolesExcluded {
                identity,
                listed: false,
            } => pick(
                format!("{identity}: excluded: GitHub lists no role for its added_by"),
                format!("{identity}: excluído: o GitHub não lista papel para o seu added_by"),
                format!("{identity}: excluido: GitHub no lista ningún rol para su added_by"),
            ),
            #[cfg(feature = "github-app")]
            Msg::RolesExcluded {
                identity,
                listed: true,
            } => pick(
                format!("{identity}: excluded: its added_by holds no role in [oidc.github] roles"),
                format!(
                    "{identity}: excluído: o seu added_by não tem papel em [oidc.github] roles"
                ),
                format!(
                    "{identity}: excluido: su added_by no tiene ningún rol en [oidc.github] roles"
                ),
            ),
            #[cfg(feature = "github-app")]
            Msg::RolesUnattributed(identity) => pick(
                format!(
                    "{identity}: excluded: unattributed, its added_by was written before the \
                     GitHub grant provider's first permissions set"
                ),
                format!(
                    "{identity}: excluído: sem atribuição, o seu added_by foi escrito antes do \
                     primeiro conjunto de permissões do provedor de concessões do GitHub"
                ),
                format!(
                    "{identity}: excluido: sin atribución, su added_by se escribió antes del \
                     primer conjunto de permisos del proveedor de concesiones de GitHub"
                ),
            ),
            #[cfg(feature = "github-app")]
            Msg::RolesSet {
                id,
                repositories,
                rows,
                exclusions,
            } => pick(
                format!(
                    "permissions set {id}: {repositories} repositories, {rows} rows, \
                     {exclusions} exclusions"
                ),
                format!(
                    "conjunto de permissões {id}: {repositories} repositórios, {rows} linhas, \
                     {exclusions} exclusões"
                ),
                format!(
                    "conjunto de permisos {id}: {repositories} repositorios, {rows} filas, \
                     {exclusions} exclusiones"
                ),
            ),
            #[cfg(feature = "workspaces")]
            Msg::CatalogAllow(host) => pick(
                format!(
                    "must be a non-empty array of distinct lowercase patterns, each {host}/<owner>/* \
                     or {host}/<owner>/<repo>"
                ),
                format!(
                    "deve ser um array não vazio de padrões distintos em minúsculas, cada um \
                     {host}/<dono>/* ou {host}/<dono>/<repositório>"
                ),
                format!(
                    "debe ser un array no vacío de patrones distintos en minúsculas, cada uno \
                     {host}/<dueño>/* o {host}/<dueño>/<repositorio>"
                ),
            ),
            #[cfg(feature = "workspaces")]
            Msg::RolesCatalogFailed { installation, why } => pick(
                format!(
                    "installation {installation}: listing its repositories for the catalog \
                     failed: {why}; nothing was written"
                ),
                format!(
                    "instalação {installation}: a listagem dos seus repositórios para o catálogo \
                     falhou: {why}; nada foi gravado"
                ),
                format!(
                    "instalación {installation}: el listado de sus repositorios para el catálogo \
                     falló: {why}; no se escribió nada"
                ),
            ),
            #[cfg(feature = "workspaces")]
            Msg::RolesCatalogInstallations { app, why } => pick(
                format!(
                    "GitHub App {app}: listing its installations for the catalog failed: {why}; \
                     nothing was written"
                ),
                format!(
                    "GitHub App {app}: a listagem das suas instalações para o catálogo falhou: \
                     {why}; nada foi gravado"
                ),
                format!(
                    "GitHub App {app}: el listado de sus instalaciones para el catálogo falló: \
                     {why}; no se escribió nada"
                ),
            ),
            #[cfg(feature = "workspaces")]
            Msg::RolesCatalogThrottled(None) => pick(
                "throttled".into(),
                "limitada".into(),
                "limitada".into(),
            ),
            #[cfg(feature = "workspaces")]
            Msg::RolesCatalogThrottled(Some(status)) => pick(
                format!("throttled (http status {status})"),
                format!("limitada (status http {status})"),
                format!("limitada (status http {status})"),
            ),
            #[cfg(feature = "workspaces")]
            Msg::RolesCatalogUnlisted => pick(
                "another run's listing did not finish in time".into(),
                "a listagem de outra execução não terminou a tempo".into(),
                "el listado de otra ejecución no terminó a tiempo".into(),
            ),
            #[cfg(feature = "workspaces")]
            Msg::RolesCatalogPages(max) => pick(
                format!("more than {max} pages of repositories"),
                format!("mais de {max} páginas de repositórios"),
                format!("más de {max} páginas de repositorios"),
            ),
            #[cfg(feature = "workspaces")]
            Msg::RolesCatalogCap { found, max } => pick(
                format!(
                    "the catalog has {found} candidates besides the selected repositories, above \
                     [catalog] max_repositories = {max}; nothing was written"
                ),
                format!(
                    "o catálogo tem {found} candidatos além dos repositórios selecionados, acima \
                     de [catalog] max_repositories = {max}; nada foi gravado"
                ),
                format!(
                    "el catálogo tiene {found} candidatos además de los repositorios \
                     seleccionados, por encima de [catalog] max_repositories = {max}; no se \
                     escribió nada"
                ),
            ),
            #[cfg(feature = "workspaces")]
            Msg::RolesSetCatalog {
                id,
                repositories,
                catalog,
                rows,
                exclusions,
            } => pick(
                format!(
                    "permissions set {id}: {repositories} repositories ({catalog} in the catalog), \
                     {rows} rows, {exclusions} exclusions"
                ),
                format!(
                    "conjunto de permissões {id}: {repositories} repositórios ({catalog} no \
                     catálogo), {rows} linhas, {exclusions} exclusões"
                ),
                format!(
                    "conjunto de permisos {id}: {repositories} repositorios ({catalog} en el \
                     catálogo), {rows} filas, {exclusions} exclusiones"
                ),
            ),
            #[cfg(feature = "workspaces")]
            Msg::CatalogQuery(max) => pick(
                format!(
                    "the catalog takes limit (1 to 100), after and q (at most {max} characters), \
                     each once"
                ),
                format!(
                    "o catálogo aceita limit (1 a 100), after e q (no máximo {max} caracteres), \
                     cada um uma vez"
                ),
                format!(
                    "el catálogo acepta limit (1 a 100), after y q (como máximo {max} \
                     caracteres), cada uno una vez"
                ),
            ),
            #[cfg(feature = "workspaces")]
            Msg::CatalogCheckBody(max) => pick(
                format!("the body must be {{\"identities\": [...]}} with at most {max} identities"),
                format!("o corpo deve ser {{\"identities\": [...]}} com no máximo {max} identidades"),
                format!("el cuerpo debe ser {{\"identities\": [...]}} con como máximo {max} identidades"),
            ),
            #[cfg(feature = "workspaces")]
            Msg::RolesNoSponsor { context, identity } => pick(
                format!("{context}: {identity}: excluded: no operator of the workspace holds an accepted role on it"),
                format!("{context}: {identity}: excluído: nenhum operador do workspace tem papel aceito nele"),
                format!("{context}: {identity}: excluido: ningún operador del workspace tiene un rol aceptado en él"),
            ),
            #[cfg(feature = "workspaces")]
            Msg::WorkspacesMirrors => pick(
                "must be an absolute path without `..`, the root of the mirrors named workspaces use"
                    .into(),
                "deve ser um caminho absoluto sem `..`, a raiz dos espelhos que os workspaces nomeados usam"
                    .into(),
                "debe ser una ruta absoluta sin `..`, la raíz de los espejos que usan los workspaces nombrados"
                    .into(),
            ),
            #[cfg(feature = "workspaces")]
            Msg::WorkspacesNeedsOperations => pick(
                "[workspaces] needs [ui] operations = true".into(),
                "[workspaces] precisa de [ui] operations = true".into(),
                "[workspaces] necesita [ui] operations = true".into(),
            ),
            #[cfg(feature = "workspaces")]
            Msg::WorkspacesNeedsStore => pick(
                "[workspaces] needs [workspace] source = \"store\"".into(),
                "[workspaces] precisa de [workspace] source = \"store\"".into(),
                "[workspaces] necesita [workspace] source = \"store\"".into(),
            ),
            #[cfg(feature = "workspaces")]
            Msg::ContextsMissing(file) => pick(
                format!("{file} is missing: run `meada workspace pull` first"),
                format!("{file} não existe: rode `meada workspace pull` antes"),
                format!("{file} no existe: ejecuta `meada workspace pull` antes"),
            ),
            #[cfg(feature = "workspaces")]
            Msg::ContextsBad(file) => pick(
                format!("{file} is not a contexts file `meada workspace pull` wrote"),
                format!("{file} não é um arquivo de contextos que `meada workspace pull` gravou"),
                format!("{file} no es un archivo de contextos que `meada workspace pull` escribió"),
            ),
            #[cfg(feature = "workspaces")]
            Msg::ContextsOther(file) => pick(
                format!(
                    "{file} names other contexts than the contexts file holds: run `meada \
                     github-app roles` after `meada workspace pull`"
                ),
                format!(
                    "{file} nomeia outros contextos que os do arquivo de contextos: rode `meada \
                     github-app roles` depois de `meada workspace pull`"
                ),
                format!(
                    "{file} nombra otros contextos que los del archivo de contextos: ejecuta \
                     `meada github-app roles` después de `meada workspace pull`"
                ),
            ),
            #[cfg(feature = "workspaces")]
            Msg::WorkspacesDocumentBad(key) => pick(
                format!("{key} is not a named workspace version document"),
                format!("{key} não é um documento de versão de workspace nomeado"),
                format!("{key} no es un documento de versión de workspace nombrado"),
            ),
            #[cfg(feature = "workspaces")]
            Msg::WorkspacesBody => pick(
                "the body must be {\"name\", \"description\"?, \"entries\", \"message\"}".into(),
                "o corpo deve ser {\"name\", \"description\"?, \"entries\", \"message\"}".into(),
                "el cuerpo debe ser {\"name\", \"description\"?, \"entries\", \"message\"}".into(),
            ),
            #[cfg(feature = "workspaces")]
            Msg::WorkspacesNotFound => pick(
                "no such workspace".into(),
                "não há tal workspace".into(),
                "no existe tal workspace".into(),
            ),
            #[cfg(feature = "workspaces")]
            Msg::WorkspacesUnavailable => pick(
                "the workspaces cannot be read or written now (Postgres or the store did not answer)"
                    .into(),
                "os workspaces não podem ser lidos nem gravados agora (o Postgres ou o store não respondeu)"
                    .into(),
                "los workspaces no se pueden leer ni escribir ahora (Postgres o el store no respondió)"
                    .into(),
            ),
            #[cfg(feature = "workspaces")]
            Msg::WorkspacesRefused(count) => pick(
                format!("{count} selected repositories are not eligible for you; nothing was created"),
                format!("{count} repositórios selecionados não são elegíveis para você; nada foi criado"),
                format!("{count} repositorios seleccionados no son elegibles para ti; no se creó nada"),
            ),
            #[cfg(feature = "workspaces")]
            Msg::WorkspacesPending(reason) => pick(
                format!("the permissions set cannot decide now ({reason}); nothing can be selected"),
                format!("o conjunto de permissões não pode decidir agora ({reason}); nada pode ser selecionado"),
                format!("el conjunto de permisos no puede decidir ahora ({reason}); no se puede seleccionar nada"),
            ),
            #[cfg(feature = "workspaces")]
            Msg::WorkspacesLimit(max) => pick(
                format!("there are already {max} workspaces ([workspaces] max_workspaces)"),
                format!("já existem {max} workspaces ([workspaces] max_workspaces)"),
                format!("ya existen {max} workspaces ([workspaces] max_workspaces)"),
            ),
            #[cfg(feature = "workspaces")]
            Msg::WorkspaceDeleted => pick(
                "that workspace was created and deleted: use another name or other entries".into(),
                "esse workspace foi criado e apagado: use outro nome ou outras entradas".into(),
                "ese workspace fue creado y eliminado: usa otro nombre u otras entradas".into(),
            ),
            #[cfg(feature = "workspaces")]
            Msg::WorkspacesQuery(max) => pick(
                format!("the list takes limit (1 to 100) and after (at most {max} characters), each once"),
                format!("a lista aceita limit (1 a 100) e after (no máximo {max} caracteres), cada um uma vez"),
                format!("la lista acepta limit (1 a 100) y after (como máximo {max} caracteres), cada uno una vez"),
            ),
            #[cfg(feature = "workspaces")]
            Msg::ContextsWritten(count) => pick(
                format!("contexts: {count} named workspaces"),
                format!("contextos: {count} workspaces nomeados"),
                format!("contextos: {count} workspaces nombrados"),
            ),
            #[cfg(feature = "workspaces")]
            Msg::WorkspacesCmd => pick(
                "Manage the named workspaces as the platform administrator (feature workspaces)"
                    .into(),
                "Gerencia os workspaces nomeados como administrador da plataforma (feature \
                 workspaces)"
                    .into(),
                "Gestiona los workspaces nombrados como administrador de la plataforma (feature \
                 workspaces)"
                    .into(),
            ),
            #[cfg(feature = "workspaces")]
            Msg::WorkspacesCmdList => pick(
                "List the named workspaces: id, current version, base and name".into(),
                "Lista os workspaces nomeados: id, versão atual, base e nome".into(),
                "Lista los workspaces nombrados: id, versión actual, base y nombre".into(),
            ),
            #[cfg(feature = "workspaces")]
            Msg::WorkspacesCmdShow => pick(
                "Print a named workspace's current version (or another) as [[repository]] entries"
                    .into(),
                "Imprime a versão atual (ou outra) de um workspace nomeado como entradas \
                 [[repository]]"
                    .into(),
                "Imprime la versión actual (u otra) de un workspace nombrado como entradas \
                 [[repository]]"
                    .into(),
            ),
            #[cfg(feature = "workspaces")]
            Msg::WorkspacesCmdVersions => pick(
                "List the versions of a named workspace, newest first, the current one marked"
                    .into(),
                "Lista as versões de um workspace nomeado, da mais nova à mais antiga, marcando \
                 a atual"
                    .into(),
                "Lista las versiones de un workspace nombrado, de la más nueva a la más antigua, \
                 marcando la actual"
                    .into(),
            ),
            #[cfg(feature = "workspaces")]
            Msg::WorkspacesCmdDiff => pick(
                "Print what a version of a named workspace adds, drops and changes against \
                 another"
                    .into(),
                "Imprime o que uma versão de um workspace nomeado adiciona, remove e altera em \
                 relação a outra"
                    .into(),
                "Imprime lo que una versión de un workspace nombrado agrega, elimina y cambia \
                 respecto a otra"
                    .into(),
            ),
            #[cfg(feature = "workspaces")]
            Msg::WorkspacesHelpAgainst => pick(
                "The version to compare with, or `current` (the default)".into(),
                "A versão com a qual comparar, ou `current` (o padrão)".into(),
                "La versión con la que comparar, o `current` (el valor por defecto)".into(),
            ),
            #[cfg(feature = "workspaces")]
            Msg::WorkspacesCmdCreate => pick(
                "Create a named workspace from the [[repository]] entries of a manifest file and \
                 print its id"
                    .into(),
                "Cria um workspace nomeado com as entradas [[repository]] de um manifesto e \
                 imprime seu id"
                    .into(),
                "Crea un workspace nombrado con las entradas [[repository]] de un manifiesto e \
                 imprime su id"
                    .into(),
            ),
            #[cfg(feature = "workspaces")]
            Msg::WorkspacesCmdSave => pick(
                "Store the [[repository]] entries of a manifest file as a version of a named \
                 workspace, moving nothing, and print the version id"
                    .into(),
                "Guarda as entradas [[repository]] de um manifesto como uma versão de um \
                 workspace nomeado, sem mover nada, e imprime o id da versão"
                    .into(),
                "Guarda las entradas [[repository]] de un manifiesto como una versión de un \
                 workspace nombrado, sin mover nada, e imprime el id de la versión"
                    .into(),
            ),
            #[cfg(feature = "workspaces")]
            Msg::WorkspacesCmdApply => pick(
                "Make a stored version the current one of a named workspace (a version that was \
                 current before is a rollback)"
                    .into(),
                "Torna uma versão guardada a atual de um workspace nomeado (uma versão que já \
                 foi a atual é um rollback)"
                    .into(),
                "Convierte una versión guardada en la actual de un workspace nombrado (una \
                 versión que ya fue la actual es un rollback)"
                    .into(),
            ),
            #[cfg(feature = "workspaces")]
            Msg::WorkspacesCmdRename => pick(
                "Rename a named workspace".into(),
                "Renomeia um workspace nomeado".into(),
                "Renombra un workspace nombrado".into(),
            ),
            #[cfg(feature = "workspaces")]
            Msg::WorkspacesCmdDelete => pick(
                "Delete a named workspace, keeping its versions and history".into(),
                "Apaga um workspace nomeado, mantendo suas versões e seu histórico".into(),
                "Elimina un workspace nombrado, conservando sus versiones y su historial".into(),
            ),
            #[cfg(feature = "workspaces")]
            Msg::WorkspacesCmdOperators => pick(
                "Add or remove the operators of a named workspace".into(),
                "Adiciona ou remove os operadores de um workspace nomeado".into(),
                "Agrega o elimina los operadores de un workspace nombrado".into(),
            ),
            #[cfg(feature = "workspaces")]
            Msg::WorkspacesHelpId => pick(
                "The workspace id".into(),
                "O id do workspace".into(),
                "El id del workspace".into(),
            ),
            #[cfg(feature = "workspaces")]
            Msg::WorkspacesHelpBase => pick(
                "The base the change was made against (the base `show` and `list` print)".into(),
                "A base sobre a qual a mudança foi feita (a base que `show` e `list` imprimem)"
                    .into(),
                "La base sobre la que se hizo el cambio (la base que imprimen `show` y `list`)"
                    .into(),
            ),
            #[cfg(feature = "workspaces")]
            Msg::WorkspacesHelpName => pick(
                "The workspace's name (1 to 80 characters)".into(),
                "O nome do workspace (1 a 80 caracteres)".into(),
                "El nombre del workspace (1 a 80 caracteres)".into(),
            ),
            #[cfg(feature = "workspaces")]
            Msg::WorkspacesHelpDescription => pick(
                "The workspace's description (0 to 500 characters)".into(),
                "A descrição do workspace (0 a 500 caracteres)".into(),
                "La descripción del workspace (0 a 500 caracteres)".into(),
            ),
            #[cfg(feature = "workspaces")]
            Msg::WorkspacesHelpAdd => pick(
                "An operator to add, as <subject>=<github login> (repeatable)".into(),
                "Um operador a adicionar, como <sujeito>=<login do github> (repetível)".into(),
                "Un operador a agregar, como <sujeto>=<login de github> (repetible)".into(),
            ),
            #[cfg(feature = "workspaces")]
            Msg::WorkspacesHelpRemove => pick(
                "The subject of an operator to remove (repeatable)".into(),
                "O sujeito de um operador a remover (repetível)".into(),
                "El sujeto de un operador a eliminar (repetible)".into(),
            ),
            #[cfg(feature = "workspaces")]
            Msg::WorkspacesOpBody(shape) => pick(
                format!("the body must be {shape}"),
                format!("o corpo deve ser {shape}"),
                format!("el cuerpo debe ser {shape}"),
            ),
            #[cfg(feature = "workspaces")]
            Msg::WorkspacesStale(seq) => pick(
                format!("the workspace changed since you read it (it is at base {seq} now): reload it and review the change"),
                format!("o workspace mudou desde a sua leitura (está na base {seq} agora): recarregue e revise a mudança"),
                format!("el workspace cambió desde que lo leíste (está en la base {seq} ahora): recárgalo y revisa el cambio"),
            ),
            #[cfg(feature = "workspaces")]
            Msg::WorkspacesNotOperator => pick(
                "only an operator of this workspace or the platform administrator may change it".into(),
                "só um operador deste workspace ou o administrador da plataforma pode alterá-lo".into(),
                "solo un operador de este workspace o el administrador de la plataforma puede modificarlo".into(),
            ),
            #[cfg(feature = "workspaces")]
            Msg::WorkspacesNotAdministrator => pick(
                "only the platform administrator changes a workspace's operators".into(),
                "só o administrador da plataforma altera os operadores de um workspace".into(),
                "solo el administrador de la plataforma modifica los operadores de un workspace".into(),
            ),
            #[cfg(feature = "workspaces")]
            Msg::WorkspacesUnknownVersion => pick(
                "no such version of this workspace".into(),
                "não há tal versão deste workspace".into(),
                "no existe tal versión de este workspace".into(),
            ),
            #[cfg(feature = "workspaces")]
            Msg::WorkspacesAlreadyCurrent => pick(
                "that version is already the current one; nothing changed".into(),
                "essa versão já é a atual; nada mudou".into(),
                "esa versión ya es la actual; no cambió nada".into(),
            ),
            #[cfg(feature = "proposals")]
            Msg::ProposalsCmd => pick(
                "Serve the named workspaces' catalog, validation and stateless proposals to \
                 agents over MCP at host:port, :port or port (loopback without a host); it \
                 writes nothing"
                    .into(),
                "Serve o catálogo, a validação e as propostas sem estado dos workspaces nomeados \
                 a agentes via MCP em host:porta, :porta ou porta (loopback sem host); não grava \
                 nada"
                    .into(),
                "Sirve el catálogo, la validación y las propuestas sin estado de los workspaces \
                 nombrados a agentes vía MCP en host:puerto, :puerto o puerto (loopback sin \
                 host); no escribe nada"
                    .into(),
            ),
            #[cfg(feature = "proposals")]
            Msg::ProposalsListenHelp => pick(
                "Where the proposal server listens: host:port, :port or port (loopback without a \
                 host)"
                    .into(),
                "Onde o servidor de propostas escuta: host:porta, :porta ou porta (loopback sem \
                 host)"
                    .into(),
                "Dónde escucha el servidor de propuestas: host:puerto, :puerto o puerto \
                 (loopback sin host)"
                    .into(),
            ),
            #[cfg(feature = "proposals")]
            Msg::ProposalsWorkspaceHelp => pick(
                "The bootstrap manifest, with its [proposals] and [workspaces] sections".into(),
                "O manifesto de bootstrap, com as seções [proposals] e [workspaces]".into(),
                "El manifiesto de bootstrap, con sus secciones [proposals] y [workspaces]".into(),
            ),
            #[cfg(feature = "proposals")]
            Msg::ProposalsNoSection => pick(
                "the bootstrap manifest has no [proposals] section".into(),
                "o manifesto de bootstrap não tem seção [proposals]".into(),
                "el manifiesto de bootstrap no tiene sección [proposals]".into(),
            ),
            #[cfg(feature = "proposals")]
            Msg::ProposalsArgument(what) => pick(
                format!("invalid arguments: {what}"),
                format!("argumentos inválidos: {what}"),
                format!("argumentos inválidos: {what}"),
            ),
            #[cfg(feature = "proposals")]
            Msg::ProposalsTooLarge(max) => pick(
                format!("the answer would exceed {max} bytes; ask for fewer entries or a page"),
                format!("a resposta passaria de {max} bytes; peça menos entradas ou uma página"),
                format!("la respuesta superaría {max} bytes; pide menos entradas o una página"),
            ),
            #[cfg(feature = "proposals")]
            Msg::ProposalsBusy => pick(
                "too many calls in flight; retry shortly".into(),
                "chamadas demais em andamento; tente de novo em instantes".into(),
                "demasiadas llamadas en curso; vuelve a intentarlo en unos instantes".into(),
            ),
            #[cfg(feature = "proposals")]
            Msg::ProposalsNotAdmitted => pick(
                "the call was not admitted".into(),
                "a chamada não foi admitida".into(),
                "la llamada no fue admitida".into(),
            ),
            #[cfg(feature = "workspaces")]
            Msg::WorkspacesOperators(reason) => pick(
                match *reason {
                    "last_operator" => "a workspace keeps at least one operator".to_string(),
                    "too_many_operators" => "a workspace holds at most 20 operators".to_string(),
                    _ => "the operator list is not valid".to_string(),
                },
                match *reason {
                    "last_operator" => "um workspace mantém pelo menos um operador".to_string(),
                    "too_many_operators" => "um workspace tem no máximo 20 operadores".to_string(),
                    _ => "a lista de operadores não é válida".to_string(),
                },
                match *reason {
                    "last_operator" => "un workspace mantiene al menos un operador".to_string(),
                    "too_many_operators" => "un workspace tiene como máximo 20 operadores".to_string(),
                    _ => "la lista de operadores no es válida".to_string(),
                },
            ),
            #[cfg(feature = "workspaces")]
            Msg::WorkspacesPageQuery(max) => pick(
                format!("the query takes after (at most {max} characters), once"),
                format!("a consulta aceita after (no máximo {max} caracteres), uma vez"),
                format!("la consulta acepta after (como máximo {max} caracteres), una vez"),
            ),
            #[cfg(feature = "workspaces")]
            Msg::WorkspacesEmptyVersion => pick(
                "the version would hold no entry: delete the workspace instead".into(),
                "a versão ficaria sem nenhuma entrada: apague o workspace".into(),
                "la versión se quedaría sin ninguna entrada: elimina el workspace".into(),
            ),
            #[cfg(feature = "workspaces")]
            Msg::WorkspacesReservedSegment(identity) => pick(
                format!("{identity}: the first segment `-` is reserved for named workspaces' routes"),
                format!("{identity}: o primeiro segmento `-` é reservado às rotas de workspaces nomeados"),
                format!("{identity}: el primer segmento `-` está reservado para las rutas de workspaces nombrados"),
            ),
            #[cfg(feature = "generation-pointer")]
            Msg::ExcludedMissing(file) => pick(
                format!("{file} is missing: run `meada github-app roles` after `meada workspace pull`"),
                format!(
                    "{file} não existe: rode `meada github-app roles` depois de `meada workspace \
                     pull`"
                ),
                format!(
                    "{file} no existe: ejecuta `meada github-app roles` después de `meada \
                     workspace pull`"
                ),
            ),
            #[cfg(feature = "generation-pointer")]
            Msg::ExcludedOther {
                file,
                named,
                pulled,
            } => pick(
                format!(
                    "{file} names version {named}, not the pulled version {pulled}: run `meada \
                     github-app roles`"
                ),
                format!(
                    "{file} nomeia a versão {named}, não a versão baixada {pulled}: rode `meada \
                     github-app roles`"
                ),
                format!(
                    "{file} nombra la versión {named}, no la versión descargada {pulled}: \
                     ejecuta `meada github-app roles`"
                ),
            ),
            #[cfg(feature = "generation-pointer")]
            Msg::ExcludedBad { file, line } => pick(
                format!("{file}:{line}: not a version id or an identity: run `meada github-app roles`"),
                format!(
                    "{file}:{line}: não é um id de versão nem uma identidade: rode `meada \
                     github-app roles`"
                ),
                format!(
                    "{file}:{line}: no es un id de versión ni una identidad: ejecuta `meada \
                     github-app roles`"
                ),
            ),
            #[cfg(feature = "ui")]
            Msg::Ui => pick(
                "Serve the management console, read only, at host:port, :port or port (loopback \
                 without a host)."
                    .into(),
                "Serve o console de gestão, só leitura, em host:porta, :porta ou porta (loopback \
                 sem host)."
                    .into(),
                "Sirve la consola de gestión, solo lectura, en host:puerto, :puerto o puerto \
                 (loopback sin host)."
                    .into(),
            ),
            #[cfg(feature = "ui")]
            Msg::UiListenHelp => pick(
                "Where the console listens: host:port, :port or port (loopback without a host)"
                    .into(),
                "Onde o console escuta: host:porta, :porta ou porta (loopback sem host)".into(),
                "Dónde escucha la consola: host:puerto, :puerto o puerto (loopback sin host)"
                    .into(),
            ),
            #[cfg(feature = "ui")]
            Msg::UiWorkspaceHelp => pick(
                "The bootstrap manifest, with its [ui] section".into(),
                "O manifesto de bootstrap, com a seção [ui]".into(),
                "El manifiesto de bootstrap, con su sección [ui]".into(),
            ),
            #[cfg(feature = "ui")]
            Msg::UiNoSection => pick(
                "the bootstrap manifest has no [ui] section".into(),
                "o manifesto de bootstrap não tem seção [ui]".into(),
                "el manifiesto de bootstrap no tiene sección [ui]".into(),
            ),
            #[cfg(all(feature = "ui", feature = "oidc"))]
            Msg::UiUnauthorized => pick(
                "a valid bearer token from the issuer is needed".into(),
                "é preciso um bearer token válido do issuer".into(),
                "se necesita un bearer token válido del issuer".into(),
            ),
            #[cfg(all(feature = "ui", feature = "oidc"))]
            Msg::UiScope => pick(
                "the token lacks a scope the console requires".into(),
                "o token não tem um escopo que o console exige".into(),
                "el token no tiene un scope que la consola exige".into(),
            ),
            #[cfg(all(feature = "ui", feature = "oidc"))]
            Msg::UiCheckBody => pick(
                "the body must be {\"token\": \"<JWT>\"}".into(),
                "o corpo deve ser {\"token\": \"<JWT>\"}".into(),
                "el cuerpo debe ser {\"token\": \"<JWT>\"}".into(),
            ),
            #[cfg(feature = "ui")]
            Msg::UiTooLarge(max) => pick(
                format!("the body is over {max} bytes"),
                format!("o corpo passa de {max} bytes"),
                format!("el cuerpo supera {max} bytes"),
            ),
            #[cfg(feature = "ui")]
            Msg::UiQueryBody => pick(
                "the body must be {\"repository\", \"generation\", \"tool\", \"arguments\"}, the \
                 tool search, context or trace and the arguments its own"
                    .into(),
                "o corpo deve ser {\"repository\", \"generation\", \"tool\", \"arguments\"}, a \
                 ferramenta search, context ou trace e os argumentos os dela"
                    .into(),
                "el cuerpo debe ser {\"repository\", \"generation\", \"tool\", \"arguments\"}, la \
                 herramienta search, context o trace y los argumentos, los de esa herramienta"
                    .into(),
            ),
            #[cfg(feature = "ui")]
            Msg::UiNoRepository(repo) => pick(
                format!("`{repo}` is not a repository this console serves"),
                format!("`{repo}` não é um repositório que este console serve"),
                format!("`{repo}` no es un repositorio que esta consola sirve"),
            ),
            #[cfg(feature = "ui")]
            Msg::UiNotHeld { id, held } => {
                let held = match held.is_empty() {
                    true => None,
                    false => Some(held.join(" ")),
                };
                pick(
                    format!(
                        "generation {id} is not held here; held: {}",
                        held.as_deref().unwrap_or("none")
                    ),
                    format!(
                        "a geração {id} não está retida aqui; retidas: {}",
                        held.as_deref().unwrap_or("nenhuma")
                    ),
                    format!(
                        "la generación {id} no está retenida aquí; retenidas: {}",
                        held.as_deref().unwrap_or("ninguna")
                    ),
                )
            }
            #[cfg(feature = "ui")]
            Msg::UiDirMissing(dir) => pick(
                format!("`{dir}` is not a directory"),
                format!("`{dir}` não é um diretório"),
                format!("`{dir}` no es un directorio"),
            ),
            #[cfg(feature = "ui")]
            Msg::UiVersion {
                dir,
                found: Some(found),
                expected,
            } => pick(
                format!("`{dir}` holds files built for Meada {found}, not {expected}"),
                format!("`{dir}` tem arquivos feitos para o Meada {found}, não {expected}"),
                format!("`{dir}` contiene archivos hechos para Meada {found}, no {expected}"),
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
                format!(
                    "`{dir}` no tiene un meada-ui.json legible con la versión; este es \
                     Meada {expected}"
                ),
            ),
            #[cfg(feature = "ui")]
            Msg::UiNoRoute => pick(
                "no API route at this path".into(),
                "nenhuma rota da API neste caminho".into(),
                "ninguna ruta de la API en esta ruta".into(),
            ),
            #[cfg(feature = "ui")]
            Msg::UiReadTimeout(ms) => pick(
                format!("no answer within {ms} ms"),
                format!("sem resposta em {ms} ms"),
                format!("sin respuesta en {ms} ms"),
            ),
            #[cfg(feature = "ui")]
            Msg::UiNeedsOidc => pick(
                "requires [oidc]".into(),
                "exige [oidc]".into(),
                "exige [oidc]".into(),
            ),
            #[cfg(all(feature = "ui", feature = "accounts"))]
            Msg::UiNeedsAdmission => pick(
                "requires [oidc] or [accounts]".into(),
                "exige [oidc] ou [accounts]".into(),
                "exige [oidc] o [accounts]".into(),
            ),
            #[cfg(feature = "ui")]
            Msg::UiOperatorNeeded => pick(
                "set `ui.operator_groups` or `ui.operator_claim`".into(),
                "defina `ui.operator_groups` ou `ui.operator_claim`".into(),
                "define `ui.operator_groups` o `ui.operator_claim`".into(),
            ),
            #[cfg(feature = "ui")]
            Msg::UiOperatorBoth => pick(
                "set either `ui.operator_groups` or `ui.operator_claim`, not both".into(),
                "defina `ui.operator_groups` ou `ui.operator_claim`, apenas um".into(),
                "define `ui.operator_groups` o `ui.operator_claim`, solo uno".into(),
            ),
            #[cfg(feature = "ui")]
            Msg::UiWritesDisabled => pick(
                "console writes are disabled by `ui.operations`".into(),
                "as escritas da console estão desativadas por `ui.operations`".into(),
                "las escrituras de la consola están desactivadas por `ui.operations`".into(),
            ),
            #[cfg(feature = "ui")]
            Msg::UiOperatorRequired => pick(
                "console writes require an authenticated operator with a subject".into(),
                "as escritas da console exigem um operador autenticado com subject".into(),
                "las escrituras de la consola exigen un operador autenticado con subject".into(),
            ),
            #[cfg(feature = "ui")]
            Msg::UiGlobalOperatorRequired => pick(
                "this action needs the global operator: a team operator may only save and apply \
                 workspace versions"
                    .into(),
                "esta ação exige o operador global: um operador de time só salva e aplica \
                 versões do workspace"
                    .into(),
                "esta acción exige el operador global: un operador de equipo solo guarda y \
                 aplica versiones del workspace"
                    .into(),
            ),
            #[cfg(feature = "ui")]
            Msg::UiEntriesInvalid => pick(
                "workspace entries failed validation".into(),
                "as entradas do workspace falharam na validação".into(),
                "las entradas del workspace fallaron en la validación".into(),
            ),
            #[cfg(feature = "ui")]
            Msg::UiWriteBody => pick(
                "not a valid console operation body".into(),
                "body inválido para uma operação da console".into(),
                "cuerpo no válido para una operación de la consola".into(),
            ),
            #[cfg(any(feature = "ui", feature = "generation-pointer"))]
            Msg::AuditCli(reason) => pick(
                format!(
                    "the audit record cannot be written to Postgres, so nothing was changed: \
                     {reason}"
                ),
                format!(
                    "o registro de auditoria não pode ser gravado no Postgres, então nada foi \
                     alterado: {reason}"
                ),
                format!(
                    "el registro de auditoría no se puede escribir en Postgres, así que no se \
                     cambió nada: {reason}"
                ),
            ),
            #[cfg(feature = "ui")]
            Msg::AuditUnavailable => pick(
                "the audit in Postgres is unavailable: no write is made without its record".into(),
                "a auditoria no Postgres está indisponível: nenhuma escrita é feita sem o seu \
                 registro"
                    .into(),
                "la auditoría en Postgres no está disponible: no se hace ninguna escritura sin \
                 su registro"
                    .into(),
            ),
            #[cfg(all(feature = "ui", feature = "directory"))]
            Msg::UiDeprovisioned => pick(
                "the identity provider deactivated this account".into(),
                "o provedor de identidade desativou esta conta".into(),
                "el proveedor de identidad desactivó esta cuenta".into(),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsBoth(other) => pick(
                format!("is set together with {other}: use one of the two"),
                format!("está junto de {other}: use um dos dois"),
                format!("está definido junto con {other}: usa uno de los dos"),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsNotUtf8 => pick(
                "names a variable whose value is not UTF-8".into(),
                "nomeia uma variável cujo valor não é UTF-8".into(),
                "nombra una variable cuyo valor no es UTF-8".into(),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsUnset(var) => pick(
                format!("the variable {var} is not set"),
                format!("a variável {var} não está definida"),
                format!("la variable {var} no está definida"),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsKeyMissing {
                env,
                file,
                private,
            } => {
                let (en, pt_br, es) = half_of(*private);
                pick(
                    format!("names no {en} key of the signing key pair: set `{env}` or `{file}`"),
                    format!(
                        "não nomeia a chave {pt_br} do par de chaves de assinatura: defina \
                         `{env}` ou `{file}`"
                    ),
                    format!(
                        "no nombra la clave {es} del par de claves de firma: define `{env}` o \
                         `{file}`"
                    ),
                )
            }
            #[cfg(feature = "accounts")]
            Msg::AccountsNotEd25519 { private } => {
                let (en, pt_br, es) = half_of(*private);
                pick(
                    format!("is not an Ed25519 {en} key in PEM"),
                    format!("não é uma chave {pt_br} Ed25519 em PEM"),
                    format!("no es una clave {es} Ed25519 en PEM"),
                )
            }
            #[cfg(feature = "accounts")]
            Msg::AccountsKeyMismatch => pick(
                "does not match the public key: what it signs, the public key does not verify"
                    .into(),
                "não corresponde à chave pública: o que ela assina, a chave pública não verifica"
                    .into(),
                "no corresponde a la clave pública: la clave pública no verifica lo que \
                 esta firma"
                    .into(),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsNodeIssuer => pick(
                "a node admits the agent tokens the console issues, whose issuer is its \
                 `[ui] public_url`: copy the console's [ui] section into this manifest"
                    .into(),
                "um nó admite os tokens de agente que a console emite, cujo emissor é a \
                 `[ui] public_url` dela: copie a seção [ui] da console para este manifesto"
                    .into(),
                "un nodo admite los tokens de agente que emite la consola, cuyo emisor es su \
                 `[ui] public_url`: copia la sección [ui] de la consola en este manifiesto"
                    .into(),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsTokenBody(max) => pick(
                format!("the body must be {{\"surface\": <surface>, \"hours\": <1 to {max}>}}"),
                format!("o corpo deve ser {{\"surface\": <superfície>, \"hours\": <1 a {max}>}}"),
                format!("el cuerpo debe ser {{\"surface\": <superficie>, \"hours\": <1 a {max}>}}"),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsTokenSurface(false) => pick(
                "agent tokens are issued for the surface `nodes`".into(),
                "tokens de agente são emitidos para a superfície `nodes`".into(),
                "los tokens de agente se emiten para la superficie `nodes`".into(),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsTokenSurface(true) => pick(
                "agent tokens are issued for the surfaces `nodes` and `proposals`".into(),
                "tokens de agente são emitidos para as superfícies `nodes` e `proposals`".into(),
                "los tokens de agente se emiten para las superficies `nodes` y `proposals`".into(),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsTokenHours(max) => pick(
                format!("an agent token is valid from 1 to {max} hours"),
                format!("um token de agente vale de 1 a {max} horas"),
                format!("un token de agente es válido de 1 a {max} horas"),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsTokenRefused => pick(
                "this account may not issue agent tokens: it is suspended or unknown".into(),
                "esta conta não pode emitir tokens de agente: está suspensa ou não existe".into(),
                "esta cuenta no puede emitir tokens de agente: está suspendida o no existe".into(),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsTokenIssued => pick(
                "agent token issued".into(),
                "token de agente emitido".into(),
                "token de agente emitido".into(),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsGithubBody => pick(
                "the body must be {\"token\": \"<a GitHub personal access token>\"}".into(),
                "o corpo deve ser {\"token\": \"<um token de acesso pessoal do GitHub>\"}".into(),
                "el cuerpo debe ser {\"token\": \"<un token de acceso personal de GitHub>\"}".into(),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsGithubToken => pick(
                "a GitHub personal access token is visible ASCII characters, with no space".into(),
                "um token de acesso pessoal do GitHub é feito de caracteres ASCII visíveis, sem \
                 espaço"
                    .into(),
                "un token de acceso personal de GitHub se compone de caracteres ASCII visibles, \
                 sin espacio"
                    .into(),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsGithubForbidden => pick(
                "this account may not change its GitHub login: it is suspended or unknown".into(),
                "esta conta não pode mudar seu login do GitHub: está suspensa ou não existe".into(),
                "esta cuenta no puede cambiar su login de GitHub: está suspendida o no existe"
                    .into(),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsGithubRefused => pick(
                "GitHub refused this personal access token".into(),
                "o GitHub recusou este token de acesso pessoal".into(),
                "GitHub rechazó este token de acceso personal".into(),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsGithubLogin => pick(
                "GitHub answered with a login Meada does not take".into(),
                "o GitHub respondeu com um login que o Meada não aceita".into(),
                "GitHub respondió con un login que Meada no acepta".into(),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsGithubUnreachable(reason) => pick(
                format!("GitHub out of reach for a login verification: {reason}"),
                format!("GitHub fora de alcance para uma verificação de login: {reason}"),
                format!("GitHub fuera de alcance para una verificación de login: {reason}"),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsGithubUnavailable => pick(
                "GitHub could not be reached; try again later".into(),
                "não foi possível alcançar o GitHub; tente de novo mais tarde".into(),
                "no se pudo alcanzar GitHub; inténtalo de nuevo más tarde".into(),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsLoginTaken => pick(
                "another account already verified this GitHub login".into(),
                "outra conta já verificou este login do GitHub".into(),
                "otra cuenta ya verificó este login de GitHub".into(),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsGithubVerified => pick(
                "GitHub login verified".into(),
                "login do GitHub verificado".into(),
                "login de GitHub verificado".into(),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsGithubRemoved => pick(
                "GitHub login removed".into(),
                "login do GitHub removido".into(),
                "login de GitHub eliminado".into(),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsBesideOidc => pick(
                "[accounts] cannot sit beside [oidc]: local sign-in is off once an identity \
                 provider is configured; only `break_glass = true` keeps the default account"
                    .into(),
                "[accounts] não pode ficar junto de [oidc]: o login local fica desligado quando \
                 há um provedor de identidade; só `break_glass = true` mantém a conta padrão"
                    .into(),
                "[accounts] no puede ir junto a [oidc]: el inicio de sesión local queda \
                 desactivado cuando hay un proveedor de identidad; solo `break_glass = true` \
                 mantiene la cuenta predeterminada"
                    .into(),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsBreakGlassNeedsOidc => pick(
                "needs [oidc]: break glass keeps the default account beside an identity provider, \
                 and none is configured"
                    .into(),
                "precisa de [oidc]: o break glass mantém a conta padrão junto de um provedor de \
                 identidade, e não há nenhum"
                    .into(),
                "requiere [oidc]: el break glass mantiene la cuenta predeterminada junto a un \
                 proveedor de identidad, y no hay ninguno configurado"
                    .into(),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsUnderBreakGlass(table) => pick(
                format!(
                    "[{table}] cannot sit under `break_glass = true`: the break-glass account is \
                     the global operator and needs no grant"
                ),
                format!(
                    "[{table}] não pode ficar sob `break_glass = true`: a conta de break glass é \
                     o operador global e não precisa de concessão"
                ),
                format!(
                    "[{table}] no puede estar bajo `break_glass = true`: la cuenta de break glass \
                     es el operador global y no necesita concesión"
                ),
            ),
            #[cfg(all(feature = "ui", feature = "accounts"))]
            Msg::AccountsBreakGlassIssuer => pick(
                "the identity provider's issuer is the console's own public_url: under break glass \
                 a token's issuer must tell the two apart"
                    .into(),
                "o emissor do provedor de identidade é a própria public_url da console: com break \
                 glass o emissor de um token precisa distinguir os dois"
                    .into(),
                "el emisor del proveedor de identidad es la propia public_url de la consola: con \
                 break glass el emisor de un token debe distinguir los dos"
                    .into(),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsPasswordShort(min) => pick(
                format!("the password has fewer than {min} characters"),
                format!("a senha tem menos de {min} caracteres"),
                format!("la contraseña tiene menos de {min} caracteres"),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsPasswordLong(max) => pick(
                format!("the password has more than {max} characters"),
                format!("a senha tem mais de {max} caracteres"),
                format!("la contraseña tiene más de {max} caracteres"),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsPasswordUsername => pick(
                "the password is the username".into(),
                "a senha é o nome de usuário".into(),
                "la contraseña es el nombre de usuario".into(),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsPasswordCurrent => pick(
                "the new password is the current one".into(),
                "a nova senha é a atual".into(),
                "la nueva contraseña es la actual".into(),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsPasswordCommon => pick(
                "the password is on the list of common passwords".into(),
                "a senha está na lista de senhas comuns".into(),
                "la contraseña está en la lista de contraseñas comunes".into(),
            ),
            #[cfg(all(feature = "ui", feature = "accounts"))]
            Msg::AccountsResetNoSection(file) => pick(
                format!("{file} has no [accounts] section: there is no default account to reset"),
                format!("{file} não tem a seção [accounts]: não há conta padrão a redefinir"),
                format!("{file} no tiene la sección [accounts]: no hay cuenta predeterminada que restablecer"),
            ),
            #[cfg(all(feature = "ui", feature = "accounts"))]
            Msg::AccountsResetAdmin { created } => match created {
                true => pick(
                    "admin created as an operator, a password change required".into(),
                    "admin criado como operador, com troca de senha obrigatória".into(),
                    "admin creado como operador, con cambio de contraseña obligatorio".into(),
                ),
                false => pick(
                    "admin reactivated as an operator, its lock cleared, a password change \
                     required"
                        .into(),
                    "admin reativado como operador, bloqueio removido, com troca de senha \
                     obrigatória"
                        .into(),
                    "admin reactivado como operador, bloqueo eliminado, con cambio de contraseña \
                     obligatorio"
                        .into(),
                ),
            },
            #[cfg(all(feature = "ui", feature = "accounts"))]
            Msg::AccountsResetDone { created, from } => {
                let done = Msg::AccountsResetAdmin { created: *created }.text();
                match from {
                    Some(key) => pick(
                        format!("{done}; its password is the one `{key}` names"),
                        format!("{done}; a senha é a que `{key}` indica"),
                        format!("{done}; la contraseña es la que indica `{key}`"),
                    ),
                    None => pick(
                        format!("{done}; its generated password is on stdout, shown once"),
                        format!("{done}; a senha gerada está na saída padrão, mostrada uma vez"),
                        format!("{done}; la contraseña generada está en la salida estándar, mostrada una vez"),
                    ),
                }
            }
            #[cfg(all(feature = "ui", feature = "accounts"))]
            Msg::UiResetAdmin => pick(
                "Recover the default account: create or reactivate admin as an operator, clear \
                 its lock and set a password that must be changed at the next sign-in."
                    .into(),
                "Recupera a conta padrão: cria ou reativa admin como operador, remove o bloqueio \
                 e define uma senha que precisa ser trocada no próximo login."
                    .into(),
                "Recupera la cuenta predeterminada: crea o reactiva admin como operador, elimina \
                 el bloqueo y define una contraseña que debe cambiarse en el próximo inicio de \
                 sesión."
                    .into(),
            ),
            #[cfg(all(feature = "ui", feature = "accounts"))]
            Msg::UiResetAdminWorkspaceHelp => pick(
                "The bootstrap manifest, with its [accounts] and [postgres] sections".into(),
                "O manifesto de bootstrap, com as seções [accounts] e [postgres]".into(),
                "El manifiesto de bootstrap, con las secciones [accounts] y [postgres]".into(),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsBootstrap(reason) => pick(
                format!("the default account cannot be created: {reason}"),
                format!("a conta padrão não pode ser criada: {reason}"),
                format!("la cuenta predeterminada no se puede crear: {reason}"),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsBusy => pick(
                "too many sign-ins at once: try again".into(),
                "logins demais ao mesmo tempo: tente de novo".into(),
                "demasiados inicios de sesión a la vez: inténtalo de nuevo".into(),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsInvalid => pick(
                "the username or the password is wrong".into(),
                "o nome de usuário ou a senha está errado".into(),
                "el nombre de usuario o la contraseña es incorrecto".into(),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsUnauthorized => pick(
                "a valid session token is needed: sign in".into(),
                "é preciso um token de sessão válido: entre".into(),
                "se necesita un token de sesión válido: inicia sesión".into(),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsChangeRequired => pick(
                "the password must be changed first".into(),
                "a senha precisa ser trocada antes".into(),
                "la contraseña debe cambiarse antes".into(),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsLoginBody => pick(
                "the body must be {\"username\", \"password\"}".into(),
                "o corpo deve ser {\"username\", \"password\"}".into(),
                "el cuerpo debe ser {\"username\", \"password\"}".into(),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsPasswordBody => pick(
                "the body must be {\"current\", \"new\"}".into(),
                "o corpo deve ser {\"current\", \"new\"}".into(),
                "el cuerpo debe ser {\"current\", \"new\"}".into(),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsBadRow(table) => pick(
                format!("accounts: a row of {table} fails its checks and admits nothing"),
                format!("contas: uma linha de {table} falha nas verificações e não admite nada"),
                format!("cuentas: una fila de {table} falla sus verificaciones y no admite nada"),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsSignedIn => pick("signed in".into(), "entrou".into(), "sesión iniciada".into()),
            #[cfg(feature = "accounts")]
            Msg::AccountsPasswordChanged => pick("password changed".into(), "senha trocada".into(), "contraseña cambiada".into()),
            #[cfg(feature = "accounts")]
            Msg::AccountsUnavailable => pick(
                "the accounts in Postgres are unavailable".into(),
                "as contas no Postgres estão indisponíveis".into(),
                "las cuentas en Postgres no están disponibles".into(),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsUnsigned => pick(
                "the session token could not be signed".into(),
                "o token de sessão não pôde ser assinado".into(),
                "el token de sesión no se pudo firmar".into(),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsOperatorRequired => pick(
                "account administration needs the global operator".into(),
                "a administração de contas exige o operador global".into(),
                "la administración de cuentas requiere el operador global".into(),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsCreateBody => pick(
                "the body must be {\"username\", \"role\", \"password\"}, with optional \
                 \"groups\" and \"change_required\""
                    .into(),
                "o corpo deve ser {\"username\", \"role\", \"password\"}, com \"groups\" e \
                 \"change_required\" opcionais"
                    .into(),
                "el cuerpo debe ser {\"username\", \"role\", \"password\"}, con \"groups\" y \
                 \"change_required\" opcionales"
                    .into(),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsChangeBody => pick(
                "the body must be one {\"action\"} (`role`, `groups`, `suspend`, `reactivate` \
                 or `reset_password`) with that action's fields"
                    .into(),
                "o corpo deve ser uma {\"action\"} (`role`, `groups`, `suspend`, `reactivate` \
                 ou `reset_password`) com os campos dessa ação"
                    .into(),
                "el cuerpo debe ser una {\"action\"} (`role`, `groups`, `suspend`, `reactivate` \
                 o `reset_password`) con los campos de esa acción"
                    .into(),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsUsernameInvalid => {
                let max = crate::accounts::USERNAME_MAX;
                pick(
                    format!("a username is 1 to {max} lowercase ASCII letters, digits, `.`, `-` and `_`"),
                    format!("um nome de usuário tem de 1 a {max} letras ASCII minúsculas, dígitos, `.`, `-` e `_`"),
                    format!("un nombre de usuario tiene de 1 a {max} letras ASCII minúsculas, dígitos, `.`, `-` y `_`"),
                )
            }
            #[cfg(feature = "accounts")]
            Msg::AccountsTaken => pick(
                "another account has that username".into(),
                "outra conta tem esse nome de usuário".into(),
                "otra cuenta tiene ese nombre de usuario".into(),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsUnknown => pick(
                "no account has that username".into(),
                "nenhuma conta tem esse nome de usuário".into(),
                "ninguna cuenta tiene ese nombre de usuario".into(),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsLastOperator => pick(
                "this would leave no active operator account".into(),
                "isto deixaria a console sem nenhuma conta de operador ativa".into(),
                "esto dejaría la consola sin ninguna cuenta de operador activa".into(),
            ),
            #[cfg(feature = "accounts")]
            Msg::AccountsCreated => pick("account created".into(), "conta criada".into(), "cuenta creada".into()),
            #[cfg(feature = "accounts")]
            Msg::AccountsRoleChanged => pick("role changed".into(), "papel trocado".into(), "rol cambiado".into()),
            #[cfg(feature = "accounts")]
            Msg::AccountsGroupsChanged => pick("groups changed".into(), "grupos trocados".into(), "grupos cambiados".into()),
            #[cfg(feature = "accounts")]
            Msg::AccountsGroupInvalid => {
                let max = meada_core::manifest::OWNER_MAX;
                pick(
                    format!(
                        "a group is a name an entry's owner may be: 1 to {max} characters, no \
                         control character, no leading or trailing space"
                    ),
                    format!(
                        "um grupo é um nome que o dono de uma entrada pode ter: de 1 a {max} \
                         caracteres, sem caractere de controle nem espaço no início ou no fim"
                    ),
                    format!(
                        "un grupo es un nombre que el dueño de una entrada puede tener: de 1 a \
                         {max} caracteres, sin carácter de control ni espacio al inicio o al final"
                    ),
                )
            }
            #[cfg(feature = "accounts")]
            Msg::AccountsSuspended => pick("account suspended".into(), "conta suspensa".into(), "cuenta suspendida".into()),
            #[cfg(feature = "accounts")]
            Msg::AccountsReactivated => pick("account reactivated".into(), "conta reativada".into(), "cuenta reactivada".into()),
            #[cfg(feature = "accounts")]
            Msg::AccountsPasswordReset => pick("password reset".into(), "senha redefinida".into(), "contraseña restablecida".into()),
            #[cfg(all(feature = "ui", feature = "directory"))]
            Msg::DirectoryUnreadable(reason) => pick(
                format!("directory: unreadable ({reason})"),
                format!("directory: ilegível ({reason})"),
                format!("directory: ilegible ({reason})"),
            ),
            #[cfg(all(feature = "ui", feature = "directory"))]
            Msg::DirectoryUnavailable => pick(
                "the directory in Postgres is unavailable".into(),
                "o diretório no Postgres está indisponível".into(),
                "el directorio en Postgres no está disponible".into(),
            ),
            #[cfg(all(feature = "ui", feature = "directory"))]
            Msg::DirectoryStateMissing => pick(
                "the directory's state is missing".into(),
                "o estado do diretório está ausente".into(),
                "falta el estado del directorio".into(),
            ),
            #[cfg(all(feature = "ui", feature = "directory"))]
            Msg::UiDirectoryQuery(max) => pick(
                format!("the owner picker takes one `prefix=<text>` of at most {max} characters"),
                format!("o seletor de owner aceita um `prefix=<texto>` de até {max} caracteres"),
                format!("el selector de owner acepta un `prefix=<texto>` de hasta {max} caracteres"),
            ),
            #[cfg(feature = "ui")]
            Msg::UiAuditQuery => pick(
                "the audit takes `before=<id>` and `limit=<1 to 100>`, nothing else".into(),
                "a auditoria aceita `before=<id>` e `limit=<1 a 100>`, nada mais".into(),
                "la auditoría acepta `before=<id>` y `limit=<1 a 100>`, nada más".into(),
            ),
            #[cfg(feature = "ui")]
            Msg::AuditBadRecord(table) => pick(
                format!("{table}: a record fails its checks and is left out"),
                format!("{table}: um registro falha nas verificações e fica de fora"),
                format!("{table}: un registro falla sus verificaciones y queda fuera"),
            ),
            Msg::About => pick(
                "LLM-free code graph for agents and people".into(),
                "Grafo de código sem LLM para agentes e pessoas".into(),
                "Grafo de código sin LLM para agentes y personas".into(),
            ),
            Msg::Index => pick(
                "Build the indexes of the workspace of the repository in the current directory."
                    .into(),
                "Constrói os índices do workspace do repositório no diretório atual.".into(),
                "Construye los índices del workspace del repositorio en el directorio actual."
                    .into(),
            ),
            Msg::Status => pick(
                "Show the workspace: each repository, its index state and any peer not found."
                    .into(),
                "Mostra o workspace: cada repositório, o estado do índice e os pares não encontrados."
                    .into(),
                "Muestra el workspace: cada repositorio, el estado del índice y los pares no encontrados."
                    .into(),
            ),
            Msg::Export => pick(
                "Print the index as sorted JSON, with the links that touch it.".into(),
                "Imprime o índice como JSON ordenado, com os links que o tocam.".into(),
                "Imprime el índice como JSON ordenado, con los links que lo tocan.".into(),
            ),
            Msg::Mcp => pick(
                "Serve the repository in the current directory to agents over MCP (stdio).".into(),
                "Serve o repositório do diretório atual a agentes por MCP (stdio).".into(),
                "Sirve el repositorio del directorio actual a agentes por MCP (stdio).".into(),
            ),
            Msg::Decide => pick(
                "Resolve the open items of this repository, ambiguous links and dynamic URLs, \
                 and record the picks in meada.decisions.toml."
                    .into(),
                "Resolve os itens abertos deste repositório, links ambíguos e URLs dinâmicas, \
                 e registra as escolhas em meada.decisions.toml."
                    .into(),
                "Resuelve los elementos abiertos de este repositorio, links ambiguos y URL \
                 dinámicas, y registra las elecciones en meada.decisions.toml."
                    .into(),
            ),
            Msg::DecideItemHelp => pick(
                "Work on this item only, even one already decided, to redo it (a call-site id, \
                 as `meada decide --list` or the MCP `search` tool prints it)."
                    .into(),
                "Trabalha só neste item, mesmo que já decidido, para refazê-lo (o id da chamada, \
                 como `meada decide --list` ou a ferramenta `search` do MCP o imprime)."
                    .into(),
                "Trabaja solo en este elemento, incluso si ya está decidido, para rehacerlo (el \
                 id de la llamada, como lo imprime `meada decide --list` o la herramienta \
                 `search` de MCP)."
                    .into(),
            ),
            Msg::DecideDryRunHelp => pick(
                "Print the decisions the run would write and leave the file unchanged.".into(),
                "Imprime as decisões que a execução gravaria e deixa o arquivo como está.".into(),
                "Imprime las decisiones que la ejecución escribiría y deja el archivo como está."
                    .into(),
            ),
            Msg::DecideListHelp => pick(
                "Print the open items; neither ask nor write.".into(),
                "Imprime os itens abertos; não pergunta nem grava.".into(),
                "Imprime los elementos abiertos; no pregunta ni escribe.".into(),
            ),
            Msg::DecidePruneHelp => pick(
                "Remove the stale decisions from meada.decisions.toml; keep those waiting on a \
                 missing peer."
                    .into(),
                "Remove as decisões obsoletas de meada.decisions.toml; mantém as que aguardam \
                 um par ausente."
                    .into(),
                "Elimina las decisiones obsoletas de meada.decisions.toml; conserva las que \
                 esperan a un par ausente."
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
                format!(
                    "{n} {} de meada.decisions.toml",
                    if *n == 1 { "decisión obsoleta eliminada" } else { "decisiones obsoletas eliminadas" }
                ),
            ),
            Msg::CallRepeated(call) => pick(
                format!("`call`: `{call}` is decided twice, one entry per call"),
                format!("`call`: `{call}` aparece duas vezes, uma entrada por chamada"),
                format!("`call`: `{call}` aparece dos veces, una entrada por llamada"),
            ),
            Msg::DecideNothingToPrune => {
                pick("nothing to prune".into(), "nada a remover".into(), "nada que eliminar".into())
            }
            Msg::DecideNoItems => pick("no open items".into(), "nenhum item aberto".into(), "ningún elemento abierto".into()),
            Msg::DecideUnknownItem(id) => pick(
                format!("`{id}` is not an open or decided item of this repository"),
                format!("`{id}` não é um item aberto nem decidido deste repositório"),
                format!("`{id}` no es un elemento abierto ni decidido de este repositorio"),
            ),
            Msg::DecideNoCandidates => pick(
                "no candidate fits; skipped".into(),
                "nenhum candidato serve; pulado".into(),
                "ningún candidato sirve; omitido".into(),
            ),
            Msg::DecidePrompt(n) => pick(
                format!("pick 1-{n}, or s to skip:"),
                format!("escolha 1-{n}, ou s para pular:"),
                format!("elige 1-{n}, o s para omitir:"),
            ),
            Msg::DecideRetry(n) => pick(
                format!("enter a number from 1 to {n}, or s to skip:"),
                format!("digite um número de 1 a {n}, ou s para pular:"),
                format!("escribe un número de 1 a {n}, o s para omitir:"),
            ),
            Msg::DecideNothing => pick("nothing decided".into(), "nada decidido".into(), "nada decidido".into()),
            Msg::DecideWrote(n) => pick(
                format!(
                    "wrote {n} decision{} to meada.decisions.toml",
                    if *n == 1 { "" } else { "s" }
                ),
                format!(
                    "{n} {} em meada.decisions.toml",
                    if *n == 1 { "decisão gravada" } else { "decisões gravadas" }
                ),
                format!(
                    "{n} {} en meada.decisions.toml",
                    if *n == 1 { "decisión escrita" } else { "decisiones escritas" }
                ),
            ),
            Msg::DecideDryRun => pick(
                "dry run, meada.decisions.toml would be:".into(),
                "execução a seco, meada.decisions.toml ficaria assim:".into(),
                "simulación, meada.decisions.toml quedaría así:".into(),
            ),
            Msg::DecideDeciderHelp => pick(
                "Ask the model the MEADA_DECIDER_* variables configure instead of a person."
                    .into(),
                "Pergunta ao modelo configurado pelas variáveis MEADA_DECIDER_* em vez de a uma \
                 pessoa."
                    .into(),
                "Pregunta al modelo que configuran las variables MEADA_DECIDER_* en lugar de a \
                 una persona."
                    .into(),
            ),
            Msg::DecideShowRequestHelp => pick(
                "With --decider: print every request the run would send, and send none.".into(),
                "Com --decider: imprime cada requisição que a execução enviaria, e não envia \
                 nenhuma."
                    .into(),
                "Con --decider: imprime cada solicitud que la ejecución enviaría, y no envía \
                 ninguna."
                    .into(),
            ),
            Msg::DecideTooMany { count, max } => pick(
                format!("{count} candidates, more than {max}; skipped in decider mode"),
                format!("{count} candidatos, mais de {max}; pulado no modo decisor"),
                format!("{count} candidatos, más de {max}; omitido en el modo decisor"),
            ),
            Msg::DecideFailed(error) => pick(
                format!("request failed: {error}"),
                format!("a requisição falhou: {error}"),
                format!("la solicitud falló: {error}"),
            ),
            Msg::DecideProposal {
                candidate,
                p,
                confidence,
                by,
            } => {
                let (en, pt_br, es) = confidence_of(*confidence);
                pick(
                    format!("proposed: {candidate} (p={p:.2}{en}, by {by})"),
                    format!("proposto: {candidate} (p={p:.2}{pt_br}, por {by})"),
                    format!("propuesto: {candidate} (p={p:.2}{es}, por {by})"),
                )
            }
            Msg::DecideSuggestion {
                candidate,
                p,
                confidence,
                min_p,
            } => {
                let (en, pt_br, es) = confidence_of(*confidence);
                pick(
                    format!(
                        "suggestion, not written: {candidate} (p={p:.2}{en}, below --min-p \
                         {min_p:.2})"
                    ),
                    format!(
                        "sugestão, não gravada: {candidate} (p={p:.2}{pt_br}, abaixo de --min-p \
                         {min_p:.2})"
                    ),
                    format!(
                        "sugerencia, no escrita: {candidate} (p={p:.2}{es}, por debajo de --min-p \
                         {min_p:.2})"
                    ),
                )
            }
            Msg::DecideInvalid(reason) => pick(
                format!("invalid answer: {reason}"),
                format!("resposta inválida: {reason}"),
                format!("respuesta inválida: {reason}"),
            ),
            Msg::AnswerUnreadable(why) => pick(
                format!("unreadable: {why}"),
                format!("ilegível: {why}"),
                format!("ilegible: {why}"),
            ),
            Msg::AnswerChoice { choice, keys } => pick(
                format!("choice `{choice}` is not one of the offered keys ({keys})"),
                format!("a escolha `{choice}` não é uma das chaves oferecidas ({keys})"),
                format!("la elección `{choice}` no es una de las claves ofrecidas ({keys})"),
            ),
            Msg::AnswerProbability(p) => pick(
                format!("probability {p} is not between 0 and 1"),
                format!("a probabilidade {p} não está entre 0 e 1"),
                format!("la probabilidad {p} no está entre 0 y 1"),
            ),
            Msg::AnswerBy(by) => pick(
                format!("model id {by:?} cannot be recorded"),
                format!("o id de modelo {by:?} não pode ser gravado"),
                format!("el id de modelo {by:?} no se puede registrar"),
            ),
            Msg::AnswerType(kind) => pick(
                format!("answer type `{kind}` is not `choice`"),
                format!("o tipo de resposta `{kind}` não é `choice`"),
                format!("el tipo de respuesta `{kind}` no es `choice`"),
            ),
            Msg::AnswerConfidence(c) => pick(
                format!("confidence {c} is not between 0 and 1"),
                format!("a confiança {c} não está entre 0 e 1"),
                format!("la confianza {c} no está entre 0 y 1"),
            ),
            Msg::AnswerKey(key) => pick(
                format!("probability key `{key}` is not an offered key"),
                format!("a chave de probabilidade `{key}` não é uma chave oferecida"),
                format!("la clave de probabilidad `{key}` no es una clave ofrecida"),
            ),
            Msg::AnswerSum { sum, tolerance } => pick(
                format!("probabilities sum to {sum:.4}, not 1 (tolerance {tolerance})"),
                format!("as probabilidades somam {sum:.4}, não 1 (tolerância {tolerance})"),
                format!("las probabilidades suman {sum:.4}, no 1 (tolerancia {tolerance})"),
            ),
            Msg::AnswerNoProbability(choice) => pick(
                format!("no probability for the choice `{choice}`"),
                format!("nenhuma probabilidade para a escolha `{choice}`"),
                format!("ninguna probabilidad para la elección `{choice}`"),
            ),
            Msg::AnswerMissing => pick(
                "no answer for this item".into(),
                "nenhuma resposta para este item".into(),
                "ninguna respuesta para este elemento".into(),
            ),
            Msg::AnswerUnknown(name) => pick(
                format!("answer to unknown question `{name}`"),
                format!("resposta a uma pergunta desconhecida `{name}`"),
                format!("respuesta a una pregunta desconocida `{name}`"),
            ),
            Msg::DecideMinP => pick(
                "`--min-p` must be a number from 0 to 1".into(),
                "`--min-p` deve ser um número de 0 a 1".into(),
                "`--min-p` debe ser un número de 0 a 1".into(),
            ),
            Msg::DecideMinPHelp => pick(
                format!("With --decider: write a model decision only from this probability up (default {}).", crate::decider::MIN_P),
                format!("Com --decider: grava a decisão do modelo só a partir desta probabilidade (padrão {}).", crate::decider::MIN_P),
                format!("Con --decider: escribe la decisión del modelo solo a partir de esta probabilidad (por defecto {}).", crate::decider::MIN_P),
            ),
            Msg::DecideFailures(n) => pick(
                format!("{n} item(s) failed, see above; the other decisions were handled"),
                format!("{n} item(ns) falharam, veja acima; as demais decisões foram tratadas"),
                format!("fallaron {n} elemento(s), ver arriba; las demás decisiones se procesaron"),
            ),
            Msg::DeciderInvalid(var) => pick(
                format!("`{var}` is not valid"),
                format!("`{var}` não é válida"),
                format!("`{var}` no es válida"),
            ),
            Msg::DeciderProtocol => pick(
                "`MEADA_DECIDER_PROTOCOL` must be `decisions` or `chat`".into(),
                "`MEADA_DECIDER_PROTOCOL` deve ser `decisions` ou `chat`".into(),
                "`MEADA_DECIDER_PROTOCOL` debe ser `decisions` o `chat`".into(),
            ),
            Msg::DeciderHttps => pick(
                "`MEADA_DECIDER_URL` must use https, or http to a loopback address".into(),
                "`MEADA_DECIDER_URL` deve usar https, ou http para um endereço de loopback".into(),
                "`MEADA_DECIDER_URL` debe usar https, o http hacia una dirección de loopback".into(),
            ),
            Msg::DecisionPHuman => pick(
                "`p` is for a model, not for `human`".into(),
                "`p` é para um modelo, não para `human`".into(),
                "`p` es para un modelo, no para `human`".into(),
            ),
            Msg::DecisionPMissing => pick(
                "`p` is missing for a model".into(),
                "falta `p` para um modelo".into(),
                "falta `p` para un modelo".into(),
            ),
            Msg::DecisionPRange => pick(
                "`p` must be between 0 and 1".into(),
                "`p` deve estar entre 0 e 1".into(),
                "`p` debe estar entre 0 y 1".into(),
            ),
            Msg::HttpStatus { status, message } => {
                let message = message.map(|m| format!(": {m}")).unwrap_or_default();
                pick(
                    format!("http status: {status}{message}"),
                    format!("status http: {status}{message}"),
                    format!("status http: {status}{message}"),
                )
            }
            Msg::AnswerNoChoices => pick(
                "unreadable: no choices".into(),
                "ilegível: nenhuma escolha em `choices`".into(),
                "ilegible: ninguna elección en `choices`".into(),
            ),
            #[cfg(not(feature = "generation-pointer"))]
            Msg::WorkspaceStoreNeedsFeature => pick(
                "\"store\" needs a Meada built with the generation-pointer feature".into(),
                "\"store\" exige um Meada compilado com a feature generation-pointer".into(),
                "\"store\" requiere un Meada compilado con la feature generation-pointer".into(),
            ),
            Msg::WorkspaceOwnerStatic => pick(
                "an owner belongs to the entries of a workspace version, which a static manifest \
                 has none of: use [workspace] source = \"store\" and `meada workspace import`"
                    .into(),
                "um dono pertence às entradas de uma versão do workspace, que um manifest \
                 estático não tem: use [workspace] source = \"store\" e `meada workspace import`"
                    .into(),
                "un dueño pertenece a las entradas de una versión del workspace, que un manifest \
                 estático no tiene: usa [workspace] source = \"store\" y `meada workspace import`"
                    .into(),
            ),
            Msg::WorkspaceAddedByStatic => pick(
                "an `added_by` belongs to the entries of a workspace version, which a static \
                 manifest has none of: use [workspace] source = \"store\" and `meada workspace import`"
                    .into(),
                "um `added_by` pertence às entradas de uma versão do workspace, que um manifest \
                 estático não tem: use [workspace] source = \"store\" e `meada workspace import`"
                    .into(),
                "un `added_by` pertenece a las entradas de una versión del workspace, que un \
                 manifest estático no tiene: usa [workspace] source = \"store\" y `meada \
                 workspace import`"
                    .into(),
            ),
            #[cfg(feature = "generation-pointer")]
            Msg::WorkspaceStoreBeside => pick(
                "[[repository]] entries beside [workspace] source = \"store\": the repositories \
                 come from the current workspace version"
                    .into(),
                "entradas [[repository]] ao lado de [workspace] source = \"store\": os \
                 repositórios vêm da versão atual do workspace"
                    .into(),
                "entradas [[repository]] junto a [workspace] source = \"store\": los \
                 repositorios vienen de la versión actual del workspace"
                    .into(),
            ),
            #[cfg(feature = "generation-pointer")]
            Msg::WorkspaceStoreRefused(manifest) => pick(
                format!(
                    "{manifest}: [workspace] source = \"store\": a managed workspace's nodes \
                     serve generations: run meada mcp --listen with --generations"
                ),
                format!(
                    "{manifest}: [workspace] source = \"store\": os nós de um workspace \
                     gerenciado servem gerações: rode meada mcp --listen com --generations"
                ),
                format!(
                    "{manifest}: [workspace] source = \"store\": los nodos de un workspace \
                     administrado sirven generaciones: ejecuta meada mcp --listen con --generations"
                ),
            ),
            #[cfg(feature = "generation-pointer")]
            Msg::WorkspaceNotPulled { manifest, dir } => pick(
                format!(
                    "{manifest}: [workspace] source = \"store\": no workspace version pulled \
                     in {dir}: run `meada workspace pull` first"
                ),
                format!(
                    "{manifest}: [workspace] source = \"store\": nenhuma versão do workspace \
                     baixada em {dir}: rode `meada workspace pull` antes"
                ),
                format!(
                    "{manifest}: [workspace] source = \"store\": ninguna versión del workspace \
                     descargada en {dir}: ejecuta `meada workspace pull` primero"
                ),
            ),
            #[cfg(feature = "generation-pointer")]
            Msg::WorkspaceCurrentBad(path) => pick(
                format!(
                    "{path}: not `<workspace version id> <fence>`; delete it and run `meada \
                     workspace pull`"
                ),
                format!(
                    "{path}: não é `<id da versão do workspace> <fence>`; apague-o e rode \
                     `meada workspace pull`"
                ),
                format!(
                    "{path}: no es `<id de la versión del workspace> <fence>`; elimínalo y \
                     ejecuta `meada workspace pull`"
                ),
            ),
            #[cfg(feature = "generation-pointer")]
            Msg::WorkspaceFenceBelow {
                key,
                id,
                fence,
                held,
                file,
                fence_key,
            } => pick(
                format!(
                    "{key}: workspace version {id} has fence {fence}, not above fence {held} \
                     in {file}: not swapped; raise {fence_key} with meada snapshot fence \
                     recover --at-least {held} and apply again, or delete {file}"
                ),
                format!(
                    "{key}: a versão do workspace {id} tem fence {fence}, não maior que o \
                     fence {held} em {file}: não trocada; eleve {fence_key} com meada snapshot \
                     fence recover --at-least {held} e aplique de novo, ou apague {file}"
                ),
                format!(
                    "{key}: la versión del workspace {id} tiene fence {fence}, no mayor que el \
                     fence {held} en {file}: no intercambiada; sube {fence_key} con meada \
                     snapshot fence recover --at-least {held} y aplica de nuevo, o elimina {file}"
                ),
            ),
            #[cfg(feature = "generation-pointer")]
            Msg::WorkspaceAlreadyCurrent(id) => pick(
                format!("{id}: already the current workspace version: nothing changed"),
                format!("{id}: já é a versão atual do workspace: nada mudou"),
                format!("{id}: ya es la versión actual del workspace: nada cambió"),
            ),
            #[cfg(feature = "generation-pointer")]
            Msg::WorkspacePull => pick(
                "Bring the current workspace version onto this machine, under index_dir".into(),
                "Traz a versão atual do workspace para esta máquina, sob o index_dir".into(),
                "Trae la versión actual del workspace a esta máquina, bajo el index_dir".into(),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::WorkspaceApply => pick(
                "Make a stored workspace version the current one, under the refresh lock".into(),
                "Torna atual uma versão guardada do workspace, sob o lock de refresh".into(),
                "Convierte en actual una versión guardada del workspace, bajo el lock de refresh"
                    .into(),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::WorkspaceApplyVersion => pick(
                "The workspace version id to apply".into(),
                "O id da versão do workspace a aplicar".into(),
                "El id de la versión del workspace a aplicar".into(),
            ),
            #[cfg(feature = "generation-pointer")]
            Msg::Workspace => pick(
                "Manage the workspace versions in the snapshot store".into(),
                "Gerencia as versões do workspace no store de snapshots".into(),
                "Administra las versiones del workspace en el store de snapshots".into(),
            ),
            #[cfg(feature = "generation-pointer")]
            Msg::WorkspaceImport => pick(
                "Store the [[repository]] entries of a manifest file as a workspace version and \
                 print its id"
                    .into(),
                "Guarda as entradas [[repository]] de um manifesto como uma versão do workspace \
                 e imprime seu id"
                    .into(),
                "Guarda las entradas [[repository]] de un manifiesto como una versión del \
                 workspace e imprime su id"
                    .into(),
            ),
            #[cfg(feature = "generation-pointer")]
            Msg::WorkspaceList => pick(
                "List the workspace versions, newest first, the current one marked".into(),
                "Lista as versões do workspace, da mais nova à mais antiga, marcando a atual"
                    .into(),
                "Lista las versiones del workspace, de la más nueva a la más antigua, marcando la \
                 actual"
                    .into(),
            ),
            #[cfg(feature = "generation-pointer")]
            Msg::WorkspaceShow => pick(
                "Print a workspace version as [[repository]] entries".into(),
                "Imprime uma versão do workspace como entradas [[repository]]".into(),
                "Imprime una versión del workspace como entradas [[repository]]".into(),
            ),
            #[cfg(feature = "generation-pointer")]
            Msg::WorkspaceImportFile => pick(
                "The manifest file whose [[repository]] entries are imported".into(),
                "O manifesto cujas entradas [[repository]] são importadas".into(),
                "El manifiesto cuyas entradas [[repository]] se importan".into(),
            ),
            #[cfg(feature = "generation-pointer")]
            Msg::WorkspaceImportMessage => pick(
                "Why the workspace changes".into(),
                "Por que o workspace muda".into(),
                "Por qué cambia el workspace".into(),
            ),
            #[cfg(feature = "generation-pointer")]
            Msg::WorkspaceImportAuthor => pick(
                "The version's author (cli:<login>@<host> by default)".into(),
                "O autor da versão (cli:<login>@<host> por padrão)".into(),
                "El autor de la versión (cli:<login>@<host> por defecto)".into(),
            ),
            #[cfg(feature = "generation-pointer")]
            Msg::WorkspaceShowVersion => pick(
                "The workspace version id (the current one by default)".into(),
                "O id da versão do workspace (a atual por padrão)".into(),
                "El id de la versión del workspace (la actual por defecto)".into(),
            ),
            #[cfg(feature = "generation-pointer")]
            Msg::WorkspaceShowPulled => pick(
                "Print the version meada workspace pull left on disk, without the entries the \
                 role check excluded"
                    .into(),
                "Imprime a versão que o meada workspace pull deixou no disco, sem as entradas \
                 que a checagem de papéis excluiu"
                    .into(),
                "Imprime la versión que meada workspace pull dejó en el disco, sin las entradas \
                 que la comprobación de roles excluyó"
                    .into(),
            ),
            #[cfg(feature = "generation-pointer")]
            Msg::WorkspaceBootstrapHelp => pick(
                "The bootstrap manifest, with its [snapshot] and [generations] sections".into(),
                "O manifesto de bootstrap, com as seções [snapshot] e [generations]".into(),
                "El manifiesto de bootstrap, con las secciones [snapshot] y [generations]".into(),
            ),
            #[cfg(feature = "generation-pointer")]
            Msg::WorkspaceNoSections(manifest) => pick(
                format!(
                    "{manifest}: no [generations] section: workspace versions need [snapshot] \
                     and [generations]"
                ),
                format!(
                    "{manifest}: sem seção [generations]: versões do workspace exigem [snapshot] \
                     e [generations]"
                ),
                format!(
                    "{manifest}: sin sección [generations]: las versiones del workspace requieren \
                     [snapshot] y [generations]"
                ),
            ),
            #[cfg(feature = "generation-pointer")]
            Msg::BadVersionArg(text) => pick(
                format!("{text}: not a workspace version id of 16 lowercase hex characters"),
                format!(
                    "{text}: não é um id de versão do workspace de 16 caracteres hexadecimais \
                     minúsculos"
                ),
                format!(
                    "{text}: no es un id de versión del workspace de 16 caracteres hexadecimales \
                     en minúscula"
                ),
            ),
            #[cfg(feature = "generation-pointer")]
            Msg::WorkspaceNoCurrent(key) => pick(
                format!("{key}: no workspace version is current: apply one with `meada workspace apply`"),
                format!(
                    "{key}: nenhuma versão do workspace é a atual: aplique uma com `meada \
                     workspace apply`"
                ),
                format!(
                    "{key}: ninguna versión del workspace es la actual: aplica una con `meada \
                     workspace apply`"
                ),
            ),
            #[cfg(feature = "generation-pointer")]
            Msg::WorkspaceBadPointer(key) => pick(
                format!(
                    "{key}: not a hash of `id`, a workspace version id of 16 lowercase hex \
                     characters, and `fence`, a decimal fence"
                ),
                format!(
                    "{key}: não é um hash de `id`, um id de versão do workspace de 16 caracteres \
                     hexadecimais minúsculos, e `fence`, um fence decimal"
                ),
                format!(
                    "{key}: no es un hash de `id`, un id de versión del workspace de 16 \
                     caracteres hexadecimales en minúscula, y `fence`, un fence decimal"
                ),
            ),
            #[cfg(feature = "generation-pointer")]
            Msg::WorkspaceNotVersion(key) => pick(
                format!("{key}: malformed: not the workspace version document of its id"),
                format!("{key}: malformado: não é o documento da versão do workspace do seu id"),
                format!("{key}: malformado: no es el documento de la versión del workspace de su id"),
            ),
            #[cfg(feature = "generation-pointer")]
            Msg::WorkspaceNoVersion(key) => pick(
                format!("{key}: no such workspace version in the store"),
                format!("{key}: essa versão do workspace não está no store"),
                format!("{key}: esa versión del workspace no está en el store"),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::WorkspaceRefused(n) => pick(
                format!(
                    "this change touches {n} entr{} you may not change: nothing applied",
                    if *n == 1 { "y" } else { "ies" }
                ),
                format!(
                    "esta mudança toca {n} {} que você não pode mudar: nada aplicado",
                    if *n == 1 { "entrada" } else { "entradas" }
                ),
                format!(
                    "este cambio toca {n} {} que no puedes cambiar: nada aplicado",
                    if *n == 1 { "entrada" } else { "entradas" }
                ),
            ),
            #[cfg(feature = "refresh-lock")]
            Msg::WorkspaceRoleLost => pick(
                "no group of yours owns an entry you read in the current workspace version any \
                 more: nothing applied"
                    .into(),
                "nenhum grupo seu é mais owner de uma entrada que você lê na versão atual do \
                 workspace: nada aplicado"
                    .into(),
                "ningún grupo tuyo es ya owner de una entrada que lees en la versión actual del \
                 workspace: nada aplicado"
                    .into(),
            ),
            #[cfg(feature = "generation-pointer")]
            Msg::WorkspaceVersionTooLarge { key, max } => pick(
                format!("{key}: workspace version exceeds {max} bytes: nothing stored"),
                format!("{key}: versão do workspace excede {max} bytes: nada guardado"),
                format!("{key}: la versión del workspace excede {max} bytes: nada guardado"),
            ),
            #[cfg(feature = "generation-pointer")]
            Msg::WorkspaceNotImported => pick(
                "not imported: a workspace version holds only [[repository]] entries".into(),
                "não importado: uma versão do workspace só guarda entradas [[repository]]".into(),
                "no importado: una versión del workspace solo guarda entradas [[repository]]".into(),
            ),
            #[cfg(feature = "generation-pointer")]
            Msg::WorkspaceStored {
                id,
                author,
                time,
                message,
            } => pick(
                format!("{id} is stored already, by {author} at {time}: {message}"),
                format!("{id} já está guardada, por {author} em {time}: {message}"),
                format!("{id} ya está guardada, por {author} en {time}: {message}"),
            ),
        }
    }
}

/// The `, confidence 0.80` part of a proposal line, in English, pt-BR and Spanish; empty without
/// one.
fn confidence_of(confidence: Option<f64>) -> (String, String, String) {
    match confidence {
        Some(c) => (
            format!(", confidence {c:.2}"),
            format!(", confiança {c:.2}"),
            format!(", confianza {c:.2}"),
        ),
        None => Default::default(),
    }
}

/// The half of the signing key pair a message names, in English, pt-BR and Spanish.
#[cfg(feature = "accounts")]
fn half_of(private: bool) -> (&'static str, &'static str, &'static str) {
    match private {
        true => ("private", "privada", "privada"),
        false => ("public", "pública", "pública"),
    }
}
