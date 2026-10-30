//! The snapshot store (ADR 0012, feature `snapshot`): the manifest's `[snapshot]` section, the
//! ids of bundles and generations, and `meada snapshot publish`, which uploads the workspace as
//! a generation once every index equals its mirror and every mirror is clean. Publishing reads
//! the mirrors and the indexes and writes nothing on the machine it runs on; indexing stays
//! offline. The store client (`store`) is the one `meada snapshot pull` (`pull`) reuses.
//!
//! Store layout under `prefix`: `repos/<identity>/<bundle id>.tar` (an uncompressed tar of
//! `index.db`, then the mirror under `mirror/`, `.git` included), `generations/<generation
//! id>.json` (the `Document`) and `current` (the current generation id and a newline; with
//! `[generations]` the pointer lives in Valkey instead, `generations`), and with it the workspace
//! versions, `workspaces/<version id>.json` (`versions`). Every object carries its size and
//! BLAKE3 hash as metadata (`store::Stored`).

#[cfg(feature = "generation-pointer")]
pub mod generations;
#[cfg(feature = "generation-pointer")]
pub mod heartbeat;
#[cfg(feature = "refresh-lock")]
pub mod lock;
#[cfg(feature = "refresh-lock")]
pub mod operation;
#[cfg(all(feature = "refresh-lock", not(unix)))]
compile_error!("the refresh-lock feature is Unix only");
pub mod pull;
pub mod serve;
pub mod store;
#[cfg(feature = "generation-pointer")]
pub mod versions;

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::modules::{Problem, Secret};
use crate::msg::{Fix, Msg};
use crate::workspace::Manifest;
use crate::{Result, index, repo};
use store::{Store, Stored};

// ponytail: the most a generation document may hold in the store, or as a node holds it; past
// it the document is refused, naming its key or file.
const DOCUMENT_MAX: u64 = 16 << 20;
// ponytail: the most the store's `current` may hold (an id and a newline); past it the
// pointer is refused, naming its key.
const CURRENT_MAX: u64 = 64;

/// The `[snapshot]` section as written.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct File {
    endpoint: String,
    region: String,
    bucket: String,
    #[serde(default)]
    prefix: String,
    #[serde(default)]
    path_style: bool,
    #[serde(default)]
    credentials: Source,
    access_key_id_env: Option<String>,
    access_key_id_file: Option<String>,
    secret_access_key_env: Option<String>,
    secret_access_key_file: Option<String>,
    session_token_env: Option<String>,
    session_token_file: Option<String>,
    local_dir: Option<String>,
    node_name: Option<String>,
    ca_file: Option<String>,
}

/// Where the store's credentials come from: one source, the one the section names.
#[derive(Deserialize, Default, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum Source {
    #[default]
    Static,
    WebIdentity,
    PodIdentity,
    Instance,
}

/// The credential source, with the static keys' places. Nothing here is a secret.
#[derive(Clone, Debug, PartialEq)]
pub enum Credentials {
    Static {
        key_id: Secret,
        secret: Secret,
        /// Optional: an unset variable means no session token.
        token: Secret,
    },
    /// IRSA: `AWS_WEB_IDENTITY_TOKEN_FILE` and `AWS_ROLE_ARN`, exchanged with STS in `region`.
    WebIdentity,
    /// EKS Pod Identity: `AWS_CONTAINER_CREDENTIALS_FULL_URI` (the agent only) and the token
    /// in `AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE`.
    PodIdentity,
    /// The instance metadata service, IMDSv2.
    Instance,
}

/// The validated section.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// An `https` URL, or `http` on loopback, with no user, path, query or fragment.
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    /// Empty, or `/`-separated segments.
    pub prefix: String,
    pub path_style: bool,
    pub credentials: Credentials,
    /// Where nodes keep the generations they pull (#273, #274).
    pub local_dir: Option<PathBuf>,
    /// The name of this node's heartbeat (#298), checked against the node name pattern; the
    /// host name in lowercase when unset.
    pub node_name: Option<String>,
    /// Additional PEM trust roots for a private-CA store, relative to the manifest.
    pub ca_file: Option<PathBuf>,
}

impl File {
    /// The section checked; relative paths are relative to `base`, the manifest's directory.
    pub fn validate(self, base: &Path) -> std::result::Result<Config, Problem> {
        let virtual_host = endpoint(&self.endpoint).map_err(|m| ("snapshot.endpoint", m))?;
        if !self.path_style && !virtual_host {
            return Err(("snapshot.path_style", Msg::SnapshotPathStyle.text()));
        }
        let lower = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit();
        if self.region.is_empty() || !self.region.chars().all(|c| lower(c) || c == '-') {
            return Err(("snapshot.region", Msg::SnapshotRegion.text()));
        }
        let bucket = &self.bucket;
        let inner = bucket.chars().all(|c| lower(c) || c == '-' || c == '.');
        let ends = bucket.starts_with(lower) && bucket.ends_with(lower);
        if !(3..=63).contains(&bucket.len()) || !inner || !ends {
            return Err(("snapshot.bucket", Msg::SnapshotBucket.text()));
        }
        let prefix = &self.prefix;
        if !prefix.is_empty()
            && prefix
                .split('/')
                .any(|s| matches!(s, "" | "." | "..") || s.contains(['\\', '*', '?']))
        {
            return Err(("snapshot.prefix", Msg::SnapshotPrefix.text()));
        }
        if (self.node_name.as_deref()).is_some_and(|name| !is_node_name(name)) {
            return Err(("snapshot.node_name", Msg::SnapshotNodeName.text()));
        }
        let pairs = [
            (
                "snapshot.access_key_id_file",
                "access_key_id_env",
                self.access_key_id_env,
                self.access_key_id_file,
                "AWS_ACCESS_KEY_ID",
            ),
            (
                "snapshot.secret_access_key_file",
                "secret_access_key_env",
                self.secret_access_key_env,
                self.secret_access_key_file,
                "AWS_SECRET_ACCESS_KEY",
            ),
            (
                "snapshot.session_token_file",
                "session_token_env",
                self.session_token_env,
                self.session_token_file,
                "AWS_SESSION_TOKEN",
            ),
        ];
        let mut secrets = Vec::new();
        for (key, other, env, file, default) in pairs {
            let named = env.is_some() || file.is_some();
            if named && self.credentials != Source::Static {
                let key = match env {
                    Some(_) => match key {
                        "snapshot.access_key_id_file" => "snapshot.access_key_id_env",
                        "snapshot.secret_access_key_file" => "snapshot.secret_access_key_env",
                        _ => "snapshot.session_token_env",
                    },
                    None => key,
                };
                return Err((key, Msg::SnapshotStaticOnly.text()));
            }
            let both = || (key, Msg::SnapshotBoth(other).text());
            let named = Secret::named(env, file, base, both)?;
            secrets.push(named.unwrap_or_else(|| Secret::Env(default.into())));
        }
        let credentials = match self.credentials {
            Source::Static => {
                let [key_id, secret, token]: [Secret; 3] = secrets.try_into().expect("three pairs");
                Credentials::Static {
                    key_id,
                    secret,
                    token,
                }
            }
            Source::WebIdentity => Credentials::WebIdentity,
            Source::PodIdentity => Credentials::PodIdentity,
            Source::Instance => Credentials::Instance,
        };
        Ok(Config {
            endpoint: self.endpoint,
            region: self.region,
            bucket: self.bucket,
            prefix: self.prefix,
            path_style: self.path_style,
            credentials,
            local_dir: self.local_dir.map(|d| base.join(d)),
            node_name: self.node_name,
            ca_file: self.ca_file.map(|file| base.join(file)),
        })
    }
}

/// Whether `name` is a node name: lowercase letters, digits, `.` and `-`, 1 to 63 characters.
pub(crate) fn is_node_name(name: &str) -> bool {
    (1..=63).contains(&name.len())
        && (name.bytes()).all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'-'))
}

/// Why `url` is no store endpoint: an `https` URL, or `http` when the host is loopback
/// (`localhost`, `127.0.0.0/8`, `::1`), with no user, password, path, query or fragment. When it
/// is one, whether a bucket can go in front of its host (virtual-hosted style): an `https`
/// endpoint named by a domain other than `localhost`. `<bucket>.localhost` may go to DNS, an IP
/// takes no subdomain, and a plain `http` endpoint is loopback, so path style.
fn endpoint(url: &str) -> std::result::Result<bool, String> {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return Err(Msg::SnapshotEndpointInvalid.text());
    };
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(Msg::SnapshotEndpointUser.text());
    }
    let bare = parsed.path() == "/" && parsed.query().is_none() && parsed.fragment().is_none();
    if !bare || parsed.host().is_none() {
        return Err(Msg::SnapshotEndpointInvalid.text());
    }
    let host = parsed.host_str().unwrap_or_default();
    let ip = host.trim_start_matches('[').trim_end_matches(']');
    let named = host != "localhost" && ip.parse::<std::net::IpAddr>().is_err();
    match parsed.scheme() {
        "https" => Ok(named),
        "http" if crate::modules::loopback(host) => Ok(false),
        "http" => Err(Msg::SnapshotEndpointPlain.text()),
        _ => Err(Msg::SnapshotEndpointInvalid.text()),
    }
}

impl Config {
    /// `name` under the section's `prefix`.
    pub fn key(&self, name: &str) -> String {
        match self.prefix.is_empty() {
            true => name.to_string(),
            false => format!("{}/{name}", self.prefix),
        }
    }
}

// The ids of ADR 0012, their shape, the fence and the entries' canonical blocks are
// `meada_core::ids`'s.
pub use meada_core::ids::{bundle_id, entry_blocks, fence, generation_id, is_id};

/// A generation document (`generations/<id>.json`), never rewritten once stored.
#[derive(Serialize, Deserialize)]
pub struct Document {
    pub generation: String,
    /// The workspace version that produced the generation (`source = "store"`, ADR 0014),
    /// absent with `source = "file"`. Not part of any id; a document stored keeps the version
    /// that first produced it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    /// In manifest order.
    pub repositories: Vec<Entry>,
}

/// One repository of a generation.
#[derive(Serialize, Deserialize)]
pub struct Entry {
    pub identity: String,
    pub head: String,
    /// The bundle's key in the store.
    pub bundle: String,
    /// The entry's blocks, as `entry_blocks` gives them.
    pub blocks: serde_json::Value,
    /// The stored bundle's size and BLAKE3 hash, read back from its metadata.
    pub size: u64,
    pub blake3: String,
}

/// A repository ready to publish.
struct Bundle {
    identity: String,
    head: String,
    key: String,
    blocks: serde_json::Value,
    mirror: PathBuf,
    index: PathBuf,
    /// The index opened by `index::open_read`, its read transaction held until the bundle is
    /// uploaded: a `meada index` commit waits for it (up to its busy timeout), so both passes
    /// over the tar read the index the freshness check read.
    _read: rusqlite::Connection,
}

/// `meada snapshot publish --workspace <manifest>`: the generation id, once the generation is
/// stored and the pointer names it (the store's `current`, or Valkey's with `[generations]`).
/// It refuses, listing every repository, any index that differs from its mirror
/// (`index::fresh`), is missing or is of another schema, and any mirror that is not clean; a
/// failure before the last write leaves the pointer as it was.
pub fn publish(manifest: &Path) -> Result<String> {
    let listed = Manifest::read_listed(manifest)?;
    let shown = manifest.display().to_string();
    let Some(config) = listed.snapshot.clone() else {
        return Err(Msg::SnapshotNoSection(&shown).text().into());
    };
    // With `[refresh_lock]`, only under `meada snapshot lock`: its token and fence.
    #[cfg(feature = "refresh-lock")]
    let held = match listed.refresh_lock {
        Some(_) => Some(lock::from_env()?),
        None => None,
    };
    let mut bundles = Vec::new();
    let mut problems = Vec::new();
    for entry in &listed.entries {
        let r = match &entry.repository {
            Ok(r) => r,
            Err(problem) => {
                problems.push(format!("{}: {problem}", entry.repo));
                continue;
            }
        };
        let repo_id = r.identity.as_str();
        let checked = (|| -> Result<std::result::Result<rusqlite::Connection, String>> {
            let db = index::open_read(&r.index, repo_id, Fix::Index(&shown))?;
            if !index::fresh(&db, &r.root, repo_id, &r.config.serve)? {
                let manifest = shown.as_str();
                return Ok(Err(Msg::SnapshotBehind {
                    repo: repo_id,
                    manifest,
                }
                .text()));
            }
            if !repo::unclean(&r.root, true)?.is_empty() {
                let mirror = r.root.display().to_string();
                return Ok(Err(Msg::SnapshotDirty {
                    repo: repo_id,
                    mirror: &mirror,
                }
                .text()));
            }
            Ok(Ok(db))
        })();
        let read = match checked {
            Ok(Ok(db)) => db,
            Ok(Err(problem)) => {
                problems.push(problem);
                continue;
            }
            // `open_read`'s errors already name the repository.
            Err(e) => {
                let e = e.to_string();
                problems.push(match e.starts_with(&format!("{repo_id}: ")) {
                    true => e,
                    false => format!("{repo_id}: {e}"),
                });
                continue;
            }
        };
        let head = match repo::head(&r.root) {
            Ok(head) => head,
            Err(e) => {
                problems.push(format!("{repo_id}: {e}"));
                continue;
            }
        };
        let blocks = serde_json::to_string(&entry.blocks)?;
        let binary = env!("CARGO_PKG_VERSION");
        let bundle = bundle_id(repo_id, &head, index::schema_version(), binary, &blocks);
        bundles.push(Bundle {
            key: config.key(&format!("repos/{repo_id}/{bundle}.tar")),
            identity: r.identity.clone(),
            head,
            blocks: entry.blocks.clone(),
            mirror: r.root.clone(),
            index: r.index.clone(),
            _read: read,
        });
    }
    // One problem per line; the CLI prefixes each.
    if !problems.is_empty() {
        return Err(problems.join("\n").into());
    }
    let ids: Vec<(&str, &str)> = (bundles.iter())
        .map(|b| (b.identity.as_str(), bundle_of(&b.key)))
        .collect();
    let generation = generation_id(&ids);
    let pointer = Pointer {
        #[cfg(feature = "generation-pointer")]
        generations: (listed.generations.clone()).zip(listed.valkey.clone()),
        #[cfg(feature = "refresh-lock")]
        held,
    };
    // The pulled workspace version with `source = "store"`; never part of an id.
    #[cfg(feature = "generation-pointer")]
    let workspace = listed.version.clone();
    #[cfg(not(feature = "generation-pointer"))]
    let workspace = None;
    let stored = store_generation(&config, &generation, workspace, bundles, pointer);
    crate::modules::block_on(stored)??;
    Ok(generation)
}

/// The bundle id of a bundle key, `<...>/<bundle id>.tar`.
pub(crate) fn bundle_of(key: &str) -> &str {
    let name = key.rsplit('/').next().unwrap_or(key);
    name.strip_suffix(".tar").unwrap_or(name)
}

/// Where `publish` writes the pointer: the store's `current`, or with `[generations]` the keys
/// of the generation pointer in Valkey (`generations::point`).
struct Pointer {
    #[cfg(feature = "generation-pointer")]
    generations: Option<(generations::Config, crate::valkey::Config)>,
    /// The refresh lock's token and fence (`[refresh_lock]`): checked before each upload and
    /// handed to the pointer script.
    #[cfg(feature = "refresh-lock")]
    held: Option<lock::Held>,
}

/// Stores the generation `generation` of `bundles`, its document naming the workspace version
/// `workspace` when one produced it, then points `pointer` at it. A document already stored is
/// verified and kept as it is, with the version that first produced it.
async fn store_generation(
    config: &Config,
    generation: &str,
    workspace: Option<String>,
    bundles: Vec<Bundle>,
    #[cfg_attr(not(feature = "generation-pointer"), allow(unused_variables))] pointer: Pointer,
) -> std::result::Result<(), String> {
    let store = Store::connect(config)?;
    // Before any upload: a Valkey that cannot be reached costs no transfer.
    #[cfg(feature = "generation-pointer")]
    let valkey = match &pointer.generations {
        Some((_, valkey)) => Some(crate::valkey::Valkey::connect(valkey).await?),
        None => None,
    };
    // With the refresh lock: an error unless the lock still holds this job's token, checked
    // before each upload, so a job that lost it uploads nothing more.
    #[cfg(feature = "refresh-lock")]
    let guard = async || match (&pointer.held, &valkey, &pointer.generations) {
        (Some(held), Some(valkey), Some((_, config))) => {
            lock::holds(valkey, config, &held.token).await
        }
        (None, ..) => Ok(()),
        // `Manifest::read` refuses `[refresh_lock]` without `[generations]`: never fail open.
        (Some(_), ..) => Err(Msg::NeedsSections {
            section: "refresh_lock",
            needed: &["generations"],
        }
        .text()),
    };
    let document = config.key(&format!("generations/{generation}.json"));
    match store.get(&document, DOCUMENT_MAX).await? {
        Some(bytes) => verify(&store, &document, &bytes, generation, &bundles).await?,
        None => {
            let mut repositories = Vec::new();
            for bundle in bundles {
                let stored = match store.stored(&bundle.key).await? {
                    Some(stored) => stored,
                    None => {
                        #[cfg(feature = "refresh-lock")]
                        guard().await?;
                        upload(&store, &bundle).await?;
                        let stored = store.stored(&bundle.key).await?;
                        stored.ok_or_else(|| Msg::SnapshotObjectBad(&bundle.key).text())?
                    }
                };
                repositories.push(Entry {
                    identity: bundle.identity,
                    head: bundle.head,
                    bundle: bundle.key,
                    blocks: bundle.blocks,
                    size: stored.size,
                    blake3: stored.blake3,
                });
            }
            let document_text = Document {
                generation: generation.to_string(),
                workspace,
                repositories,
            };
            let mut text = serde_json::to_vec_pretty(&document_text).map_err(|e| e.to_string())?;
            text.push(b'\n');
            #[cfg(feature = "refresh-lock")]
            guard().await?;
            store.put(&document, text).await?;
        }
    }
    #[cfg(feature = "generation-pointer")]
    if let (Some((section, config)), Some(valkey)) = (&pointer.generations, &valkey) {
        let keep = section.keep;
        #[cfg(feature = "refresh-lock")]
        let (token, fence) = match &pointer.held {
            Some(held) => (Some(held.token.as_str()), held.fence),
            None => (None, 0),
        };
        #[cfg(not(feature = "refresh-lock"))]
        let (token, fence) = (None, 0);
        return (generations::point(valkey, config, keep, generation, token, fence).await)
            .map_err(Into::into);
    }
    let current = config.key("current");
    let pointer = format!("{generation}\n").into_bytes();
    if store.get(&current, CURRENT_MAX).await?.as_ref() != Some(&pointer) {
        store.put(&current, pointer).await?;
    }
    Ok(())
}

/// A stored generation document checked: it is the generation `generation` of `bundles`, and
/// every bundle it lists is stored with the size and hash it records.
async fn verify(
    store: &Store,
    key: &str,
    bytes: &[u8],
    generation: &str,
    bundles: &[Bundle],
) -> std::result::Result<(), String> {
    let bad = |key: &str| Msg::SnapshotObjectBad(key).text();
    let document: Document = serde_json::from_slice(bytes).map_err(|_| bad(key))?;
    let mut listed: Vec<&str> = (document.repositories.iter())
        .map(|e| e.bundle.as_str())
        .collect();
    let mut expected: Vec<&str> = bundles.iter().map(|b| b.key.as_str()).collect();
    listed.sort();
    expected.sort();
    if document.generation != generation || listed != expected {
        return Err(bad(key));
    }
    for entry in &document.repositories {
        let recorded = Stored {
            size: entry.size,
            blake3: entry.blake3.clone(),
        };
        if store.stored(&entry.bundle).await? != Some(recorded) {
            return Err(bad(&entry.bundle));
        }
    }
    Ok(())
}

/// Counts and hashes what goes through it.
struct Counted<W> {
    inner: W,
    hash: blake3::Hasher,
    size: u64,
}

impl<W: Write> Write for Counted<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.hash.update(&buf[..n]);
        self.size += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Sends what is written to it in chunks of `store::PART` bytes.
struct Chunks {
    tx: tokio::sync::mpsc::Sender<Vec<u8>>,
    buf: Vec<u8>,
}

impl Chunks {
    fn send(&mut self) -> io::Result<()> {
        let chunk = std::mem::take(&mut self.buf);
        let sent = self.tx.blocking_send(chunk);
        sent.map_err(|_| io::Error::other("the upload stopped"))
    }
}

impl Write for Chunks {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(buf);
        if self.buf.len() >= store::PART {
            self.send()?;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        match self.buf.is_empty() {
            true => Ok(()),
            false => self.send(),
        }
    }
}

/// The bundle's tar written to `out`: `index.db`, then the mirror under `mirror/` in path
/// order, symbolic links as links. Its size and BLAKE3 hash.
fn write_bundle<W: Write>(mirror: &Path, index: &Path, out: W) -> io::Result<Stored> {
    let counted = Counted {
        inner: out,
        hash: blake3::Hasher::new(),
        size: 0,
    };
    let mut tar = tar::Builder::new(counted);
    tar.follow_symlinks(false);
    tar.append_path_with_name(index, "index.db")?;
    append_tree(&mut tar, mirror, Path::new("mirror"))?;
    let mut counted = tar.into_inner()?;
    counted.flush()?;
    Ok(Stored {
        size: counted.size,
        blake3: counted.hash.finalize().to_hex().to_string(),
    })
}

fn append_tree<W: Write>(tar: &mut tar::Builder<W>, dir: &Path, name: &Path) -> io::Result<()> {
    tar.append_dir(name, dir)?;
    let mut entries = std::fs::read_dir(dir)?.collect::<io::Result<Vec<_>>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let (path, name) = (entry.path(), name.join(entry.file_name()));
        match entry.file_type()?.is_dir() {
            true => append_tree(tar, &path, &name)?,
            false => tar.append_path_with_name(&path, &name)?,
        }
    }
    Ok(())
}

/// Uploads `bundle`'s tar with the size and hash a first pass over it measured; a second pass
/// streams it. A mirror or an index that changed between the passes aborts the upload.
async fn upload(store: &Store, bundle: &Bundle) -> std::result::Result<(), String> {
    let (mirror, index) = (bundle.mirror.clone(), bundle.index.clone());
    let measure = tokio::task::spawn_blocking(move || write_bundle(&mirror, &index, io::sink()));
    let measured = measure.await.map_err(|e| e.to_string())?;
    let measured = measured.map_err(|e| format!("{}: {e}", bundle.identity))?;
    let mut upload = store.upload(&bundle.key, &measured).await?;
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let (mirror, index) = (bundle.mirror.clone(), bundle.index.clone());
    let produce = tokio::task::spawn_blocking(move || {
        let chunks = Chunks {
            tx,
            buf: Vec::with_capacity(store::PART),
        };
        write_bundle(&mirror, &index, chunks)
    });
    let mut sent = Ok(());
    while let Some(chunk) = rx.recv().await {
        sent = upload.write(&chunk).await;
        if sent.is_err() {
            break;
        }
    }
    drop(rx);
    let produced = produce.await.map_err(|e| e.to_string())?;
    let same = produced.as_ref().is_ok_and(|p| *p == measured);
    match (sent, produced) {
        (Err(e), _) => {
            upload.abort().await;
            Err(e)
        }
        (_, Err(e)) => {
            upload.abort().await;
            Err(format!("{}: {e}", bundle.identity))
        }
        _ if !same => {
            upload.abort().await;
            Err(Msg::SnapshotChanged(&bundle.identity).text())
        }
        _ => upload.finish().await,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn section(toml: &str) -> std::result::Result<Config, Problem> {
        let file: File = toml::from_str(toml).unwrap();
        file.validate(Path::new("/etc/meada"))
    }

    const REQUIRED: &str = "endpoint = \"https://s3.us-east-1.amazonaws.com\"\n\
                            region = \"us-east-1\"\nbucket = \"meada-store\"\n";

    #[test]
    fn the_section_takes_its_defaults_and_resolves_paths_from_the_manifest() {
        let config = section(REQUIRED).unwrap();
        assert_eq!(config.prefix, "");
        assert!(!config.path_style);
        assert_eq!(config.local_dir, None);
        assert_eq!(config.node_name, None);
        assert_eq!(config.ca_file, None);
        let env = |v: &str| Secret::Env(v.into());
        assert_eq!(
            config.credentials,
            Credentials::Static {
                key_id: env("AWS_ACCESS_KEY_ID"),
                secret: env("AWS_SECRET_ACCESS_KEY"),
                token: env("AWS_SESSION_TOKEN"),
            }
        );
        assert_eq!(config.key("current"), "current");

        let config = section(&format!(
            "{REQUIRED}prefix = \"prod/a\"\npath_style = true\naccess_key_id_env = \"K\"\n\
             secret_access_key_file = \"secret\"\nsession_token_file = \"/run/token\"\n\
             local_dir = \"generations\"\nnode_name = \"meada-node-0\"\nca_file = \"ca.pem\""
        ))
        .unwrap();
        assert_eq!(config.node_name.as_deref(), Some("meada-node-0"));
        assert_eq!(config.key("current"), "prod/a/current");
        assert!(config.path_style);
        assert_eq!(config.local_dir, Some("/etc/meada/generations".into()));
        assert_eq!(config.ca_file, Some("/etc/meada/ca.pem".into()));
        assert_eq!(
            config.credentials,
            Credentials::Static {
                key_id: env("K"),
                secret: Secret::File("/etc/meada/secret".into()),
                token: Secret::File("/run/token".into()),
            }
        );
        for (source, credentials) in [
            ("web-identity", Credentials::WebIdentity),
            ("pod-identity", Credentials::PodIdentity),
            ("instance", Credentials::Instance),
        ] {
            let toml = format!("{REQUIRED}credentials = \"{source}\"");
            assert_eq!(section(&toml).unwrap().credentials, credentials);
        }
    }

    #[test]
    fn a_node_name_is_lowercase_letters_digits_dots_and_dashes() {
        for good in ["a", "meada-node-0", "node.example.com", &"a".repeat(63)] {
            assert!(is_node_name(good), "{good}");
        }
        for bad in ["", "Upper", "a_b", "a b", "../x", "a:b", &"a".repeat(64)] {
            assert!(!is_node_name(bad), "{bad}");
        }
    }

    #[test]
    fn a_bad_section_names_the_key() {
        let with = |endpoint: &str| {
            format!("endpoint = \"{endpoint}\"\nregion = \"us-east-1\"\nbucket = \"b-1\"\n")
        };
        let endpoint = "snapshot.endpoint";
        for (toml, key) in [
            (with("http://s3.example.internal"), endpoint),
            (with("http://10.0.0.1:9000"), endpoint),
            (with("https://user:pw@s3.example.internal"), endpoint),
            (with("https://s3.example.internal/bucket"), endpoint),
            (with("https://s3.example.internal/?a=b"), endpoint),
            (with("ftp://s3.example.internal"), endpoint),
            (with("s3.example.internal"), endpoint),
            (
                REQUIRED.replace("us-east-1\"", "US East\""),
                "snapshot.region",
            ),
            (REQUIRED.replace("meada-store", "Meada"), "snapshot.bucket"),
            (REQUIRED.replace("meada-store", "ab"), "snapshot.bucket"),
            (REQUIRED.replace("meada-store", "-ab"), "snapshot.bucket"),
            (
                format!("{REQUIRED}node_name = \"Upper\""),
                "snapshot.node_name",
            ),
            (
                format!("{REQUIRED}node_name = \"a_b\""),
                "snapshot.node_name",
            ),
            (format!("{REQUIRED}node_name = \"\""), "snapshot.node_name"),
            (
                format!("{REQUIRED}node_name = \"{}\"", "a".repeat(64)),
                "snapshot.node_name",
            ),
            (format!("{REQUIRED}prefix = \"/prod\""), "snapshot.prefix"),
            (format!("{REQUIRED}prefix = \"a/../b\""), "snapshot.prefix"),
            (format!("{REQUIRED}prefix = \"a//b\""), "snapshot.prefix"),
            (
                format!("{REQUIRED}access_key_id_env = \"K\"\naccess_key_id_file = \"k\""),
                "snapshot.access_key_id_file",
            ),
            (
                format!("{REQUIRED}credentials = \"instance\"\nsecret_access_key_env = \"S\""),
                "snapshot.secret_access_key_env",
            ),
            (
                format!("{REQUIRED}credentials = \"web-identity\"\nsession_token_file = \"t\""),
                "snapshot.session_token_file",
            ),
        ] {
            let (found, message) = section(&toml).unwrap_err();
            assert_eq!(found, key, "{toml}");
            assert!(!message.contains("pw"), "{message}");
        }
        // Loopback may stay plain, in any of its forms.
        for host in [
            "localhost:9000",
            "127.0.0.1",
            "127.9.9.9:9000",
            "[::1]:9000",
        ] {
            let plain = with(&format!("http://{host}"));
            let (found, _) = section(&plain).unwrap_err();
            assert_eq!(found, "snapshot.path_style", "{host}");
            assert!(
                section(&format!("{plain}path_style = true\n")).is_ok(),
                "{host}"
            );
        }
        // The bucket cannot go in front of an IP or `localhost` either.
        for endpoint in [
            "https://10.0.0.1",
            "https://[::1]:9000",
            "https://localhost",
        ] {
            let (found, _) = section(&with(endpoint)).unwrap_err();
            assert_eq!(found, "snapshot.path_style", "{endpoint}");
        }
        assert!(section(&with("https://s3.example.internal:8443/")).is_ok());
    }
}
