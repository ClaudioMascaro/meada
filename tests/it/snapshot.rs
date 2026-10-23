//! The snapshot store (ADR 0012, spec #259, feature `snapshot`): `meada snapshot publish
//! --workspace` over the chain's mirrors and `meada snapshot pull --workspace` on a node's
//! manifest elsewhere, against the local S3-compatible server
//! `MEADA_TEST_S3_ENDPOINT` names (path style). Without it the store tests print why and skip;
//! with `MEADA_TEST_SERVICES_REQUIRED=1` they fail.
#![cfg(feature = "snapshot")]

mod tls;

use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use object_store::aws::{AmazonS3, AmazonS3Builder};
use object_store::path::Path as Key;
use object_store::signer::Signer;
use object_store::{
    Attribute, Attributes, GetOptions, ObjectMeta, ObjectStore, ObjectStoreExt, PutOptions,
};

use crate::common::git;
use crate::manifest::{CHAIN, MANIFEST, chain_manifest, run};

pub(crate) const KEY_ID: &str = "meada-test";
pub(crate) const SECRET: &str = "meada-test-secret";

/// The local store, or `None` with the reason printed.
pub(crate) fn service() -> Option<String> {
    let url = std::env::var("MEADA_TEST_S3_ENDPOINT").ok();
    if url.is_none() {
        let required = std::env::var("MEADA_TEST_SERVICES_REQUIRED").is_ok_and(|v| v == "1");
        assert!(!required, "MEADA_TEST_S3_ENDPOINT is not set");
        eprintln!("skipped: MEADA_TEST_S3_ENDPOINT is not set");
    }
    url
}

pub(crate) fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build();
    runtime.unwrap().block_on(future)
}

/// A client of `bucket` as the tests see the store, the bucket created.
pub(crate) fn bucket(endpoint: &str, bucket: &str) -> AmazonS3 {
    crate::common::crypto_provider();
    let s3 = AmazonS3Builder::new()
        .with_endpoint(endpoint)
        .with_region("us-east-1")
        .with_bucket_name(bucket)
        .with_access_key_id(KEY_ID)
        .with_secret_access_key(SECRET)
        .with_allow_http(true)
        .build()
        .unwrap();
    // A presigned PUT of the bucket's own URL creates it.
    let url = block_on(s3.signed_url(
        reqwest::Method::PUT,
        &Key::from(""),
        Duration::from_secs(60),
    ));
    let agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build();
    let answer = ureq::Agent::new_with_config(agent)
        .put(url.unwrap().as_str())
        .send_empty();
    let status = answer.unwrap().status().as_u16();
    assert!(
        status == 200 || status == 409,
        "create bucket {bucket}: {status}"
    );
    s3
}

/// A bucket name of this test's own.
pub(crate) fn bucket_name(case: &str) -> String {
    format!("meada-{case}-{}", std::process::id()).replace('_', "-")
}

/// The chain's manifest in `<case>/` with a `[snapshot]` section for `bucket`, indexed. The
/// secret key comes from a file, the key id from a variable `publish` sets.
pub(crate) fn store_manifest(case: &str, endpoint: &str, bucket: &str, secret: &str) -> PathBuf {
    let dir = chain_manifest(case);
    let secret_file = dir.join("ops/s3-secret");
    fs::write(&secret_file, format!("{secret}\n")).unwrap();
    append_section(&dir, &section(endpoint, bucket));
    assert!(run(&dir, "index").2, "index");
    dir
}

pub(crate) fn section(endpoint: &str, bucket: &str) -> String {
    format!(
        "\n[snapshot]\nendpoint = \"{endpoint}\"\nregion = \"us-east-1\"\nbucket = \"{bucket}\"\n\
         prefix = \"prod/meada\"\npath_style = true\naccess_key_id_env = \"MEADA_TEST_S3_KEY\"\n\
         secret_access_key_file = \"s3-secret\"\nsession_token_env = \"MEADA_TEST_S3_TOKEN\"\n"
    )
}

pub(crate) fn append_section(dir: &Path, section: &str) {
    let manifest = dir.join(MANIFEST);
    let toml = fs::read_to_string(&manifest).unwrap() + section;
    fs::write(&manifest, toml).unwrap();
}

/// `meada snapshot publish --workspace <manifest>` in `dir`: stdout, stderr and the exit code.
pub(crate) fn publish(dir: &Path, manifest: &str) -> (String, String, Option<i32>) {
    let out = Command::new(env!("CARGO_BIN_EXE_meada"))
        .current_dir(dir)
        .args(["snapshot", "publish", "--workspace", manifest])
        .env("LANG", "en_US.UTF-8")
        .env("MEADA_TEST_S3_KEY", KEY_ID)
        .env_remove("MEADA_TEST_S3_TOKEN")
        .output()
        .expect("meada runs");
    let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
    (text(&out.stdout), text(&out.stderr), out.status.code())
}

/// Every object under `prefix`, by key.
pub(crate) fn objects(s3: &AmazonS3, prefix: &str) -> Vec<ObjectMeta> {
    block_on(async {
        let mut all = Vec::new();
        let mut pending = vec![Key::from(prefix)];
        while let Some(at) = pending.pop() {
            let listed = s3.list_with_delimiter(Some(&at)).await.unwrap();
            all.extend(listed.objects);
            pending.extend(listed.common_prefixes);
        }
        all.sort_by(|a, b| a.location.cmp(&b.location));
        all
    })
}

pub(crate) fn get(s3: &AmazonS3, key: &str) -> Vec<u8> {
    block_on(async {
        s3.get(&Key::from(key))
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap()
            .to_vec()
    })
}

pub(crate) fn put(s3: &AmazonS3, key: &str, bytes: &[u8]) {
    block_on(s3.put(&Key::from(key), bytes.to_vec().into())).unwrap();
}

/// The `meada-size` and `meada-blake3` metadata of the object at `key`.
pub(crate) fn metadata(s3: &AmazonS3, key: &str) -> (u64, String) {
    let options = GetOptions {
        head: true,
        ..GetOptions::default()
    };
    let got = block_on(s3.get_opts(&Key::from(key), options)).unwrap();
    let meta = |name: &'static str| {
        let value = got.attributes.get(&Attribute::Metadata(name.into()));
        value.unwrap().as_ref().to_string()
    };
    (meta("meada-size").parse().unwrap(), meta("meada-blake3"))
}

/// Every file and directory under `dirs`, with its bytes and mtime, sorted.
fn trees(dirs: &[PathBuf]) -> Vec<(PathBuf, Vec<u8>, SystemTime)> {
    let mut all = Vec::new();
    let mut pending = dirs.to_vec();
    while let Some(at) = pending.pop() {
        let meta = fs::symlink_metadata(&at).unwrap();
        let bytes = match meta.is_file() {
            true => fs::read(&at).unwrap(),
            false => Vec::new(),
        };
        all.push((at.clone(), bytes, meta.modified().unwrap()));
        if meta.is_dir() {
            pending.extend(fs::read_dir(&at).unwrap().map(|e| e.unwrap().path()));
        }
    }
    all.sort();
    all
}

/// The chain's mirrors and every `index_dir` under `dir`.
fn local(dir: &Path, index_dirs: &[&str]) -> Vec<PathBuf> {
    let mirrors = CHAIN.iter().map(|(_, path)| dir.join(path));
    mirrors
        .chain(index_dirs.iter().map(|d| dir.join(d)))
        .collect()
}

fn is_id(text: &str) -> bool {
    text.len() == 16 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[test]
fn publish_stores_a_generation_and_a_republish_or_a_second_machine_uploads_nothing() {
    let Some(endpoint) = service() else { return };
    let name = bucket_name("snapshot-publish");
    let s3 = bucket(&endpoint, &name);
    let dir = store_manifest("snapshot_publish", &endpoint, &name, SECRET);
    let before = trees(&local(&dir, &["indexes"]));

    let (stdout, stderr, code) = publish(&dir, MANIFEST);
    assert_eq!(code, Some(0), "{stderr}");
    let generation = stdout.strip_suffix('\n').unwrap().to_string();
    assert!(is_id(&generation) && !generation.contains('\n'), "{stdout}");
    // Publishing wrote nothing into a mirror or under index_dir, mtimes included.
    assert_eq!(trees(&local(&dir, &["indexes"])), before);

    let document_key = format!("prod/meada/generations/{generation}.json");
    let keys: Vec<String> = objects(&s3, "prod/meada")
        .iter()
        .map(|o| o.location.to_string())
        .collect();
    let document: serde_json::Value = serde_json::from_slice(&get(&s3, &document_key)).unwrap();
    assert_eq!(document["generation"], generation.as_str());
    let repositories = document["repositories"].as_array().unwrap();
    let mut expected = vec![document_key.clone(), "prod/meada/current".to_string()];
    for ((identity, path), entry) in CHAIN.iter().zip(repositories) {
        assert_eq!(entry["identity"], *identity);
        let head = git(&dir.join(path), &["rev-parse", "HEAD"]);
        assert_eq!(entry["head"], head.trim());
        let bundle = entry["bundle"].as_str().unwrap();
        let id = (bundle.strip_prefix(&format!("prod/meada/repos/{identity}/")))
            .and_then(|name| name.strip_suffix(".tar"))
            .unwrap();
        assert!(is_id(id), "{bundle}");
        // The size and hash recorded are the stored object's, which its bytes bear out.
        let bytes = get(&s3, bundle);
        let hash = blake3::hash(&bytes).to_hex().to_string();
        assert_eq!(entry["size"], bytes.len() as u64, "{bundle}");
        assert_eq!(entry["blake3"], hash.as_str(), "{bundle}");
        assert_eq!(metadata(&s3, bundle), (bytes.len() as u64, hash));
        let mut archive = tar::Archive::new(bytes.as_slice());
        let names: Vec<String> = (archive.entries().unwrap())
            .map(|e| e.unwrap().path().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names[0], "index.db", "{bundle}");
        let tracked = git(&dir.join(path), &["ls-files"]);
        let tracked = tracked.lines().map(|file| format!("mirror/{file}"));
        for name in ["mirror".to_string(), "mirror/.git/HEAD".into()]
            .into_iter()
            .chain(tracked)
        {
            let found = names.iter().any(|n| n.trim_end_matches('/') == name);
            assert!(found, "{bundle}: {name}");
        }
        expected.push(bundle.to_string());
    }
    assert_eq!(repositories.len(), CHAIN.len());
    let mut keys_sorted = keys.clone();
    keys_sorted.sort();
    expected.sort();
    assert_eq!(keys_sorted, expected);
    assert_eq!(
        get(&s3, "prod/meada/current"),
        format!("{generation}\n").as_bytes()
    );

    // Second publish, a second later: the same id, and no object written again.
    let stored = objects(&s3, "prod/meada");
    let document_bytes = get(&s3, &document_key);
    std::thread::sleep(Duration::from_millis(1100));
    let (again, stderr, code) = publish(&dir, MANIFEST);
    assert_eq!(
        (again.as_str(), code),
        (stdout.as_str(), Some(0)),
        "{stderr}"
    );
    assert_eq!(objects(&s3, "prod/meada"), stored);
    assert_eq!(get(&s3, &document_key), document_bytes);

    // A second machine: another index_dir, indexed at the same commits.
    let other = fs::read_to_string(dir.join(MANIFEST))
        .unwrap()
        .replace("../indexes", "../indexes2");
    fs::write(dir.join("ops/second.toml"), other).unwrap();
    let (_, stderr, code) =
        crate::manifest::run_args(&dir, &["index", "--workspace", "ops/second.toml"]);
    assert_eq!(code, Some(0), "{stderr}");
    let (from_second, stderr, code) = publish(&dir, "ops/second.toml");
    assert_eq!(
        (from_second.as_str(), code),
        (stdout.as_str(), Some(0)),
        "{stderr}"
    );
    assert_eq!(objects(&s3, "prod/meada"), stored);

    // The verification a publish of an existing generation runs (#273's pull test closes it
    // end to end): a bundle no longer stored with the recorded size and hash is named, and
    // `current` stays as it was.
    let tampered = repositories[1]["bundle"].as_str().unwrap();
    put(&s3, tampered, b"not the bundle");
    put(&s3, "prod/meada/current", b"0000000000000000\n");
    let (stdout, stderr, code) = publish(&dir, MANIFEST);
    assert_eq!((stdout.as_str(), code), ("", Some(1)));
    assert!(stderr.contains(tampered), "{stderr}");
    assert_eq!(get(&s3, "prod/meada/current"), b"0000000000000000\n");
}

#[test]
fn publish_refuses_an_index_behind_its_mirror_a_dirty_mirror_and_a_missing_index() {
    let Some(endpoint) = service() else { return };
    let name = bucket_name("snapshot-refused");
    let s3 = bucket(&endpoint, &name);
    let dir = store_manifest("snapshot_refused", &endpoint, &name, SECRET);
    let (web, catalog) = (dir.join("chain-web"), dir.join("chain-catalog"));

    // An edited and committed file, without meada index: clean, but the index is behind.
    let file = web.join("src/status.ts");
    let src = fs::read_to_string(&file).unwrap();
    fs::write(&file, src + "\nexport const added = 1;\n").unwrap();
    git(&web, &["commit", "-q", "-am", "edit"]);
    // An untracked file: the index is current, the mirror is not clean.
    fs::write(catalog.join("notes.txt"), "draft\n").unwrap();
    // No index at all.
    fs::remove_dir_all(dir.join("indexes/example.com/chain/worker")).unwrap();

    let (stdout, stderr, code) = publish(&dir, MANIFEST);
    assert_eq!((stdout.as_str(), code), ("", Some(1)));
    let lines: Vec<&str> = stderr.lines().collect();
    assert_eq!(lines.len(), 3, "{stderr}");
    assert_eq!(
        lines[0],
        "meada: example.com/chain/web: its index differs from the mirror; run meada index \
         --workspace ops/meada.workspace.toml"
    );
    assert!(
        lines[1].starts_with("meada: example.com/chain/catalog: the mirror ")
            && lines[1]
                .ends_with(" is not clean: git status lists changed, untracked or ignored files"),
        "{stderr}"
    );
    assert_eq!(
        lines[2],
        "meada: example.com/chain/worker: not indexed; run meada index --workspace ops/meada.workspace.toml"
    );
    // Nothing reached the store.
    assert!(objects(&s3, "prod/meada").is_empty());

    // Once indexed and clean again, it publishes.
    fs::remove_file(catalog.join("notes.txt")).unwrap();
    assert!(run(&dir, "index").2);
    let (_, stderr, code) = publish(&dir, MANIFEST);
    assert_eq!(code, Some(0), "{stderr}");
}

#[test]
fn an_authentication_failure_shows_no_secret() {
    let Some(endpoint) = service() else { return };
    let name = bucket_name("snapshot-auth");
    bucket(&endpoint, &name);
    let wrong = "n0t-the-s3cret-key";
    let dir = store_manifest("snapshot_auth", &endpoint, &name, wrong);
    let (stdout, stderr, code) = publish(&dir, MANIFEST);
    assert_eq!((stdout.as_str(), code), ("", Some(1)), "{stderr}");
    assert!(
        stderr.starts_with("meada: ") && stderr.len() > 8,
        "{stderr}"
    );
    assert!(
        !stderr.contains(wrong) && !stderr.contains(SECRET),
        "{stderr}"
    );
}

/// `meada index --workspace` with a `[snapshot]` section opens no connection: the endpoint is a
/// port this test holds and never sees a connection on.
#[test]
fn index_with_a_snapshot_section_opens_no_connection() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let dir = chain_manifest("snapshot_index_offline");
    fs::write(dir.join("ops/s3-secret"), "x\n").unwrap();
    append_section(
        &dir,
        &section(&format!("http://127.0.0.1:{port}"), "meada-offline"),
    );
    let (_, stderr, ok) = run(&dir, "index");
    assert!(ok, "{stderr}");
    listener.set_nonblocking(true).unwrap();
    let accepted = listener.accept().map(drop).map_err(|e| e.kind());
    assert_eq!(accepted, Err(std::io::ErrorKind::WouldBlock));
}

/// A store that redirects every request to another origin: the publish fails, and the other
/// origin never sees a connection.
#[test]
fn publish_follows_no_redirect() {
    use std::io::{BufRead, BufReader, Write};
    let elsewhere = TcpListener::bind("127.0.0.1:0").unwrap();
    let to = elsewhere.local_addr().unwrap().port();
    let store = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = store.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in store.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                line.clear();
            }
            let answer = format!(
                "HTTP/1.1 307 Temporary Redirect\r\nLocation: http://127.0.0.1:{to}/x\r\n\
                 Content-Length: 0\r\nConnection: close\r\n\r\n"
            );
            let _ = stream.write_all(answer.as_bytes());
        }
    });
    let dir = store_manifest(
        "snapshot_redirect",
        &format!("http://127.0.0.1:{port}"),
        "meada-redirect",
        SECRET,
    );
    let (stdout, stderr, code) = publish(&dir, MANIFEST);
    assert_eq!((stdout.as_str(), code), ("", Some(1)), "{stderr}");
    elsewhere.set_nonblocking(true).unwrap();
    let accepted = elsewhere.accept().map(drop).map_err(|e| e.kind());
    assert_eq!(accepted, Err(std::io::ErrorKind::WouldBlock));
}

/// A section the module checks, in a build with it: each problem names the file, the line and
/// the key.
#[test]
fn snapshot_section_errors_name_the_file_the_line_and_the_key() {
    let dir = chain_manifest("snapshot_section_errors");
    let manifest = fs::read_to_string(dir.join(MANIFEST)).unwrap();
    let at = "meada: ops/meada.workspace.toml";
    // An absent key is reported at its table's line.
    let table = format!(
        ":{}: `snapshot.path_style`",
        manifest.matches('\n').count() + 2
    );
    for (body, expected) in [
        (
            "endpoint = \"https://10.0.0.1\"\nregion = \"us-east-1\"\nbucket = \"b-1\"\n",
            table.as_str(),
        ),
        (
            "endpoint = \"http://s3.example.internal\"\nregion = \"us-east-1\"\nbucket = \"b-1\"\n",
            "`snapshot.endpoint`: must be an https:// URL: TLS is required unless the host is loopback",
        ),
        (
            "endpoint = \"https://s3.example.internal\"\nregion = \"us-east-1\"\n",
            "`snapshot.bucket` is missing",
        ),
        (
            "endpoint = \"https://s3.example.internal\"\nregion = \"us-east-1\"\nbucket = \"b-1\"\ncredentials = \"profile\"\n",
            "`snapshot.credentials`",
        ),
    ] {
        fs::write(
            dir.join(MANIFEST),
            format!("{manifest}\n[snapshot]\n{body}"),
        )
        .unwrap();
        let (stdout, stderr, ok) = run(&dir, "index");
        assert!(!ok && stdout.is_empty(), "{body}");
        assert!(
            stderr.starts_with(at) && stderr.contains(expected),
            "{stderr}"
        );
    }
}

// The pull.

/// A node's manifest, `<dir>/node/meada.workspace.toml`, listing no repository: its
/// `[snapshot]` section names the same store, and `local_dir = "generations"`. Returns the
/// manifest, relative to `dir`, and the local directory.
pub(crate) fn node(dir: &Path, endpoint: &str, bucket: &str) -> (String, PathBuf) {
    named_node(dir, "node", endpoint, bucket)
}

/// `node` in `<dir>/<name>/`: another node of the same store.
pub(crate) fn named_node(
    dir: &Path,
    name: &str,
    endpoint: &str,
    bucket: &str,
) -> (String, PathBuf) {
    let node = dir.join(name);
    let _ = fs::remove_dir_all(&node);
    fs::create_dir_all(&node).unwrap();
    fs::write(node.join("s3-secret"), format!("{SECRET}\n")).unwrap();
    let toml = format!(
        "index_dir = \"indexes\"\n{}local_dir = \"generations\"\n",
        section(endpoint, bucket)
    );
    fs::write(node.join("meada.workspace.toml"), toml).unwrap();
    (
        format!("{name}/meada.workspace.toml"),
        node.join("generations"),
    )
}

/// `meada snapshot pull --workspace <manifest> <more>` in `dir`, as `publish`.
pub(crate) fn pull(dir: &Path, manifest: &str, more: &[&str]) -> (String, String, Option<i32>) {
    let out = pull_command(dir, manifest, more)
        .output()
        .expect("meada runs");
    let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
    (text(&out.stdout), text(&out.stderr), out.status.code())
}

pub(crate) fn pull_command(dir: &Path, manifest: &str, more: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_meada"));
    command
        .current_dir(dir)
        .args(["snapshot", "pull", "--workspace", manifest])
        .args(more)
        .env("LANG", "en_US.UTF-8")
        .env("MEADA_TEST_S3_KEY", KEY_ID)
        .env_remove("MEADA_TEST_S3_TOKEN");
    command
}

/// What `local_dir` holds: each name, sorted, and the pointer file.
pub(crate) fn held(local: &Path) -> (Vec<String>, String) {
    let mut names: Vec<String> = (fs::read_dir(local).unwrap())
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let pointer = fs::read_to_string(local.join("current")).unwrap_or_default();
    (names, pointer)
}

/// The files of the mirror at `dir`, bytes included, but for the git configuration and hooks
/// a pull replaces and removes.
fn mirror_files(dir: &Path) -> Vec<(String, Vec<u8>)> {
    (crate::manifest::contents(dir).into_iter())
        .filter(|(path, _)| path != ".git/config" && !path.starts_with(".git/hooks/"))
        .collect()
}

/// A commit in `repo` and `meada index --workspace`, so the next publish is a new generation.
pub(crate) fn next_commit(dir: &Path, repo: &str, line: &str) {
    let file = dir.join(repo).join("src/status.ts");
    let src = fs::read_to_string(&file).unwrap();
    fs::write(
        &file,
        format!("{src}\nexport function {line}() {{\n  return 1;\n}}\n"),
    )
    .unwrap();
    git(&dir.join(repo), &["commit", "-q", "-am", line]);
    assert!(run(dir, "index").2, "index");
}

#[test]
fn pull_takes_the_published_generation_and_keeps_the_previous_one() {
    let Some(endpoint) = service() else { return };
    let name = bucket_name("snapshot-pull");
    bucket(&endpoint, &name);
    let dir = store_manifest("snapshot_pull", &endpoint, &name, SECRET);
    let (generation, stderr, code) = publish(&dir, MANIFEST);
    assert_eq!(code, Some(0), "{stderr}");
    let (manifest, local) = node(&dir, &endpoint, &name);

    // Published, then pulled in another directory: the same mirrors and indexes.
    let (stdout, stderr, code) = pull(&dir, &manifest, &[]);
    assert_eq!(
        (stdout.as_str(), code),
        (generation.as_str(), Some(0)),
        "{stderr}"
    );
    let first = generation.trim().to_string();
    // The generations held, the pointer file and the lock.
    let names = |mut ids: Vec<String>| {
        ids.extend(["current".to_string(), "pull.lock".to_string()]);
        ids.sort();
        ids
    };
    assert_eq!(
        held(&local),
        (names(vec![first.clone()]), format!("{first} 0\n"))
    );
    let at = local.join(&first);
    let document: serde_json::Value =
        serde_json::from_slice(&fs::read(at.join("generation.json")).unwrap()).unwrap();
    let repositories = document["repositories"].as_array().unwrap();
    assert_eq!(repositories.len(), CHAIN.len());
    for ((identity, path), entry) in CHAIN.iter().zip(repositories) {
        assert_eq!(entry["identity"], *identity);
        let bundle = entry["bundle"].as_str().unwrap();
        let bundle = bundle
            .rsplit('/')
            .next()
            .unwrap()
            .strip_suffix(".tar")
            .unwrap();
        let pulled = at.join(bundle);
        let index = dir.join("indexes").join(identity).join("index.db");
        assert_eq!(
            fs::read(pulled.join("index.db")).unwrap(),
            fs::read(index).unwrap(),
            "{identity}"
        );
        let mirror = pulled.join("mirror");
        assert_eq!(
            mirror_files(&mirror),
            mirror_files(&dir.join(path)),
            "{identity}"
        );
        assert_eq!(
            fs::read_to_string(mirror.join(".git/config")).unwrap(),
            format!(
                "[core]\n\trepositoryformatversion = 0\n\tbare = false\n\
                 [remote \"origin\"]\n\turl = https://{identity}\n"
            )
        );
        assert!(!mirror.join(".git/hooks").exists(), "{identity}");
    }

    // Already current: the same id, nothing moves.
    let before = held(&local);
    let (again, stderr, code) = pull(&dir, &manifest, &[]);
    assert_eq!(
        (again.as_str(), code),
        (generation.as_str(), Some(0)),
        "{stderr}"
    );
    assert_eq!(held(&local), before);

    // One pull at a time: a second one, while the lock is held, exits 0 saying so.
    let lock = fs::File::open(local.join("pull.lock")).unwrap();
    lock.lock().unwrap();
    let (stdout, stderr, code) = pull(&dir, &manifest, &[]);
    assert_eq!((stdout.as_str(), code), ("", Some(0)));
    assert!(stderr.contains("a pull is running in "), "{stderr}");
    drop(lock);

    // A second generation: the first is kept as the previous one.
    next_commit(&dir, "chain-web", "second");
    let (second, stderr, code) = publish(&dir, MANIFEST);
    assert_eq!(code, Some(0), "{stderr}");
    let second = second.trim().to_string();
    let (stdout, stderr, code) = pull(&dir, &manifest, &[]);
    assert_eq!(
        (stdout.trim(), code),
        (second.as_str(), Some(0)),
        "{stderr}"
    );
    let both = vec![first.clone(), second.clone()];
    assert_eq!(held(&local), (names(both), format!("{second} 0\n")));

    // `--every` repeats the pull: a third generation published while it runs is pulled, and
    // the first one, older than the previous, is deleted.
    let mut every = pull_command(&dir, &manifest, &["--every", "1"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let stdout = every.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        use std::io::BufRead;
        for line in std::io::BufReader::new(stdout).lines() {
            let _ = tx.send(line.unwrap());
        }
    });
    next_commit(&dir, "chain-web", "third");
    let (third, stderr, code) = publish(&dir, MANIFEST);
    assert_eq!(code, Some(0), "{stderr}");
    let third = third.trim().to_string();
    let line = rx.recv_timeout(Duration::from_secs(60));
    let _ = every.kill();
    let _ = every.wait();
    // Nothing was printed while the second generation stayed current.
    assert_eq!(line.as_deref(), Ok(third.as_str()));
    let both = vec![second.clone(), third.clone()];
    assert_eq!(held(&local), (names(both), format!("{third} 0\n")));

    // The pointer names a generation whose directory is gone: `--every` fetches it again and
    // prints its id, as for any generation it swaps in.
    fs::remove_dir_all(local.join(&third)).unwrap();
    let mut every = pull_command(&dir, &manifest, &["--every", "1"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let mut stdout = std::io::BufReader::new(every.stdout.take().unwrap());
    let mut line = String::new();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        use std::io::BufRead;
        let _ = stdout.read_line(&mut line);
        let _ = tx.send(line);
    });
    let line = rx.recv_timeout(Duration::from_secs(60));
    let _ = every.kill();
    let _ = every.wait();
    assert_eq!(line.as_deref().map(str::trim), Ok(third.as_str()));
    // The pointer named the generation fetched again, so that is the one it replaced: the
    // second one goes.
    let one = vec![third.clone()];
    assert_eq!(held(&local), (names(one), format!("{third} 0\n")));
}

/// Puts `bytes` at `key` with the metadata Meada stores every object with.
pub(crate) fn put_stored(s3: &AmazonS3, key: &str, bytes: &[u8]) {
    let mut attributes = Attributes::new();
    let size = bytes.len().to_string();
    let hash = blake3::hash(bytes).to_hex().to_string();
    attributes.insert(Attribute::Metadata("meada-size".into()), size.into());
    attributes.insert(Attribute::Metadata("meada-blake3".into()), hash.into());
    let options = PutOptions {
        attributes,
        ..PutOptions::default()
    };
    block_on(s3.put_opts(&Key::from(key), bytes.to_vec().into(), options)).unwrap();
}

/// A generation of one repository made by hand: the bundle `tar` stored under `identity`, a
/// document recording `recorded` (or the bundle's own size and hash) and `head`, and the store's
/// pointer moved to it. Returns its id.
fn stage(
    s3: &AmazonS3,
    identity: &str,
    head: &str,
    tar: &[u8],
    recorded: Option<(u64, String)>,
) -> String {
    let bundle = blake3::hash(tar).to_hex()[..16].to_string();
    let key = format!("prod/meada/repos/{identity}/{bundle}.tar");
    put_stored(s3, &key, tar);
    let (size, hash) =
        recorded.unwrap_or_else(|| (tar.len() as u64, blake3::hash(tar).to_hex().to_string()));
    let generation = meada::snapshot::generation_id(&[(identity, &bundle)]);
    let document = serde_json::json!({"generation": generation, "repositories": [{
        "identity": identity, "head": head, "bundle": key, "blocks": {},
        "size": size, "blake3": hash}]});
    let document = serde_json::to_vec(&document).unwrap();
    put_stored(
        s3,
        &format!("prod/meada/generations/{generation}.json"),
        &document,
    );
    put_stored(
        s3,
        "prod/meada/current",
        format!("{generation}\n").as_bytes(),
    );
    generation
}

/// A bundle: `index.db`, the mirror at `mirror` (symbolic links as links), then `extra` entries
/// written raw, path and link name as given.
fn bundle_of(index: &Path, mirror: &Path, extra: &[(&[u8], tar::EntryType, &[u8])]) -> Vec<u8> {
    let mut tar = tar::Builder::new(Vec::new());
    tar.follow_symlinks(false);
    tar.append_path_with_name(index, "index.db").unwrap();
    tar.append_dir_all("mirror", mirror).unwrap();
    for (path, kind, link) in extra {
        let mut header = tar::Header::new_gnu();
        header.as_old_mut().name[..path.len()].copy_from_slice(path);
        header.as_old_mut().linkname[..link.len()].copy_from_slice(link);
        header.set_entry_type(*kind);
        let data: &[u8] = if *kind == tar::EntryType::Regular {
            b"x"
        } else {
            b""
        };
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append(&header, data).unwrap();
    }
    tar.into_inner().unwrap()
}

/// Schema objects a store could slip into an index of the right version, each with the object
/// the refusal names: an extra view, an extra trigger and an altered table.
pub(crate) const HOSTILE_SCHEMAS: [(&str, &str); 3] = [
    (
        "CREATE VIEW endless AS WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n) \
         SELECT i FROM n;",
        "index.db: the view endless",
    ),
    (
        "CREATE TRIGGER t AFTER INSERT ON meta BEGIN DELETE FROM files; END;",
        "index.db: the trigger t",
    ),
    (
        "DROP TABLE communities; CREATE TABLE communities (name TEXT, member TEXT, x);",
        "index.db: the table communities",
    ),
];

/// A copy of the index at `index`, at `to`, with `sql` run over it.
pub(crate) fn crafted_index(index: &Path, to: &Path, sql: &str) {
    fs::copy(index, to).unwrap();
    rusqlite::Connection::open(to)
        .unwrap()
        .execute_batch(sql)
        .unwrap();
}

#[test]
fn a_corrupted_or_hostile_generation_fails_the_pull_and_leaves_the_previous_one() {
    let Some(endpoint) = service() else { return };
    let name = bucket_name("snapshot-hostile");
    let s3 = bucket(&endpoint, &name);
    let dir = store_manifest("snapshot_hostile", &endpoint, &name, SECRET);
    let (_, stderr, code) = publish(&dir, MANIFEST);
    assert_eq!(code, Some(0), "{stderr}");
    let (manifest, local) = node(&dir, &endpoint, &name);
    let (_, stderr, code) = pull(&dir, &manifest, &[]);
    assert_eq!(code, Some(0), "{stderr}");
    let before = held(&local);

    let (identity, path) = CHAIN[0];
    let (mirror, index) = (
        dir.join(path),
        dir.join("indexes").join(identity).join("index.db"),
    );
    let head = git(&mirror, &["rev-parse", "HEAD"]).trim().to_string();
    let valid = bundle_of(&index, &mirror, &[]);
    let escaped = dir.join("escaped");
    let _ = fs::remove_file(&escaped);
    let link = tar::EntryType::Symlink;
    let file = tar::EntryType::Regular;
    let hostile: Vec<PathBuf> = (HOSTILE_SCHEMAS.iter().enumerate())
        .map(|(n, (sql, _))| {
            let crafted = dir.join(format!("hostile-{n}.db"));
            crafted_index(&index, &crafted, sql);
            crafted
        })
        .collect();
    type Stage<'a> = Box<dyn Fn() -> String + 'a>;
    let mut cases: Vec<(&str, Stage, &str)> = vec![
        (
            "a corrupted bundle",
            Box::new(|| {
                let hash = blake3::hash(b"another bundle").to_hex().to_string();
                stage(
                    &s3,
                    identity,
                    &head,
                    &valid,
                    Some((valid.len() as u64, hash)),
                )
            }),
            "not stored with the size and hash it records",
        ),
        (
            "a truncated bundle",
            Box::new(|| {
                let hash = blake3::hash(&valid).to_hex().to_string();
                stage(
                    &s3,
                    identity,
                    &head,
                    &valid[..valid.len() - 512],
                    Some((valid.len() as u64, hash)),
                )
            }),
            "not stored with the size and hash it records",
        ),
        (
            "a traversal entry",
            Box::new(|| {
                let tar = bundle_of(&index, &mirror, &[(b"mirror/../../../escaped", file, b"")]);
                stage(&s3, identity, &head, &tar, None)
            }),
            "`..` component",
        ),
        (
            "an escaping link",
            Box::new(|| {
                let tar = bundle_of(
                    &index,
                    &mirror,
                    &[(b"mirror/out", link, b"../../../../escaped")],
                );
                stage(&s3, identity, &head, &tar, None)
            }),
            "leaves the repository directory",
        ),
        (
            "a hostile id",
            Box::new(|| {
                put_stored(&s3, "prod/meada/current", b"../../../../escaped\n");
                String::new()
            }),
            "prod/meada/current: does not hold an id",
        ),
        (
            "a pointer past `CURRENT_MAX`",
            Box::new(|| {
                put_stored(&s3, "prod/meada/current", &[b'0'; 65]);
                String::new()
            }),
            "prod/meada/current: not stored with the size and hash it records",
        ),
        (
            "a document past `DOCUMENT_MAX`",
            Box::new(|| {
                let id = "00000000000000dd";
                let key = format!("prod/meada/generations/{id}.json");
                put_stored(&s3, &key, &vec![b' '; (16 << 20) + 1]);
                put_stored(&s3, "prod/meada/current", format!("{id}\n").as_bytes());
                String::new()
            }),
            "prod/meada/generations/00000000000000dd.json: not stored with the size and hash",
        ),
        (
            "a hostile identity",
            Box::new(|| stage(&s3, "example.com/../../escaped", &head, &valid, None)),
            "not a repository identity",
        ),
    ];
    for (crafted, (_, object)) in hostile.iter().zip(HOSTILE_SCHEMAS) {
        let (mirror, s3, head) = (&mirror, &s3, &head);
        let stage_it = move || {
            let tar = bundle_of(crafted, mirror, &[]);
            stage(s3, identity, head, &tar, None)
        };
        cases.push(("a hostile index schema", Box::new(stage_it), object));
    }
    for (case, staged, expected) in cases {
        staged();
        let (stdout, stderr, code) = pull(&dir, &manifest, &[]);
        assert_eq!((stdout.as_str(), code), ("", Some(1)), "{case}: {stderr}");
        assert!(stderr.contains(expected), "{case}: {stderr}");
        assert_eq!(held(&local), before, "{case}");
        assert!(!escaped.exists(), "{case}");
    }

    // A store that cannot be reached changes nothing either.
    let toml = fs::read_to_string(dir.join(&manifest)).unwrap();
    let toml = toml.replace(&endpoint, "http://127.0.0.1:1");
    fs::write(dir.join("node/unreachable.toml"), toml).unwrap();
    let (_, _, code) = pull(&dir, "node/unreachable.toml", &[]);
    assert_eq!(code, Some(1));
    assert_eq!(held(&local), before);
}

/// A bundle whose `.git/config` sets `core.fsmonitor` and `core.hooksPath`, and whose
/// `.git/hooks` holds hooks: the pull replaces the one and removes the other, so neither the
/// pull's `git` calls nor a later `git status` in the pulled mirror runs anything.
#[cfg(unix)]
#[test]
fn a_pulled_mirror_runs_nothing_its_bundle_configured() {
    use std::os::unix::fs::PermissionsExt;
    let Some(endpoint) = service() else { return };
    let name = bucket_name("snapshot-config");
    let s3 = bucket(&endpoint, &name);
    let dir = store_manifest("snapshot_config", &endpoint, &name, SECRET);
    let (identity, path) = CHAIN[0];
    let index = dir.join("indexes").join(identity).join("index.db");

    // The hostile mirror: a copy of the web mirror whose git runs `ran` when it can.
    let ran = dir.join("ran");
    let script = dir.join("hook.sh");
    fs::write(
        &script,
        format!("#!/bin/sh\necho \"$0\" >> {}\n", ran.display()),
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let hostile = dir.join("hostile");
    let _ = fs::remove_dir_all(&hostile);
    assert!(
        Command::new("cp")
            .args(["-a"])
            .arg(dir.join(path))
            .arg(&hostile)
            .status()
            .unwrap()
            .success()
    );
    let hooks = hostile.join(".git/hooks");
    fs::create_dir_all(&hooks).unwrap();
    for hook in ["post-checkout", "pre-commit", "fsmonitor-watchman"] {
        fs::copy(&script, hooks.join(hook)).unwrap();
    }
    let config = hostile.join(".git/config");
    let text = fs::read_to_string(&config).unwrap();
    let fsmonitor = format!(
        "[core]\n\tfsmonitor = {}\n\thooksPath = {}\n",
        script.display(),
        hooks.display()
    );
    fs::write(&config, format!("{text}{fsmonitor}")).unwrap();
    // The configuration does run the script where it stays.
    let status = |at: &Path| {
        Command::new("git")
            .current_dir(at)
            .args(["status", "--porcelain"])
            .output()
            .unwrap()
    };
    status(&hostile);
    assert!(ran.exists(), "the hostile configuration runs the script");
    fs::remove_file(&ran).unwrap();

    let head = git(&hostile, &["rev-parse", "HEAD"]).trim().to_string();
    let generation = stage(
        &s3,
        identity,
        &head,
        &bundle_of(&index, &hostile, &[]),
        None,
    );
    let (manifest, local) = node(&dir, &endpoint, &name);
    let (stdout, stderr, code) = pull(&dir, &manifest, &[]);
    assert_eq!(
        (stdout.trim(), code),
        (generation.as_str(), Some(0)),
        "{stderr}"
    );
    assert!(!ran.exists(), "the pull ran the script");
    let document: serde_json::Value =
        serde_json::from_slice(&fs::read(local.join(&generation).join("generation.json")).unwrap())
            .unwrap();
    let bundle = document["repositories"][0]["bundle"].as_str().unwrap();
    let bundle = bundle
        .rsplit('/')
        .next()
        .unwrap()
        .strip_suffix(".tar")
        .unwrap();
    let pulled = local.join(&generation).join(bundle).join("mirror");
    assert!(!pulled.join(".git/hooks").exists());
    let out = status(&pulled);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !ran.exists(),
        "git status in the pulled mirror ran the script"
    );
}

/// A bundle carrying a nested repository behind a gitlink, its configuration running a script
/// through `core.fsmonitor`: as a `.git` directory in the submodule, or as a gitfile pointing
/// into `.git/modules`. `git status` in the mirror descends into it and runs the script, so the
/// pull refuses the entry, runs nothing and keeps the previous generation.
#[cfg(unix)]
#[test]
fn a_nested_repository_fails_the_pull_and_runs_nothing() {
    use std::os::unix::fs::PermissionsExt;
    let Some(endpoint) = service() else { return };
    let name = bucket_name("snapshot-nested");
    let s3 = bucket(&endpoint, &name);
    let dir = store_manifest("snapshot_nested", &endpoint, &name, SECRET);
    let (_, stderr, code) = publish(&dir, MANIFEST);
    assert_eq!(code, Some(0), "{stderr}");
    let (manifest, local) = node(&dir, &endpoint, &name);
    let (_, stderr, code) = pull(&dir, &manifest, &[]);
    assert_eq!(code, Some(0), "{stderr}");
    let before = held(&local);

    let (identity, path) = CHAIN[0];
    let index = dir.join("indexes").join(identity).join("index.db");
    let ran = dir.join("ran");
    let script = dir.join("hook.sh");
    let text = format!("#!/bin/sh\necho \"$0\" >> {}\n", ran.display());
    fs::write(&script, text).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let fsmonitor = format!("[core]\n\tfsmonitor = {}\n", script.display());

    for layout in ["directory", "gitfile"] {
        // The web mirror with `sub`, a repository of its own, committed as a gitlink.
        let hostile = dir.join(format!("hostile-{layout}"));
        let _ = fs::remove_dir_all(&hostile);
        let mut copy = Command::new("cp");
        copy.arg("-a").arg(dir.join(path)).arg(&hostile);
        assert!(copy.status().unwrap().success());
        let sub = hostile.join("sub");
        fs::create_dir_all(&sub).unwrap();
        git(&sub, &["-c", "init.defaultBranch=main", "init", "-q"]);
        fs::write(sub.join("a.txt"), "a\n").unwrap();
        git(&sub, &["add", "a.txt"]);
        git(&sub, &["commit", "-q", "-m", "sub"]);
        git(&hostile, &["add", "sub"]);
        git(&hostile, &["commit", "-q", "-m", "gitlink"]);
        let config = match layout {
            "directory" => sub.join(".git/config"),
            _ => {
                let modules = hostile.join(".git/modules");
                fs::create_dir_all(&modules).unwrap();
                fs::rename(sub.join(".git"), modules.join("sub")).unwrap();
                fs::write(sub.join(".git"), "gitdir: ../.git/modules/sub\n").unwrap();
                let config = modules.join("sub/config");
                let file = config.to_str().unwrap();
                git(
                    &hostile,
                    &["config", "-f", file, "core.worktree", "../../../sub"],
                );
                config
            }
        };
        let text = fs::read_to_string(&config).unwrap();
        fs::write(&config, format!("{text}{fsmonitor}")).unwrap();
        // Where it stays, `git status` in the mirror runs the submodule's script.
        let out = Command::new("git")
            .current_dir(&hostile)
            .args(["status", "--porcelain"])
            .output()
            .unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{layout}: {err}");
        assert!(
            ran.exists(),
            "{layout}: the nested configuration runs the script"
        );
        fs::remove_file(&ran).unwrap();

        let head = git(&hostile, &["rev-parse", "HEAD"]).trim().to_string();
        stage(
            &s3,
            identity,
            &head,
            &bundle_of(&index, &hostile, &[]),
            None,
        );
        let (stdout, stderr, code) = pull(&dir, &manifest, &[]);
        assert_eq!((stdout.as_str(), code), ("", Some(1)), "{layout}: {stderr}");
        assert!(
            stderr.contains("entry `mirror/") && stderr.contains("nested repository"),
            "{layout}: {stderr}"
        );
        assert!(!ran.exists(), "{layout}: the pull ran the script");
        assert_eq!(held(&local), before, "{layout}");
    }
}
