//! The snapshot store's publish (ADR 0012, spec #259, feature `snapshot`): `meada snapshot
//! publish --workspace` over the chain's mirrors, against the local S3-compatible server
//! `MEADA_TEST_S3_ENDPOINT` names (path style). Without it the store tests print why and skip;
//! with `MEADA_TEST_SERVICES_REQUIRED=1` they fail.
#![cfg(feature = "snapshot")]

use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use object_store::aws::{AmazonS3, AmazonS3Builder};
use object_store::path::Path as Key;
use object_store::signer::Signer;
use object_store::{Attribute, GetOptions, ObjectMeta, ObjectStore, ObjectStoreExt};

use crate::common::git;
use crate::manifest::{CHAIN, MANIFEST, chain_manifest, run};

const KEY_ID: &str = "meada-test";
const SECRET: &str = "meada-test-secret";

/// The local store, or `None` with the reason printed.
fn service() -> Option<String> {
    let url = std::env::var("MEADA_TEST_S3_ENDPOINT").ok();
    if url.is_none() {
        let required = std::env::var("MEADA_TEST_SERVICES_REQUIRED").is_ok_and(|v| v == "1");
        assert!(!required, "MEADA_TEST_S3_ENDPOINT is not set");
        eprintln!("skipped: MEADA_TEST_S3_ENDPOINT is not set");
    }
    url
}

fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build();
    runtime.unwrap().block_on(future)
}

/// A client of `bucket` as the tests see the store, the bucket created.
fn bucket(endpoint: &str, bucket: &str) -> AmazonS3 {
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
fn bucket_name(case: &str) -> String {
    format!("meada-{case}-{}", std::process::id()).replace('_', "-")
}

/// The chain's manifest in `<case>/` with a `[snapshot]` section for `bucket`, indexed. The
/// secret key comes from a file, the key id from a variable `publish` sets.
fn store_manifest(case: &str, endpoint: &str, bucket: &str, secret: &str) -> PathBuf {
    let dir = chain_manifest(case);
    let secret_file = dir.join("ops/s3-secret");
    fs::write(&secret_file, format!("{secret}\n")).unwrap();
    append_section(&dir, &section(endpoint, bucket));
    assert!(run(&dir, "index").2, "index");
    dir
}

fn section(endpoint: &str, bucket: &str) -> String {
    format!(
        "\n[snapshot]\nendpoint = \"{endpoint}\"\nregion = \"us-east-1\"\nbucket = \"{bucket}\"\n\
         prefix = \"prod/meada\"\npath_style = true\naccess_key_id_env = \"MEADA_TEST_S3_KEY\"\n\
         secret_access_key_file = \"s3-secret\"\nsession_token_env = \"MEADA_TEST_S3_TOKEN\"\n"
    )
}

fn append_section(dir: &Path, section: &str) {
    let manifest = dir.join(MANIFEST);
    let toml = fs::read_to_string(&manifest).unwrap() + section;
    fs::write(&manifest, toml).unwrap();
}

/// `meada snapshot publish --workspace <manifest>` in `dir`: stdout, stderr and the exit code.
fn publish(dir: &Path, manifest: &str) -> (String, String, Option<i32>) {
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
fn objects(s3: &AmazonS3, prefix: &str) -> Vec<ObjectMeta> {
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

fn get(s3: &AmazonS3, key: &str) -> Vec<u8> {
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

fn put(s3: &AmazonS3, key: &str, bytes: &[u8]) {
    block_on(s3.put(&Key::from(key), bytes.to_vec().into())).unwrap();
}

/// The `meada-size` and `meada-blake3` metadata of the object at `key`.
fn metadata(s3: &AmazonS3, key: &str) -> (u64, String) {
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
