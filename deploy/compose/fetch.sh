#!/bin/sh
set -eu
# A failed fetch stops the refresh before `meada index` (#316): the job publishes nothing and
# the nodes keep the current generation. Its line names the identity; no URL carries a credential.
for remote in ${REPOSITORIES:?Set REPOSITORIES}; do
  case "$remote" in
    https://*|http://*) identity=${remote#*://}; identity=${identity%.git} ;;
    *) echo 'REPOSITORIES accepts HTTP(S) git URLs' >&2; exit 1 ;;
  esac
  case "${identity%%/*}" in
    *@*) echo 'REPOSITORIES takes URLs without credentials; give git a credential helper' >&2; exit 1 ;;
  esac
  case "/$identity/" in
    *//*|*/../*|*/./*) echo 'Invalid repository path' >&2; exit 1 ;;
  esac
  mirror=/srv/mirrors/$identity
  if [ -d "$mirror/.git" ]; then
    git -c "safe.directory=$mirror" -C "$mirror" fetch --prune origin HEAD &&
      git -c "safe.directory=$mirror" -C "$mirror" reset --hard FETCH_HEAD
  else
    git clone -- "$remote" "$mirror"
  fi || {
    echo "refresh: fetch failed for $identity; the refresh stops before index and publish" >&2
    exit 1
  }
done
