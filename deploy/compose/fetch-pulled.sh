#!/bin/sh
set -eu
# The fetch of a refresh under [oidc.github] (ADR 0016): it reads the entries that
# `meada workspace show --pulled` prints, which leave out the additions whose adder holds no
# accepted role, and fetches each mirror. A failed fetch stops the refresh before index and
# publish (#316). Usage: fetch-pulled.sh <file written by `meada workspace show --pulled`>.
list=${1:?Give the file written by meada workspace show --pulled}
pairs=$(awk '
  # Reads the keys of each [[repository]] table itself: any table header ends the entry before
  # it, so a [[repository.peer]] or [[repository.resource]] table adds nothing. A value is the text
  # between its quotes. Basic strings only: an identity and a mirror path carry no literal string,
  # multi-line string or escape, and no multi-line array with a line that starts with [. The same
  # program is in deploy/compose/fetch-pulled.sh and the chart workspace.fetch (values.yaml); a
  # test fails when the two drift (#544).
  function value(line) { sub(/^[^"]*"/, "", line); sub(/".*$/, "", line); return line }
  function flush() { if (inrepo && (repo != "" || path != "")) print repo " " path; repo = ""; path = "" }
  /^[ \t]*\[/ { flush(); inrepo = ($0 ~ /^[ \t]*\[\[[ \t]*repository[ \t]*\]\]/); next }
  inrepo && /^[ \t]*repo[ \t]*=[ \t]*"/ { repo = value($0) }
  inrepo && /^[ \t]*path[ \t]*=[ \t]*"/ { path = value($0) }
  END { flush() }
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
    origin=$(git -c "safe.directory=$path" -C "$path" remote get-url origin 2>/dev/null || true)
    case "$origin" in
      https://*|http://*)
        host=${origin#*://}
        case "${host%%/*}" in
          *@*) echo "refresh: the origin of $identity takes a URL without credentials; give git a credential helper" >&2; exit 1 ;;
        esac ;;
    esac
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
