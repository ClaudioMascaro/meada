# Console and its API

The console is `meada ui --listen <addr> --workspace <bootstrap manifest>` (feature `ui`, ADR
0014), serving the built front end of `ui/` and `/api/v1/`. Without `[accounts]` or `[oidc]` it
admits every request as a viewer that reads everything: the case of a console behind the
operator's proxy. `console.py` drives that mode, and with `--accounts` the local accounts of ADR
0024 over Postgres.

It reuses the repositories, the queries and the head's stdio answers of a `prove.py run` in the
same `$RUN`: run that first.

## Build

```bash
$S/build.sh head-ui . "$RUN" ui,accounts
rustup target add wasm32-unknown-unknown
command -v wasm-pack || {  # the version and sha256 CI's frontend job pins
  a=$(mktemp -d)/wasm-pack.tar.gz
  curl -sSfL -o "$a" https://github.com/wasm-bindgen/wasm-pack/releases/download/v0.15.0/wasm-pack-v0.15.0-x86_64-unknown-linux-musl.tar.gz
  echo "c09f971ecaed9a2efc80fdcea7a00ef6b53c7fadc8c57d1f61b53a6aa66b668a  $a" | sha256sum -c -
  tar -xzf "$a" -C "$(dirname "$a")" && install -m 755 "$(find "$(dirname "$a")" -type f -name wasm-pack)" ~/.cargo/bin/
}
(cd ui && npm ci && npm run wasm && npm run build && npm run check:version)
```

Ready when: `ui/dist/meada-ui.json` holds this workspace's version (`npm run check:version`
passes) and `"$RUN/bin/meada-head-ui" ui --help` lists `--listen`.

## API

```bash
ENTRY=meada  # repeat the API check for each corpus entry Scope names
python3 $S/console.py start --bin "$RUN/bin/meada-head-ui" --run "$RUN" --ui-dir ui/dist --repos "$ENTRY"
python3 $S/console.py api --run "$RUN"
python3 $S/console.py stop --run "$RUN"
mv "$RUN/console" "$RUN/console-$ENTRY"  # retain captures before starting the next entry
```

`start` writes `$RUN/console/meada.workspace.toml` (the selected head entries, all by default, `[ui]` on a free
loopback port), runs `meada index --workspace`, starts the console detached and waits for
`GET /api/v1/config`. `api` then sends every stdio query of the run to `POST /api/v1/query`
(`{"repository", "generation": null, "tool", "arguments"}`, with the `Origin` the console
requires of a write) and compares successful `text` byte for byte with the stdio answer.
API parity requires one corpus entry per manifest: standalone stdio answers about that entry,
whereas a manifest of several entries makes one workspace and changes search/context scope.
For an expected MCP tool error it requires HTTP 422, code `tool_error` and the exact same full
message, the existing API protocol. A wrong status, code or message fails. Every raw status/body
is saved under `responses/`; errors are reported separately from successful text-byte parity.

`GET /api/v1/repositories/{identity}/export` is compared byte for byte with the same console
binary's `export --workspace <that manifest> --repo <that identity>`. Its complete stdout,
stderr and exit status are saved; no fields are removed. A local checkout export may carry
`commit`/`dirty` that server mode omits, so comparing across modes is not an export answer key.
Report in
`$RUN/console/api.md`.

With sign-in in Scope (`src/accounts*`, `src/postgres.rs`, `src/audit.rs`, `ui/src/Account*`,
`ui/src/local.ts`):

```bash
export MEADA_TEST_POSTGRES_URL=postgres://...   # a Postgres 15+ the run may create a schema in
python3 $S/console.py start --bin "$RUN/bin/meada-head-ui" --run "$RUN" --ui-dir ui/dist --repos "$ENTRY" --accounts
python3 $S/console.py api --run "$RUN"
```

That adds `[postgres]` (schema `zz_prove_<run>`) and `[accounts]` with an Ed25519 pair from
`openssl` and an `admin` password derived from that key (never written), then checks: a wrong password refused (401 or 403),
a login without `Origin` refused (403), `admin` signed in with a token, `/api/v1/session` with it,
`/api/v1/manifest` without it refused (401), and the query and export parity under the token.
Without `MEADA_TEST_POSTGRES_URL` the start is BLOCKED: never stub Postgres.

## Pages

After scoped API checks, start the normal viewer for page checks (without `--repos`):

```bash
python3 $S/console.py start --bin "$RUN/bin/meada-head-ui" --run "$RUN" --ui-dir ui/dist
```

```bash
CACHE=${PROVE_MEADA_CACHE:-$HOME/.cache/prove-meada}
npm install --no-save --prefix "$CACHE/node" playwright-core@1.61.1 axe-core@4.12.1
ls -d "${PLAYWRIGHT_BROWSERS_PATH:-$HOME/.cache/ms-playwright}"/chromium-* 2>/dev/null \
  || PLAYWRIGHT_BROWSERS_PATH="$CACHE/browsers" node "$CACHE/node/node_modules/playwright-core/cli.js" install chromium
PROVE_NODE="$CACHE/node" PLAYWRIGHT_BROWSERS_PATH="${PLAYWRIGHT_BROWSERS_PATH:-$CACHE/browsers}" \
  node $S/console-pages.mjs "$RUN"
```

The install goes to a cache of its own, so it never deletes another project's browsers from the
shared `~/.cache/ms-playwright`; an existing Chromium there is reused through `executablePath`.

`console-pages.mjs` visits `/config`, `/session`, `/manifest`, `/playground` and `/graph` (signing
in first when the run has an admin password) and, per page, asserts an `h1`, layout shift at most
0.1, load at most 2 s, zero console errors and zero axe violations of impact serious or critical,
with a full-page screenshot in `$RUN/console/shots/`. Then it runs the first `context` query of the
run in the playground (the form's two required inputs, its first `select`, its submit button) and
compares `pre[role=region]` with the stdio answer. Report in `$RUN/console/pages.md`.

A new page or route the change adds: add its path to the list in `console-pages.mjs` and say so
in the receipt; its own behaviour answers to the issue.

## Stop

```bash
python3 $S/console.py stop --run "$RUN"
```

It stops the console by its process group, removes the console's indexes and the private key (the
admin password goes with it), and leaves the Postgres schema `zz_prove_*`: drop it by name
(`DROP SCHEMA "zz_prove_..." CASCADE`) and list it under Left behind until you do.
