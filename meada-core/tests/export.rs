//! An export at the crate's public seam: what the binary writes reads back into the export types
//! and writes again to the same bytes, and a document that does not fit them is refused, naming
//! the field.

use std::fs;
use std::path::Path;

use meada_core::export::parse;

#[test]
fn every_export_golden_reads_and_writes_back_byte_for_byte() {
    let goldens = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/golden");
    let mut read = 0;
    for entry in fs::read_dir(&goldens).unwrap() {
        let path = entry.unwrap().path();
        if !path.to_string_lossy().ends_with(".export.json") {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap();
        let export = parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert_eq!(export.write().unwrap(), text, "{}", path.display());
        read += 1;
    }
    assert!(read > 50, "{read} export goldens");
}

const EXPORT: &str = r#"{
  "format": 1,
  "repository": "example.com/acme/web",
  "commit": "0123456789abcdef0123456789abcdef01234567",
  "dirty": false,
  "files": [
    {
      "path": "src/a.ts",
      "hash": "00"
    }
  ],
  "symbols": [],
  "calls": [
    {
      "id": "example.com/acme/web::src/a.ts::main::fetch#0",
      "caller": "example.com/acme/web::src/a.ts::main",
      "target": null,
      "level": null,
      "span": {
        "path": "src/a.ts",
        "start": 0,
        "end": 5
      }
    }
  ],
  "providers": [],
  "consumers": [],
  "links": []
}
"#;

#[test]
fn a_valid_export_reads() {
    let export = parse(EXPORT).unwrap();
    assert_eq!(export.calls[0].span.end, 5);
    assert_eq!(export.format, meada_core::export::FORMAT);
    assert_eq!(export.dirty, Some(false));
    assert_eq!(export.write().unwrap(), EXPORT);
}

/// `commit` and `dirty` are optional and left out when absent; `format` is not (#512).
#[test]
fn an_export_without_a_commit_writes_neither_provenance_field() {
    let bare = EXPORT.replace(
        "  \"commit\": \"0123456789abcdef0123456789abcdef01234567\",\n  \"dirty\": false,\n",
        "",
    );
    let export = parse(&bare).unwrap();
    assert_eq!((export.commit.as_deref(), export.dirty), (None, None));
    assert_eq!(export.write().unwrap(), bare);
}

#[test]
fn a_missing_required_field_is_refused_by_name() {
    let top = EXPORT.replace("  \"repository\": \"example.com/acme/web\",\n", "");
    assert_eq!(parse(&top).unwrap_err(), "missing field `repository`");
    let unversioned = EXPORT.replace("  \"format\": 1,\n", "");
    assert_eq!(parse(&unversioned).unwrap_err(), "missing field `format`");
    let nested = EXPORT.replace(",\n        \"end\": 5", "");
    assert_eq!(
        parse(&nested).unwrap_err(),
        "calls[0].span: missing field `end`"
    );
}

#[test]
fn a_field_of_the_wrong_type_is_refused_by_name() {
    let number = EXPORT.replace("\"hash\": \"00\"", "\"hash\": 7");
    assert_eq!(
        parse(&number).unwrap_err(),
        "files[0].hash: invalid type: integer `7`, expected a string"
    );
    let string = EXPORT.replace("\"start\": 0", "\"start\": \"0\"");
    assert_eq!(
        parse(&string).unwrap_err(),
        "calls[0].span.start: invalid type: string \"0\", expected i64"
    );
    let level = EXPORT.replace("\"level\": null", "\"level\": \"sure\"");
    let error = parse(&level).unwrap_err();
    assert!(
        error.starts_with("calls[0].level: unknown variant `sure`"),
        "{error}"
    );
    let list = EXPORT.replace("\"links\": []", "\"links\": {}");
    assert_eq!(
        parse(&list).unwrap_err(),
        "links: invalid type: map, expected a sequence"
    );
}

#[test]
fn text_that_is_no_json_is_refused() {
    assert!(parse("{").is_err());
    assert!(parse("").is_err());
}
