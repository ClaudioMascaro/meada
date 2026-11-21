#!/usr/bin/env bash
set -euo pipefail
case "$PR" in
  941) BASE=ccfa0c7ac3c064c1b38e0c14d299a9d7ddd9785e; OLD=e4058ebc17f97f0ae08adfd89374be159fe11040; OLD_RUN=44215534892 ;;
  943) BASE=e27664e23d5da1ea5dffcdb91ae49915d501b95a; OLD=e0925fe4463ea44b6d1a928509fc68c0d8c14b81; OLD_RUN=44395349090 ;;
  944) BASE=7dee48a7b7fec9695c5e1c4a4d80208bbe827671; OLD=5d322753da33274f14facb53996f5f4f2caabac5; OLD_RUN=44504741203 ;;
  947) BASE=d7fbc5b74363500b375718fdff0135b606e971d9 ;;
  959) BASE=b9d4c2315354fc348a77a3b12647fd78d92753aa ;;
  960) BASE=b9d4c2315354fc348a77a3b12647fd78d92753aa ;;
esac
RUN="$RUNNER_TEMP/restack-evidence"
mkdir -p "$RUN"
git worktree add --detach "$RUNNER_TEMP/driver" b9d4c2315354fc348a77a3b12647fd78d92753aa
P="$RUNNER_TEMP/venv/bin/python"
S="$RUNNER_TEMP/driver/.claude/skills/prove-meada/scripts"
git fetch --no-tags origin "refs/heads/verify/restack-pr-$PR-20261121"
SHA=$(git rev-parse FETCH_HEAD)
test "$(git merge-base "$BASE" "$SHA")" = "$BASE"
git branch restack-exact-base "$BASE"
git switch --create restack-exact-head "$SHA"
git config branch.restack-exact-head.remote .
git config branch.restack-exact-head.merge refs/heads/restack-exact-base
test "$(git rev-parse '@{upstream}')" = "$BASE"
printf 'PR #%s\nbase %s\nhead %s\nchecker %s\n' "$PR" "$BASE" "$SHA" "$(git -C "$RUNNER_TEMP/driver" rev-parse HEAD)" | tee "$RUN/provenance.txt"
git diff --check "$BASE" "$SHA"
case "$MODE" in
checks)
  exec > >(tee "$RUN/gate.log") 2>&1
  PYTHONPATH="$S" "$P" scripts/check --selftest
  "$P" scripts/prove-ci --selftest
  "$P" "$S/prove.py" selftest
  if [ "$PR" = 947 ] || [ "$PR" = 959 ] || [ "$PR" = 960 ]; then
    "$P" "$S/smoke_stack.py" --selftest
    for path in src/config.rs src/workspace.rs src/iac/provision.rs src/lang/python.rs src/index.rs tests/golden/schema.txt README.md; do
      printf '\nPath %s\n' "$path"
      "$P" scripts/prove-ci --rows "$path"
    done
  fi
  [ "$PR" = 941 ] || [ "$PR" = 943 ] || [ "$PR" = 944 ] || rustup target add wasm32-unknown-unknown
  "$P" scripts/check --unpushed
  ;;
features)
  exec > >(tee "$RUN/features.log") 2>&1
  scripts/clippy-features
  ;;
probe)
  exec > >(tee "$RUN/probe.log") 2>&1
  mkdir -p "$RUNNER_TEMP/binaries"
  other() {
    local revision=$1 run=$2 label=$3 folder
    folder="$RUNNER_TEMP/binaries/download-$label"
    gh run download "$run" --repo ClaudioMascaro/meada --name "meada-debug-$revision" --dir "$folder"
    find "$folder" -type f -printf '%P\n'
    local files=()
    mapfile -t files < <(find "$folder" -type f -name meada)
    test "${#files[@]}" -eq 1
    install -m755 "${files[0]}" "$RUNNER_TEMP/binaries/$label"
    printf 'commit %s, features: default, Diff run %s\n' "$revision" "$run" > "$RUNNER_TEMP/binaries/$label.from"
  }
  cargo build --locked --bin meada
  cp target/debug/meada "$RUNNER_TEMP/binaries/candidate"
  printf 'commit %s, features: default\n' "$SHA" > "$RUNNER_TEMP/binaries/candidate.from"
  other "$OLD" "$OLD_RUN" historical
  if [ "$PR" = 943 ]; then
    other e4058ebc17f97f0ae08adfd89374be159fe11040 44215534892 source
  fi
  sha256sum "$RUNNER_TEMP"/binaries/{candidate,historical}
  set +e
  "$P" "$RUNNER_TEMP/probe.py" "$PR" "$RUN" "$RUNNER_TEMP/binaries" "$S"
  status=$?
  set -e
  if [ "$status" -eq 0 ]; then
    find "$RUN" -mindepth 2 -maxdepth 2 -type d -name work -exec rm -rf {} +
  fi
  exit "$status"
  ;;
esac
