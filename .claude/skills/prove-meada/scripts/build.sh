#!/usr/bin/env bash
# Builds meada into <run>/bin/meada-<label> and records what it was built from.
#   build.sh <label> <rev> <run> [features]
# <rev> is `.` for this working tree (uncommitted changes included) or a commit, built in a
# detached worktree under <run>. Build the head first: the base build reuses the same target
# directory, so the crates.io dependencies compile once. PROVE_CARGO overrides `cargo` (a build
# queue wrapper, for example).
set -euo pipefail
label=$1 rev=$2 run=$3 features=${4:-}
root=$(git rev-parse --show-toplevel)
target=${CARGO_TARGET_DIR:-$root/target}
mkdir -p "$run/bin"
read -r -a cargo <<<"${PROVE_CARGO:-cargo}"
# Every build of a run at one optimization level, whatever each tree's Cargo.toml sets for its
# dev profile, so the time and memory budgets compare like with like.
export CARGO_PROFILE_DEV_OPT_LEVEL=1
args=(build --locked --target-dir "$target")
[ -n "$features" ] && args+=(--features "$features")
if [ "$rev" = . ]; then
  (cd "$root" && "${cargo[@]}" "${args[@]}")
  from="working tree at $(git -C "$root" rev-parse HEAD), uncommitted: $(git -C "$root" status --porcelain | grep -c . || true) path(s)"
else
  src="$run/src-$label"
  git -C "$root" worktree add -q --detach "$src" "$rev"
  trap 'git -C "$root" worktree remove --force "$src"' EXIT
  (cd "$src" && "${cargo[@]}" "${args[@]}")
  from="commit $(git -C "$root" rev-parse "$rev")"
fi
cp "$target/debug/meada" "$run/bin/meada-$label"
echo "$from, features: ${features:-default}" >"$run/bin/meada-$label.from"
echo "$run/bin/meada-$label: $("$run/bin/meada-$label" --version), $from, features: ${features:-default}"
