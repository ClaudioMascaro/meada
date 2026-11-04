#!/bin/sh
# A8 peer workspaces (spec #683, ticket #684): neutral sibling repositories under example.com/acme.
#
#   a8-peers.sh gen DIR                    write the workspaces into DIR (new or empty)
#   a8-peers.sh check BASE_BIN HEAD_BIN RUN
#
# `gen` writes the peers (`api` and `other` both serve GET /orders, `inventory` serves
# GET /clusters) and one consumer repository per workspace (g8, g9, g10a to g10g), each from
# heredocs, each `git init` with an origin under https://example.com/acme/ and one commit at fixed
# dates. `check` generates one copy per binary (RUN/base, RUN/head), indexes and exports every
# consumer with its binary, compares each case of a8-cases.tsv with its answer key, and
# everything else of each export (the controls and pending cases included) with the base's, and
# writes RUN/peers.md.
# Needs only meada (the two binaries), git and jq; no network.
# Exit 0: no difference, 1: differences, 2: a run failed (BLOCKED).
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
export GIT_AUTHOR_NAME=prove-meada GIT_AUTHOR_EMAIL=prove@example.com GIT_AUTHOR_DATE=2026-01-01T00:00:00Z
export GIT_COMMITTER_NAME=prove-meada GIT_COMMITTER_EMAIL=prove@example.com GIT_COMMITTER_DATE=2026-01-01T00:00:00Z

die() { echo "a8-peers: $*" >&2; exit 2; }

mk() { # <dir>: git init, origin example.com/acme/<dir>, one commit
  (cd "$1" && git init -q -b main && git remote add origin "https://example.com/acme/$1.git" \
    && git add -A && git commit -qm init) || die "git init of $1 failed"
}

consumer() { # <dir>: a consumer with the three peers, API_URL declared as api's base_url_env
  mkdir -p "$1"
  cat > "$1/meada.toml" <<'EOF'
[[peer]]
repo = "example.com/acme/api"
path = "../api"
base_url_env = ["API_URL"]

[[peer]]
repo = "example.com/acme/other"
path = "../other"

[[peer]]
repo = "example.com/acme/inventory"
path = "../inventory"
EOF
  printf '{ "name": "%s", "version": "1.0.0", "dependencies": { "@backstage/config": "^1.0.0", "@backstage/backend-plugin-api": "^1.0.0" } }\n' "$1" > "$1/package.json"
}

gen_peers() {
  for p in api other; do
    mkdir -p "$p"
    printf 'module example.com/acme/%s\n\ngo 1.22\n' "$p" > "$p/go.mod"
    printf '[serve]\n' > "$p/meada.toml"
    cat > "$p/main.go" <<'EOF'
package main

import "net/http"

func main() {
	mux := http.NewServeMux()
	mux.HandleFunc("GET /orders", orders)
	http.ListenAndServe(":8080", mux)
}

func orders(w http.ResponseWriter, r *http.Request) {}
EOF
    mk "$p"
  done
  mkdir -p inventory/cmd
  printf 'module example.com/acme/inventory\n\ngo 1.22\n\nrequire github.com/gin-gonic/gin v1.10.0\n' > inventory/go.mod
  printf '[serve]\n' > inventory/meada.toml
  cat > inventory/cmd/main.go <<'EOF'
package main

import "github.com/gin-gonic/gin"

func main() {
	r := gin.New()
	r.GET("/clusters", list)
	r.Run()
}

func list(c *gin.Context) {}
EOF
  mk inventory
}

gen_g8() { # TS URL builders: new URL(..).toString(), .href
  consumer g8; mkdir -p g8/src
  cat > g8/src/client.ts <<'EOF'
// control: template string straight into fetch
export async function ordersTpl(base: string) {
  return fetch(`${base}/orders`);
}
// the same URL through new URL(...).toString(); searchParams only touches the query
export async function ordersUrl(base: string) {
  const url = new URL(`${base}/orders`);
  url.searchParams.append('q', 'x');
  return fetch(url.toString());
}
export async function ordersUrlEnv() {
  const url = new URL(`${process.env.API_URL}/orders`);
  return fetch(url.toString());
}
export async function ordersHref(base: string) {
  return fetch(new URL(`${base}/orders`).href);
}
EOF
  mk g8
}

gen_g9() { # a base URL from the environment kept in a Go struct field or a TS class field
  consumer g9; mkdir -p g9/gocli g9/src
  printf 'module example.com/acme/g9\n\ngo 1.22\n\nrequire github.com/ilyakaznacheev/cleanenv v1.5.0\n' > g9/go.mod
  printf 'acme:\n  api:\n    baseUrl: ${API_URL}\n' > g9/app-config.yaml
  cat > g9/gocli/client.go <<'EOF'
package gocli

import (
	"net/http"
	"os"
	"strings"

	"github.com/ilyakaznacheev/cleanenv"
)

type Config struct {
	API struct {
		URL string `env:"API_URL"`
	}
}

// control: tag read and call in the same function (#618)
func OrdersInline() (*http.Response, error) {
	var cfg Config
	_ = cleanenv.ReadEnv(&cfg)
	return http.Get(cfg.API.URL + "/orders")
}

type Client struct{ base string }

func New(base string) *Client { return &Client{base: base} }

// the only two constructors: both fill base from API_URL
func FromTag() *Client {
	var cfg Config
	_ = cleanenv.ReadEnv(&cfg)
	return New(cfg.API.URL)
}

func FromGetenv() *Client { return New(strings.TrimRight(os.Getenv("API_URL"), "/")) }

func (c *Client) send(method, path string) (*http.Response, error) {
	req, err := http.NewRequest(method, c.base+path, nil)
	if err != nil {
		return nil, err
	}
	return http.DefaultClient.Do(req)
}

// base comes from the field
func (c *Client) OrdersDirect() (*http.Response, error) { return http.Get(c.base + "/orders") }
func (c *Client) Orders() (*http.Response, error)       { return c.send("GET", "/orders") }
EOF
  cat > g9/src/client.ts <<'EOF'
import { Config } from '@backstage/config';

// control: config read and call in the same function (#617)
export async function ordersInline(config: Config) {
  const base = config.getString('acme.api.baseUrl');
  return fetch(`${base}/orders`);
}

// the same read stored in a field by the constructor
export class OrdersClient {
  private readonly baseUrl: string;
  constructor(config: Config) {
    this.baseUrl = config.getString('acme.api.baseUrl');
  }
  async list() {
    return fetch(`${this.baseUrl}/orders`);
  }
}
EOF
  mk g9
}

# Backstage app-config: $include and the legacy proxy shape. One consumer per config shape.
ts_cfg() {
  cat <<'EOF'
import { Config } from '@backstage/config';
export async function ordersConfig(config: Config) {
  const base = config.getString('acme.api.baseUrl');
  return fetch(`${base}/orders`);
}
EOF
}
ts_proxy() {
  cat <<'EOF'
import { DiscoveryService } from '@backstage/backend-plugin-api';
export async function listClusters(discovery: DiscoveryService) {
  const proxy = await discovery.getBaseUrl('proxy');
  return (await fetch(`${proxy}/inventory/clusters`)).json();
}
EOF
}
backend() { printf 'backend:\n  baseUrl: http://localhost:7007\n'; }
endpoint() { printf "%s'/inventory':\n%s  target: \${INVENTORY_URL}\n" "$1" "$1"; } # <indent>

g10() { # <dir> <ts writer> <config writer>
  consumer "$1"; mkdir -p "$1/src" "$1/config/partials"
  $2 > "$1/src/client.ts"
  $3 "$1"
  mk "$1"
}
cfg_inline() { { backend; printf 'acme:\n  api:\n    baseUrl: ${API_URL}\n'; } > "$1/config/app-config.yaml"; }
cfg_include() {
  { backend; printf 'acme:\n  api: { $include: ./partials/acme.yaml#api }\n'; } > "$1/config/app-config.yaml"
  printf 'api:\n  baseUrl: ${API_URL}\n' > "$1/config/partials/acme.yaml"
}
cfg_include_whole() {
  { backend; printf 'acme: { $include: ./partials/acme.yaml }\n'; } > "$1/config/app-config.yaml"
  printf 'api:\n  baseUrl: ${API_URL}\n' > "$1/config/partials/acme.yaml"
}
px_endpoints() { { backend; printf 'proxy:\n  endpoints:\n'; endpoint '    '; } > "$1/config/app-config.yaml"; }
px_legacy() { { backend; printf 'proxy:\n'; endpoint '  '; } > "$1/config/app-config.yaml"; }
px_inc_ep() {
  { backend; printf 'proxy: { $include: ./partials/proxy.yaml }\n'; } > "$1/config/app-config.yaml"
  { printf 'endpoints:\n'; endpoint '  '; } > "$1/config/partials/proxy.yaml"
}
px_inc_legacy() {
  { backend; printf 'proxy: { $include: ./partials/proxy.yaml }\n'; } > "$1/config/app-config.yaml"
  endpoint '' > "$1/config/partials/proxy.yaml"
}

gen() { # <dir>
  mkdir -p "$1"
  [ -z "$(ls -A "$1")" ] || die "$1 is not empty"
  (cd "$1"
   gen_peers; gen_g8; gen_g9
   g10 g10a ts_cfg cfg_inline; g10 g10b ts_cfg cfg_include; g10 g10c ts_cfg cfg_include_whole
   g10 g10d ts_proxy px_endpoints; g10 g10e ts_proxy px_legacy
   g10 g10f ts_proxy px_inc_ep; g10 g10g ts_proxy px_inc_legacy)
}

# The workspaces each case reads, in the order of a8-cases.tsv.
workspaces() { grep -v '^#' "$HERE/a8-cases.tsv" | cut -f1 | awk 'NF && !seen[$0]++'; }

index_all() { # <bin> <dir>: index and export every workspace of the cases
  for w in $(workspaces); do
    (cd "$2/$w" && "$1" index > index.out 2> index.err && "$1" export > export.json 2> export.err) \
      || die "$1 failed on $2/$w: $(cat "$2/$w"/*.err)"
    [ "$(jq '.symbols | length' "$2/$w/export.json")" -gt 0 ] || die "$2/$w/export.json has no symbol"
  done
}

# The consumer field of function <fn> (the third part of its site), or "no consumer".
field() { # <export> <fn> <field>
  jq -r --arg f "$2" --arg k "$3" \
    '[.consumers[] | select((.site | split("::")[2]) == $f) | .[$k] // "null"] | first // "no consumer"' "$1"
}

# The links of the consumers of <fn>, one per line: provider repository, level, reason.
links_of() { # <export> <fn>
  jq -r --arg f "$2" '[.links[] | select((.consumer | split("::")[2]) == $f)
    | "\(.provider | split("::")[0]) \(.level) \(.reason // "")"] | unique[]' "$1"
}

# The export without the checkout facts and without the consumers and links of the given
# functions (the landed cases, which answer to the spec, not to the base).
rest() { # <export> <fn,fn,...>
  jq -S --arg fns "$2" '($fns | split(",")) as $skip
    | def mine(s): (s | split("::")[2]) as $f | $skip | index($f) != null;
    del(.commit, .dirty)
    | .consumers |= map(select(mine(.site) | not))
    | .links |= map(select(mine(.consumer) | not))' "$1"
}

check() { # <base bin> <head bin> <run>
  base=$1 head=$2 run=$3
  for b in "$base" "$head"; do [ -f "$b" ] && [ -x "$b" ] || die "no binary at $b"; done
  # absolute: each index runs inside its workspace
  base=$(cd "$(dirname "$base")" && pwd)/$(basename "$base")
  head=$(cd "$(dirname "$head")" && pwd)/$(basename "$head")
  for c in git jq; do command -v "$c" >/dev/null || die "missing: $c"; done
  mkdir -p "$run"; run=$(cd "$run" && pwd)
  [ ! -e "$run/base" ] && [ ! -e "$run/head" ] || die "$run already holds base/ or head/: use a new run folder"
  gen "$run/base"; gen "$run/head"
  index_all "$base" "$run/base"; index_all "$head" "$run/head"
  out=$run/peers.md diffs=0 pending=0
  {
    echo "# A8 peer workspaces"
    echo
    echo "base: $("$base" --version) ($base)"
    echo "head: $("$head" --version) ($head)"
    echo
    echo "| Result | Workspace | Case | Consumer | Field | Answer key | Base | Head | 1.2.0 | Links (head) |"
    echo "|---|---|---|---|---|---|---|---|---|---|"
  } > "$out"
  tab=$(printf '\t')
  grep -v '^#' "$HERE/a8-cases.tsv" | grep . > "$run/cases.tsv"
  # eight non-empty columns: `read` with a tab IFS would merge an empty one into the next
  awk -F"$tab" 'NF != 8 { exit 1 } { for (i = 1; i <= 8; i++) if ($i == "") exit 1 }' "$run/cases.tsv" \
    || die "a8-cases.tsv: every row needs eight non-empty tab-separated columns"
  while IFS="$tab" read -r w fn k want state link v120 name; do
    b=$(field "$run/base/$w/export.json" "$fn" "$k") || die "jq failed on base/$w"
    h=$(field "$run/head/$w/export.json" "$fn" "$k") || die "jq failed on head/$w"
    hl=$(links_of "$run/head/$w/export.json" "$fn") || die "jq failed on head/$w"
    bl=$(links_of "$run/base/$w/export.json" "$fn") || die "jq failed on base/$w"
    # link: `-` names none; else the one link: "<provider repository> <level or *> <reason text>"
    lok=yes
    if [ "$link" != - ]; then
      repo=${link%% *} rest=${link#* }; lvl=${rest%% *} why=${rest#* }
      [ "$(printf '%s\n' "$hl" | grep -c .)" -eq 1 ] || lok=no
      # shellcheck disable=SC2254 # $lvl is a pattern on purpose: `*` takes any level
      case $hl in "$repo "$lvl" "*"$why"*) ;; *) lok=no ;; esac
    fi
    case $state in
      control) # the answer key and the base, field and links
        if [ "$h" = "$want" ] && [ "$h" = "$b" ] && [ "$hl" = "$bl" ] && [ $lok = yes ]; then r=PASS; else r=FAIL; fi ;;
      landed) # the spec
        if [ "$h" = "$want" ] && [ $lok = yes ]; then r=PASS; else r=FAIL; fi ;;
      pending) # the spec once its ticket lands; until then it reads as 1.2.0 and the base, not counted
        if [ "$h" = "$want" ] && [ $lok = yes ]; then r="MEETS SPEC: mark it landed"
        elif [ "$h" = "$v120" ] && [ "$h" = "$b" ] && [ "$hl" = "$bl" ]; then r="PENDING (as 1.2.0, not counted)"; pending=$((pending + 1))
        else r=FAIL; fi ;;
      *) die "case $name: unknown state $state" ;;
    esac
    case $r in FAIL|MEETS*) diffs=$((diffs + 1)) ;; esac
    printf '| %s | %s | %s | `%s` | %s | `%s` | `%s` | `%s` | `%s` | %s (want: %s) |\n' \
      "$r" "$w" "$name" "$fn" "$k" "$want" "$b" "$h" "$v120" \
      "$(echo "$hl" | tr '\n' ';' | sed 's/;$//; s/|/\\|/g')" "$link" >> "$out"
  done < "$run/cases.tsv"
  {
    echo
    echo "## Everything else against the base"
    echo
    echo "Each export without \`commit\`, \`dirty\` and the consumers and links of the landed cases"
    echo "(those answer to the spec above); the controls and pending cases stay in."
    echo
  } >> "$out"
  for w in $(workspaces); do
    skip=$(awk -F"$tab" -v w="$w" '$1 == w && $5 == "landed" { printf "%s%s", s, $2; s = "," }' "$run/cases.tsv")
    rest "$run/base/$w/export.json" "$skip" > "$run/base/$w/rest.json" || die "jq failed on base/$w"
    rest "$run/head/$w/export.json" "$skip" > "$run/head/$w/rest.json" || die "jq failed on head/$w"
    if cmp -s "$run/base/$w/rest.json" "$run/head/$w/rest.json"; then
      echo "- $w: equal to base" >> "$out"
    else
      diffs=$((diffs + 1))
      { echo "- $w: DIFFERS from base"; echo; echo '```diff'
        diff -u "$run/base/$w/rest.json" "$run/head/$w/rest.json" | head -60 || true
        echo '```'; } >> "$out"
    fi
  done
  { echo; echo "pending (not counted): $pending; differences: $diffs"; } >> "$out"
  if [ $diffs -eq 0 ]; then echo "checks passed ($pending pending, not counted): $out"; else echo "differences found ($diffs): $out"; exit 1; fi
}

case ${1:-} in
  gen) [ $# -eq 2 ] || die "usage: a8-peers.sh gen DIR"; gen "$2" ;;
  check) [ $# -eq 4 ] || die "usage: a8-peers.sh check BASE_BIN HEAD_BIN RUN"; check "$2" "$3" "$4" ;;
  *) die "usage: a8-peers.sh gen DIR | check BASE_BIN HEAD_BIN RUN" ;;
esac
