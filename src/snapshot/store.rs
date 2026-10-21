//! The snapshot store's client (ADR 0012): an S3-compatible bucket reached only at the endpoint
//! the `[snapshot]` section names, and at the one credential endpoint its `credentials` source
//! chose. Every request goes through `Guard`, which refuses any other origin and follows no
//! redirect. Credentials are read from that one source only, kept in memory, and scrubbed from
//! every error. `meada snapshot publish` writes through it; `meada snapshot pull` (#273) reads.

use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use object_store::aws::{AmazonS3, AmazonS3Builder, AmazonS3ConfigKey, AwsCredential};
use object_store::client::{
    HttpClient, HttpConnector, HttpError, HttpErrorKind, HttpRequest, HttpResponse, HttpService,
};
use object_store::path::Path as Key;
use object_store::{
    Attribute, Attributes, ClientOptions, GetOptions, ObjectStore, PutMultipartOptions, PutOptions,
    PutPayload, RetryConfig, StaticCredentialProvider, WriteMultipart,
};
use reqwest::Url;

use super::{Config, Credentials, Secret};
use crate::msg::Msg;

/// The object metadata every object Meada stores carries: its size and BLAKE3 hash.
const SIZE: &str = "meada-size";
const HASH: &str = "meada-blake3";
/// The most a key id, a secret, a token or a token file may hold.
const SECRET_MAX: u64 = 16 * 1024;
/// Each part of a bundle's multipart upload, and how many are in flight at once.
pub const PART: usize = 8 << 20;
const IN_FLIGHT: usize = 2;
/// The Pod Identity agent and the instance metadata service (ADR 0012, the only `http`
/// endpoints outside loopback).
const POD_AGENTS: [&str; 2] = ["169.254.170.23", "[fd00:ec2::23]"];
const METADATA: &str = "http://169.254.169.254";

/// An object's size and BLAKE3 hash, as its metadata records them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stored {
    pub size: u64,
    pub blake3: String,
}

impl Stored {
    pub fn of(bytes: &[u8]) -> Self {
        Self {
            size: bytes.len() as u64,
            blake3: blake3::hash(bytes).to_hex().to_string(),
        }
    }

    fn attributes(&self) -> Attributes {
        let mut attributes = Attributes::new();
        let size = self.size.to_string();
        attributes.insert(Attribute::Metadata(SIZE.into()), size.into());
        attributes.insert(Attribute::Metadata(HASH.into()), self.blake3.clone().into());
        attributes
    }

    /// The metadata of an object of `size` bytes, when it records that size and a hash.
    fn read(attributes: &Attributes, size: u64) -> Option<Self> {
        let get = |name: &'static str| attributes.get(&Attribute::Metadata(name.into()));
        let recorded: u64 = get(SIZE)?.as_ref().parse().ok()?;
        let blake3 = get(HASH)?.as_ref().to_string();
        let hex = blake3.len() == 64
            && blake3
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        (recorded == size && hex).then_some(Self { size, blake3 })
    }
}

/// A scheme, host and port a request may go to.
#[derive(Debug, PartialEq)]
struct Origin(String, String, u16);

impl Origin {
    fn of(url: &Url) -> Option<Self> {
        let host = url.host_str()?.to_ascii_lowercase();
        Some(Self(
            url.scheme().into(),
            host,
            url.port_or_known_default()?,
        ))
    }

    fn of_request(req: &HttpRequest) -> Option<Self> {
        let uri = req.uri();
        let scheme = uri.scheme_str()?.to_string();
        let port = uri.port_u16().or(match scheme.as_str() {
            "https" => Some(443),
            "http" => Some(80),
            _ => None,
        })?;
        Some(Self(scheme, uri.host()?.to_ascii_lowercase(), port))
    }
}

/// The transport under the client: a request to an origin the section did not configure is
/// refused before it leaves, and a redirect is never followed (nor a proxy used).
#[derive(Debug, Clone)]
struct Guard {
    client: reqwest::Client,
    allowed: Arc<Vec<Origin>>,
}

#[async_trait::async_trait]
impl HttpService for Guard {
    async fn call(&self, req: HttpRequest) -> Result<HttpResponse, HttpError> {
        let origin = Origin::of_request(&req);
        if !origin.as_ref().is_some_and(|o| self.allowed.contains(o)) {
            let to = origin.map_or_else(String::new, |Origin(s, h, p)| format!("{s}://{h}:{p}"));
            let refused = std::io::Error::other(Msg::SnapshotRefused(&to).text());
            return Err(HttpError::new(HttpErrorKind::Unknown, refused));
        }
        HttpService::call(&self.client, req).await
    }
}

impl HttpConnector for Guard {
    fn connect(&self, _: &ClientOptions) -> object_store::Result<HttpClient> {
        Ok(HttpClient::new(self.clone()))
    }
}

/// A connection to the configured bucket.
pub struct Store {
    s3: AmazonS3,
    /// What an error must never show: the secret key and the session token.
    secrets: Vec<String>,
}

impl Store {
    /// The client of `config`'s bucket, its credentials read now from the source `credentials`
    /// names, and from nowhere else.
    pub fn connect(config: &Config) -> Result<Self, String> {
        let endpoint = Url::parse(&config.endpoint).map_err(|e| e.to_string())?;
        let bucket_url = match config.path_style {
            true => endpoint,
            false => {
                let mut url = endpoint;
                let host = format!("{}.{}", config.bucket, url.host_str().unwrap_or_default());
                url.set_host(Some(&host)).map_err(|e| e.to_string())?;
                url
            }
        };
        let mut allowed: Vec<Origin> = Origin::of(&bucket_url).into_iter().collect();
        let retry = RetryConfig {
            max_retries: 2,
            retry_timeout: Duration::from_secs(30),
            ..RetryConfig::default()
        };
        let mut builder = AmazonS3Builder::new()
            .with_endpoint(bucket_url.as_str().trim_end_matches('/'))
            .with_region(&config.region)
            .with_bucket_name(&config.bucket)
            .with_virtual_hosted_style_request(!config.path_style)
            .with_allow_http(true)
            .with_retry(retry);
        let mut secrets = Vec::new();
        match &config.credentials {
            Credentials::Static {
                key_id,
                secret,
                token,
            } => {
                let credential = AwsCredential {
                    key_id: secret_value(key_id)?.ok_or_else(|| unset(key_id))?,
                    secret_key: secret_value(secret)?.ok_or_else(|| unset(secret))?,
                    // The default variable may be unset: a key pair without a session.
                    token: secret_value(token)?,
                };
                secrets.push(credential.secret_key.clone());
                secrets.extend(credential.token.clone());
                let provider = StaticCredentialProvider::new(credential);
                builder = builder.with_credentials(Arc::new(provider));
            }
            Credentials::WebIdentity => {
                let token_file = env("AWS_WEB_IDENTITY_TOKEN_FILE")?;
                read_secret(Path::new(&token_file))?;
                let sts = format!("https://sts.{}.amazonaws.com", config.region);
                allowed.extend(Url::parse(&sts).ok().as_ref().and_then(Origin::of));
                builder = builder
                    .with_config(AmazonS3ConfigKey::WebIdentityTokenFile, token_file)
                    .with_config(AmazonS3ConfigKey::RoleArn, env("AWS_ROLE_ARN")?)
                    .with_config(AmazonS3ConfigKey::StsEndpoint, sts);
            }
            Credentials::PodIdentity => {
                let uri = env("AWS_CONTAINER_CREDENTIALS_FULL_URI")?;
                let agent = pod_agent(&uri).ok_or_else(|| Msg::SnapshotPodUrl.text())?;
                allowed.extend(Origin::of(&agent));
                let token_file = env("AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE")?;
                read_secret(Path::new(&token_file))?;
                builder = builder
                    .with_config(AmazonS3ConfigKey::ContainerCredentialsFullUri, uri)
                    .with_config(
                        AmazonS3ConfigKey::ContainerAuthorizationTokenFile,
                        token_file,
                    );
            }
            // IMDSv2 only: object_store falls back to IMDSv1 only when told to.
            Credentials::Instance => {
                allowed.extend(Url::parse(METADATA).ok().as_ref().and_then(Origin::of));
                builder = builder.with_config(AmazonS3ConfigKey::MetadataEndpoint, METADATA);
            }
        }
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| e.to_string())?;
        let guard = Guard {
            client,
            allowed: Arc::new(allowed),
        };
        let store = Self {
            s3: builder
                .with_http_connector(guard)
                .build()
                .map_err(|e| e.to_string())?,
            secrets,
        };
        Ok(store)
    }

    /// `text` with no secret in it.
    pub fn scrub(&self, text: impl ToString) -> String {
        let mut text = text.to_string();
        for secret in self.secrets.iter().filter(|s| !s.is_empty()) {
            text = text.replace(secret.as_str(), "***");
        }
        text
    }

    fn key(key: &str) -> Result<Key, String> {
        Key::parse(key).map_err(|e| e.to_string())
    }

    /// The size and hash the object at `key` is stored with, `None` when there is no object
    /// there. An object without them, or whose size is not the one they record, is an error
    /// naming it.
    pub async fn stored(&self, key: &str) -> Result<Option<Stored>, String> {
        let options = GetOptions {
            head: true,
            ..GetOptions::default()
        };
        match self.s3.get_opts(&Self::key(key)?, options).await {
            Ok(got) => Stored::read(&got.attributes, got.meta.size)
                .map(Some)
                .ok_or_else(|| Msg::SnapshotObjectBad(key).text()),
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(e) => Err(self.scrub(e)),
        }
    }

    /// The object at `key`, at most `max` bytes, checked against the size and hash it is stored
    /// with; `None` when there is none.
    pub async fn get(&self, key: &str, max: u64) -> Result<Option<Vec<u8>>, String> {
        let got = match self
            .s3
            .get_opts(&Self::key(key)?, GetOptions::default())
            .await
        {
            Ok(got) => got,
            Err(object_store::Error::NotFound { .. }) => return Ok(None),
            Err(e) => return Err(self.scrub(e)),
        };
        let bad = || Msg::SnapshotObjectBad(key).text();
        let stored = Stored::read(&got.attributes, got.meta.size).ok_or_else(bad)?;
        if stored.size > max {
            return Err(bad());
        }
        let bytes = got.bytes().await.map_err(|e| self.scrub(e))?.to_vec();
        (Stored::of(&bytes) == stored)
            .then_some(Some(bytes))
            .ok_or_else(bad)
    }

    /// Stores `bytes` at `key` with their size and hash.
    pub async fn put(&self, key: &str, bytes: Vec<u8>) -> Result<(), String> {
        let options = PutOptions {
            attributes: Stored::of(&bytes).attributes(),
            ..PutOptions::default()
        };
        let payload = PutPayload::from(bytes);
        let put = self.s3.put_opts(&Self::key(key)?, payload, options).await;
        put.map(drop).map_err(|e| self.scrub(e))
    }

    /// A multipart upload to `key` of an object of the size and hash `stored` says, its parts
    /// written by the caller (at most `PART` bytes held per part, `IN_FLIGHT` parts at once).
    pub async fn upload(&self, key: &str, stored: &Stored) -> Result<Upload<'_>, String> {
        let options = PutMultipartOptions {
            attributes: stored.attributes(),
            ..PutMultipartOptions::default()
        };
        let upload = self.s3.put_multipart_opts(&Self::key(key)?, options).await;
        let upload = upload.map_err(|e| self.scrub(e))?;
        Ok(Upload {
            store: self,
            writer: WriteMultipart::new_with_chunk_size(upload, PART),
        })
    }
}

/// A multipart upload in progress: nothing is visible at its key until `finish`.
pub struct Upload<'a> {
    store: &'a Store,
    writer: WriteMultipart,
}

impl Upload<'_> {
    pub async fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        let wait = self.writer.wait_for_capacity(IN_FLIGHT).await;
        wait.map_err(|e| self.store.scrub(e))?;
        self.writer.write(bytes);
        Ok(())
    }

    pub async fn finish(self) -> Result<(), String> {
        let store = self.store;
        self.writer
            .finish()
            .await
            .map(drop)
            .map_err(|e| store.scrub(e))
    }

    pub async fn abort(self) {
        let _ = self.writer.abort().await;
    }
}

/// `uri` when it names the Pod Identity agent, at one of its two addresses, over `http` (the
/// only way the platform serves it).
fn pod_agent(uri: &str) -> Option<Url> {
    Url::parse(uri).ok().filter(|url| {
        url.scheme() == "http"
            && url.port().is_none()
            && url.username().is_empty()
            && url.password().is_none()
            && url.host_str().is_some_and(|h| POD_AGENTS.contains(&h))
    })
}

/// The variable `var`, which the credential source needs.
fn env(var: &str) -> Result<String, String> {
    std::env::var(var).map_err(|_| Msg::SnapshotEnvUnset(var).text())
}

fn unset(secret: &Secret) -> String {
    match secret {
        Secret::Env(var) => Msg::SnapshotEnvUnset(var).text(),
        Secret::File(file) => file.display().to_string(),
    }
}

/// The value of `secret`: `None` for an unset variable, an error for a file that cannot be read.
fn secret_value(secret: &Secret) -> Result<Option<String>, String> {
    match secret {
        Secret::Env(var) => Ok(std::env::var(var).ok()),
        Secret::File(file) => read_secret(file).map(Some),
    }
}

/// The content of the regular file `path`, at most `SECRET_MAX` bytes, without the editor's
/// trailing newline. A FIFO or a directory is refused before it is opened.
fn read_secret(path: &Path) -> Result<String, String> {
    let problem = |e: std::io::Error| format!("{}: {}", path.display(), e.kind());
    if !std::fs::metadata(path).map_err(problem)?.is_file() {
        return Err(format!("{}: not a regular file", path.display()));
    }
    let mut bytes = Vec::new();
    let file = std::fs::File::open(path).map_err(problem)?;
    file.take(SECRET_MAX + 1)
        .read_to_end(&mut bytes)
        .map_err(problem)?;
    if bytes.len() as u64 > SECRET_MAX {
        return Err(format!(
            "{}: larger than {SECRET_MAX} bytes",
            path.display()
        ));
    }
    let text = String::from_utf8(bytes).map_err(|_| format!("{}: not UTF-8", path.display()))?;
    Ok(text.trim_end_matches(['\n', '\r']).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_is_read_only_when_it_records_the_object_s_own_size_and_a_hash() {
        let stored = Stored::of(b"bundle");
        assert_eq!(Stored::read(&stored.attributes(), 6), Some(stored.clone()));
        assert_eq!(Stored::read(&stored.attributes(), 7), None);
        assert_eq!(Stored::read(&Attributes::new(), 6), None);
        let mut bad = stored.attributes();
        bad.insert(Attribute::Metadata(HASH.into()), "../x".into());
        assert_eq!(Stored::read(&bad, 6), None);
    }

    #[test]
    fn only_the_pod_identity_agent_is_a_pod_identity_endpoint() {
        for uri in [
            "http://169.254.170.23/v1/credentials",
            "http://[fd00:ec2::23]/v1/credentials",
        ] {
            let agent = pod_agent(uri).unwrap();
            assert_eq!(Origin::of(&agent).unwrap().2, 80, "{uri}");
        }
        for uri in [
            "https://169.254.170.23/v1/credentials",
            "http://169.254.170.23:8080/v1/credentials",
            "http://169.254.170.2/v1/credentials",
            "http://u:p@169.254.170.23/",
            "http://agent.example.com/v1/credentials",
            "http://[fd00:ec2::254]/",
            "not a url",
        ] {
            assert!(pod_agent(uri).is_none(), "{uri}");
        }
    }

    #[test]
    fn a_secret_file_is_read_only_when_regular_and_within_the_bound() {
        let dir = std::env::temp_dir().join(format!("meada-store-secret-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (small, big) = (dir.join("small"), dir.join("big"));
        std::fs::write(&small, "s3cret\n").unwrap();
        std::fs::write(&big, vec![b'x'; SECRET_MAX as usize + 1]).unwrap();
        assert_eq!(read_secret(&small).unwrap(), "s3cret");
        assert!(read_secret(&big).unwrap_err().ends_with("bytes"));
        assert!(
            read_secret(&dir)
                .unwrap_err()
                .ends_with("not a regular file")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
