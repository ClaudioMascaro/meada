//! A private-CA TLS front end over the local S3 test service, exercised through the CLI.

use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};

use super::{
    KEY_ID, MANIFEST, SECRET, bucket, bucket_name, node, publish, pull, service, store_manifest,
};

const CA: &[u8] = include_bytes!("../../fixtures/tls/ca.pem");
const WRONG_CA: &[u8] = include_bytes!("../../fixtures/tls/wrong-ca.pem");

/// TLS is terminated here; the real store still checks signatures and stores every object.
fn front(endpoint: &str) -> String {
    let endpoint = reqwest::Url::parse(endpoint).unwrap();
    assert_eq!(
        endpoint.scheme(),
        "http",
        "the local S3 test service uses HTTP"
    );
    let target = (
        endpoint.host_str().unwrap().to_string(),
        endpoint.port_or_known_default().unwrap(),
    );
    let certs = CertificateDer::pem_slice_iter(include_bytes!("../../fixtures/tls/server.pem"))
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let key =
        PrivateKeyDer::from_pem_slice(include_bytes!("../../fixtures/tls/server.key.pem")).unwrap();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .unwrap();
    let config = Arc::new(config);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("https://{}", listener.local_addr().unwrap());
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let config = Arc::clone(&config);
            let target = target.clone();
            thread::spawn(move || {
                let _ = forward(stream, &target, config);
            });
        }
    });
    url
}

fn forward(
    stream: TcpStream,
    target: &(String, u16),
    config: Arc<rustls::ServerConfig>,
) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let connection = rustls::ServerConnection::new(config).map_err(io::Error::other)?;
    let mut incoming = BufReader::new(rustls::StreamOwned::new(connection, stream));
    let mut outgoing = TcpStream::connect((target.0.as_str(), target.1))?;
    outgoing.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut line = String::new();
    incoming.read_line(&mut line)?;
    outgoing.write_all(line.as_bytes())?;
    let mut length = 0;
    let mut chunked = false;
    loop {
        line.clear();
        incoming.read_line(&mut line)?;
        if line == "\r\n" {
            break;
        }
        let (name, value) = line
            .trim_end()
            .split_once(':')
            .ok_or_else(|| io::Error::other("header"))?;
        if name.eq_ignore_ascii_case("content-length") {
            length = value.trim().parse::<u64>().unwrap();
        }
        if name.eq_ignore_ascii_case("transfer-encoding") {
            chunked = value.trim().eq_ignore_ascii_case("chunked");
        }
        if !name.eq_ignore_ascii_case("connection") {
            outgoing.write_all(line.as_bytes())?;
        }
    }
    outgoing.write_all(b"Connection: close\r\n\r\n")?;
    if chunked {
        loop {
            line.clear();
            incoming.read_line(&mut line)?;
            outgoing.write_all(line.as_bytes())?;
            let size = u64::from_str_radix(line.trim().split(';').next().unwrap(), 16).unwrap();
            if size == 0 {
                loop {
                    line.clear();
                    incoming.read_line(&mut line)?;
                    outgoing.write_all(line.as_bytes())?;
                    if line == "\r\n" {
                        break;
                    }
                }
                break;
            }
            io::copy(&mut incoming.by_ref().take(size + 2), &mut outgoing)?;
        }
    } else {
        io::copy(&mut incoming.by_ref().take(length), &mut outgoing)?;
    }
    outgoing.flush()?;
    io::copy(&mut outgoing, incoming.get_mut())?;
    let mut stream = incoming.into_inner();
    stream.conn.send_close_notify();
    stream.flush()
}

#[test]
fn publish_and_pull_trust_only_the_configured_private_ca_without_leaking_credentials() {
    let Some(endpoint) = service() else { return };
    let name = bucket_name("snapshot-private-ca");
    bucket(&endpoint, &name);
    let endpoint = front(&endpoint);
    let dir = store_manifest("snapshot_private_ca", &endpoint, &name, SECRET);
    fs::write(dir.join("ops/ca.pem"), CA).unwrap();
    let manifest = dir.join(MANIFEST);
    let original = fs::read_to_string(&manifest).unwrap();
    fs::write(&manifest, format!("{original}ca_file = \"ca.pem\"\n")).unwrap();
    let (generation, error, code) = publish(&dir, MANIFEST);
    assert_eq!(code, Some(0), "{error}");
    assert_eq!(generation.trim().len(), 16, "{generation}");

    let (node_manifest, _) = node(&dir, &endpoint, &name);
    let node_file = dir.join(&node_manifest);
    let original_node = fs::read_to_string(&node_file).unwrap();
    fs::write(
        &node_file,
        format!("{original_node}ca_file = \"../ops/ca.pem\"\n"),
    )
    .unwrap();
    let (pulled, error, code) = pull(&dir, &node_manifest, &[]);
    assert_eq!((pulled, code), (generation, Some(0)), "{error}");

    for ca in [None, Some(WRONG_CA)] {
        if let Some(ca) = ca {
            fs::write(dir.join("ops/ca.pem"), ca).unwrap();
        } else {
            fs::write(&manifest, &original).unwrap();
            fs::write(&node_file, &original_node).unwrap();
        }
        if ca.is_some() {
            fs::write(&manifest, format!("{original}ca_file = \"ca.pem\"\n")).unwrap();
            fs::write(
                &node_file,
                format!("{original_node}ca_file = \"../ops/ca.pem\"\n"),
            )
            .unwrap();
        }
        for result in [publish(&dir, MANIFEST), pull(&dir, &node_manifest, &[])] {
            let (stdout, error, code) = result;
            assert_eq!((stdout.as_str(), code), ("", Some(1)), "{error}");
            assert!(
                !error.contains(KEY_ID) && !error.contains(SECRET),
                "{error}"
            );
        }
    }

    for malformed in [
        Vec::new(),
        b"-----BEGIN CERTIFICATE-----\nY2VydGlmaWNhdGU=\n-----END CERTIFICATE-----\n".to_vec(),
        vec![b'x'; (1 << 20) + 1],
    ] {
        fs::write(dir.join("ops/ca.pem"), malformed).unwrap();
        for (stdout, error, code) in [publish(&dir, MANIFEST), pull(&dir, &node_manifest, &[])] {
            assert_eq!((stdout.as_str(), code), ("", Some(1)), "{error}");
            assert!(error.contains("ca.pem"), "{error}");
            assert!(
                !error.contains(KEY_ID) && !error.contains(SECRET),
                "{error}"
            );
        }
    }
}
