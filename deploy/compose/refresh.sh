#!/bin/sh
set -eu
sh /etc/meada/fetch.sh
meada index --workspace /etc/meada/meada.workspace.toml
if [ "${PUBLISH:-0}" = 1 ]; then
  exec meada snapshot publish --workspace /etc/meada/meada.workspace.toml
fi
