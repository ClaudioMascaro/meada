#!/bin/sh
set -eu
until meada snapshot pull --workspace /etc/meada/meada.workspace.toml; do
  sleep 2
done
