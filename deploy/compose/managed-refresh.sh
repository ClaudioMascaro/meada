#!/bin/sh
set -eu
# The caller, index-loop.sh, holds one snapshot lock for this entire sequence.
meada workspace pull --workspace /etc/meada/meada.workspace.toml
sh /etc/meada/fetch.sh
meada index --workspace /etc/meada/meada.workspace.toml
exec meada snapshot publish --workspace /etc/meada/meada.workspace.toml
