#!/bin/sh
set -eu
bootstrap=/etc/meada/meada.workspace.toml
export LANG=C
if meada workspace show --workspace "$bootstrap" >/dev/null 2>/tmp/show.err; then
  exit 0
fi
grep -q 'no workspace version is current' /tmp/show.err || { cat /tmp/show.err >&2; exit 1; }
version=$(meada workspace import /etc/meada/repositories.toml --message "Initial managed workspace" --workspace "$bootstrap")
meada workspace apply "$version" --workspace "$bootstrap"
