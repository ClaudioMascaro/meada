#!/bin/sh
set -eu
case "${REFRESH_SECONDS:-}" in
  ''|*[!0-9]*) echo 'REFRESH_SECONDS must be a positive integer' >&2; exit 1 ;;
esac
if ! test "$REFRESH_SECONDS" -ge 1 2>/dev/null; then
  echo 'REFRESH_SECONDS must be a positive integer' >&2
  exit 1
fi
child=
stop() {
  if test -n "$child"; then
    kill -TERM "$child" 2>/dev/null || true
    wait "$child" || true
  fi
  exit 0
}
trap stop TERM INT
while :; do
  meada snapshot lock --workspace /etc/meada/meada.workspace.toml -- \
    sh /etc/meada/refresh.sh &
  child=$!
  wait "$child" || printf 'Refresh failed; retrying after %s seconds.\n' "$REFRESH_SECONDS" >&2
  sleep "$REFRESH_SECONDS" &
  child=$!
  wait "$child" || true
done
