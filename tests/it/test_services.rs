//! `scripts/test-services` mirrors the services of CI's `cloud` job: images, ports, credentials
//! and the `MEADA_TEST_*` exports change together or this fails.

use std::fs;

fn read(path: &str) -> String {
    fs::read_to_string(format!("{}/{path}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

/// The value of `NAME=value` in the script's variable block.
fn var(script: &str, name: &str) -> String {
    let prefix = format!("{name}=");
    let line = script.lines().find_map(|l| l.strip_prefix(prefix.as_str()));
    line.unwrap_or_else(|| panic!("{name} missing in scripts/test-services"))
        .to_string()
}

#[test]
fn the_script_mirrors_the_cloud_job() {
    let ci = read(".github/workflows/ci.yml");
    let cloud = &ci[ci.find("\n  cloud:").expect("cloud job")..];
    let script = read("scripts/test-services");

    for name in ["VALKEY", "S3", "POSTGRES"] {
        let image = var(&script, &format!("{name}_IMAGE"));
        let port = var(&script, &format!("{name}_PORT"));
        assert!(
            cloud.contains(&format!("image: {image}\n")),
            "{name}: image {image}"
        );
        assert!(
            cloud.contains(&format!("- {port}:{port}\n")),
            "{name}: port {port}"
        );
    }
    let (access, secret) = (var(&script, "S3_ACCESS_KEY"), var(&script, "S3_SECRET_KEY"));
    let password = var(&script, "POSTGRES_PASSWORD");
    for (key, value) in [
        ("RUSTFS_ACCESS_KEY", &access),
        ("RUSTFS_SECRET_KEY", &secret),
        ("POSTGRES_PASSWORD", &password),
    ] {
        let line = format!("{key}: {value}\n");
        assert!(
            cloud.contains(&line),
            "ci.yml lacks {key} with the value scripts/test-services sets"
        );
    }

    // Each export the script prints, with its variables expanded, is a line of the job's env.
    let expand = |s: &str| {
        let mut s = s.to_string();
        for (k, v) in [
            ("$VALKEY_PORT", var(&script, "VALKEY_PORT")),
            ("$S3_PORT", var(&script, "S3_PORT")),
            ("$POSTGRES_TLS_PORT", var(&script, "POSTGRES_TLS_PORT")),
            ("$POSTGRES_PORT", var(&script, "POSTGRES_PORT")),
            ("$POSTGRES_PASSWORD", password.clone()),
        ] {
            s = s.replace(k, &v);
        }
        s
    };
    let exports: Vec<String> = (script.lines())
        .filter_map(|l| l.trim().strip_prefix("echo \"export "))
        .map(|l| expand(l.trim_end_matches('"')))
        .collect();
    let keys: Vec<&str> = exports
        .iter()
        .map(|e| e.split('=').next().unwrap())
        .collect();
    assert_eq!(exports.len(), 6, "{keys:?}");
    for export in exports {
        let (key, value) = export.split_once('=').unwrap();
        // The CA path is made by CI's step at run time, not set in the job's env.
        if key == "MEADA_TEST_POSTGRES_TLS_CA" {
            continue;
        }
        let value = value.trim_matches('"');
        let line = if value == "1" {
            format!("{key}: \"1\"\n")
        } else {
            format!("{key}: {value}\n")
        };
        assert!(cloud.contains(&line), "ci.yml env lacks {key}");
    }
}
