# Tests (`tests/`)

- The integration tests are one binary, `tests/it/main.rs`, so `cargo test` links it once: a
  module per subject (`tests/it/<subject>.rs`) and the harness in `tests/it/common/`. A new
  subject is a file there and its `mod` line in `main.rs` (`every_file_here_is_a_module` checks
  it); `cargo test --test it <subject>::` runs one.
- **Golden files** live in `tests/golden/<fixture>.<what>.{txt,json}` and are compared with
  `assert_golden(name, actual)`. `MEADA_BLESS=1 cargo test` rewrites them; the diff is the
  review, read per section with `cargo run --example golden_diff` (`--range` picks the
  revisions; the default is `origin/main...HEAD`). `.gitattributes` marks fixtures and goldens `-text`, since BLAKE3 hashes and byte
  ranges are in them, and `CHANGELOG.md merge=union` (`docs::changelog_union_merge_keeps_both_bullets`), so same-heading entries from two branches merge without a conflict. An export golden of a test about something else goes through
  `without_processes(export)`, which drops `processes`: only the chain's own exports
  (`chain-{web,gateway,catalog,worker}.export.json`), the conformance export and the
  `*.processes.json` goldens carry them (spec #31), so a change to the process walk re-blesses
  those alone.
- Test files by subject, the fixtures each uses and the loopback decider stub: [map/tests.md](../tests.md),
  split into parts by subject.
- A new behavior gets a fixture repo plus a golden; reuse a fixture before adding one. Fixture
  `meada.toml` files declare the peers and `serve.hosts`.
