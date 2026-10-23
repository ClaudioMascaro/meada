#!/bin/sh
set -eu
for remote in ${REPOSITORIES:?Set REPOSITORIES}; do
  case "$remote" in
    https://*|http://*) identity=${remote#*://}; identity=${identity%.git} ;;
    *) echo 'REPOSITORIES accepts HTTP(S) git URLs' >&2; exit 1 ;;
  esac
  case "/$identity/" in
    *//*|*/../*|*/./*) echo 'Invalid repository path' >&2; exit 1 ;;
  esac
  mirror=/srv/mirrors/$identity
  if [ -d "$mirror/.git" ]; then
    git -c "safe.directory=$mirror" -C "$mirror" fetch --prune origin HEAD
    git -c "safe.directory=$mirror" -C "$mirror" reset --hard FETCH_HEAD
  else
    git clone -- "$remote" "$mirror"
  fi
done
