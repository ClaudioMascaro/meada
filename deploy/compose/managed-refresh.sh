#!/bin/sh
set -eu
bootstrap=/etc/meada/meada.workspace.toml
meada workspace pull --workspace "$bootstrap"
if grep -q '^[[:space:]]*\[oidc\.github\]' "$bootstrap"; then
  meada github-app roles --workspace "$bootstrap"
  meada workspace show --pulled --workspace "$bootstrap" > /tmp/pulled.toml
  sh /etc/meada/fetch-pulled.sh /tmp/pulled.toml
else
  sh /etc/meada/fetch.sh
fi
meada index --workspace "$bootstrap"
exec meada snapshot publish --workspace "$bootstrap"
