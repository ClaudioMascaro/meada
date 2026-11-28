#!/bin/sh
set -eu
list=${1:?Give the file written by meada workspace show --pulled}
pairs=$(awk '
  function value(line) { sub(/^[^=]*=[ \t]*"/, "", line); sub(/".*$/, "", line); return line }
  function flush() { if (inrepo && (repo != "" || path != "")) print repo " " path; repo = ""; path = "" }
  /^[ \t]*\[/ { flush(); inrepo = ($0 ~ /^[ \t]*\[\[[ \t]*repository[ \t]*\]\]/); next }
  inrepo && /^[ \t]*"?repo"?[ \t]*=[ \t]*"/ { repo = value($0) }
  inrepo && /^[ \t]*"?path"?[ \t]*=[ \t]*"/ { path = value($0) }
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
