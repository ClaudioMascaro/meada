#!/bin/sh
# Stop hook: a change whose diff since the merge-base reaches a surface of /prove-meada's
# Scope table is not done until a receipt under .prove-it/runs/ is newer than the last
# changed file. It blocks the stop at most twice per transcript, so a session that cannot
# prove still ends.
SKILL=prove-meada
SURFACE='^(src/|meada-core/src/|ui/|deploy/|Dockerfile|Cargo\.toml|Cargo\.lock|tests/golden/|vendor/|scripts/(compose-check|helm-check|container-smoke|ui-tarball)|\.github/workflows/(compose|helm|container)\.yml)'
MARK='prove-stop:'

input=$(cat)
cd "$(git rev-parse --show-toplevel 2>/dev/null)" || exit 0
transcript=$(printf '%s' "$input" | sed -n 's/.*"transcript_path":"\([^"]*\)".*/\1/p')
if [ -f "$transcript" ] && [ "$(grep -c "$MARK" "$transcript")" -ge 2 ]; then exit 0; fi

base=$(git merge-base HEAD origin/main 2>/dev/null || git merge-base HEAD main 2>/dev/null) || exit 0
changed=$({ git diff --name-only "$base"; git ls-files --others --exclude-standard; } | grep -E "$SURFACE" | sort -u)
[ -n "$changed" ] || exit 0

last=0
for f in $changed; do
  [ -e "$f" ] || continue
  t=$(stat -c %Y "$f")
  [ "$t" -gt "$last" ] && last=$t
done
newest=$(ls -t .prove-it/runs/*/receipt.md 2>/dev/null | head -n 1)
if [ -n "$newest" ] && [ "$(stat -c %Y "$newest")" -ge "$last" ]; then exit 0; fi

printf '{"decision":"block","reason":"%s the diff since the merge-base reaches %s and no receipt under .prove-it/runs/ is newer than it. Run /%s on this change and name its receipt in the PR body Evidence section, then stop."}\n' \
  "$MARK" "$(printf '%s' "$changed" | tr '\n' ' ' | sed 's/ $//')" "$SKILL"
