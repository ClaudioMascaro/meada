#!/bin/sh
set -eu
# The caller, index-loop.sh, holds one snapshot lock for this entire sequence.
bootstrap=/etc/meada/meada.workspace.toml
meada workspace pull --workspace "$bootstrap"
if grep -q '^[[:space:]]*\[oidc\.github\]' "$bootstrap"; then
  # The role check stops the refresh when it fails; the fetch then follows the entries it left in.
  meada github-app roles --workspace "$bootstrap"
  meada workspace show --pulled --workspace "$bootstrap" > /tmp/pulled.toml
  sh /etc/meada/fetch-pulled.sh /tmp/pulled.toml
else
  sh /etc/meada/fetch.sh
fi
meada index --workspace "$bootstrap"
exec meada snapshot publish --workspace "$bootstrap"
