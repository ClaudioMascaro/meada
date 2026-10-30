#!/bin/sh
set -eu
# The fetch of a refresh under [oidc.github] (ADR 0016): it reads the entries that
# `meada workspace show --pulled` prints, which leave out the additions whose adder holds no
# accepted role, and fetches each mirror. A failed fetch stops the refresh before index and
# publish (#316). Usage: fetch-pulled.sh <file written by `meada workspace show --pulled`>.
list=${1:?Give the file written by meada workspace show --pulled}
pairs=$(awk '
  /^\[\[repository\]\]/ { if (repo != "" || path != "") print repo " " path; repo = ""; path = "" }
  /^repo = "/ { repo = $0; sub(/^repo = "/, "", repo); sub(/"$/, "", repo) }
  /^path = "/ { path = $0; sub(/^path = "/, "", path); sub(/"$/, "", path) }
  END { if (repo != "" || path != "") print repo " " path }
' "$list")
[ -n "$pairs" ] || exit 0
while read -r identity path; do
  case "/$identity/" in
    *//*|*/../*|*/./*) echo 'Invalid repository path' >&2; exit 1 ;;
  esac
  if [ "$path" != "/srv/mirrors/$identity" ]; then
    echo "refresh: the path of $identity must be /srv/mirrors/$identity" >&2
    exit 1
  fi
  remote=https://$identity.git
  if [ -d "$path/.git" ]; then
    git -c "safe.directory=$path" -C "$path" fetch --prune origin HEAD &&
      git -c "safe.directory=$path" -C "$path" reset --hard FETCH_HEAD
  else
    git clone -- "$remote" "$path"
  fi || {
    echo "refresh: fetch failed for $identity; the refresh stops before index and publish" >&2
    exit 1
  }
done <<EOF
$pairs
EOF
