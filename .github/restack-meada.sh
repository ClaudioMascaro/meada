#!/usr/bin/env bash
set -euo pipefail
git config user.name 'Meada Builder'
git config user.email '41898282+github-actions[bot]@users.noreply.github.com'

main=ccfa0c7ac3c064c1b38e0c14d299a9d7ddd9785e
old=(e4058ebc17f97f0ae08adfd89374be159fe11040 e0925fe4463ea44b6d1a928509fc68c0d8c14b81 5d322753da33274f14facb53996f5f4f2caabac5 69f68791ede7dc14fc7167af109d6aabcfdebe25 d2bc5f745a6daee9b08efe84e4e453009a351de7 def34e4f887e8d97ebe02d4cca2aee3cfc8474aa)
prs=(941 943 944 947 959 960)
branches=(fix/decide-linked-decisions-file fix/go-literal-key-import fix/status-unevaluated-decisions fix/index-and-resolution-stability fix/http-peer-authority fix/http-peer-proof-integration)

mkdir -p "$RUNNER_TEMP/restack"
out="$RUNNER_TEMP/restack"
exec > >(tee "$out/prepare.log") 2>&1
git fetch --no-tags origin refs/heads/main:refs/remotes/origin/main
test "$(git rev-parse refs/remotes/origin/main)" = "$main"
for i in "${!prs[@]}"; do
  git fetch --no-tags origin "refs/pull/${prs[i]}/head:refs/remotes/origin/restack-pr-${prs[i]}"
  branch_sha=$(git ls-remote origin "refs/heads/${branches[i]}" | cut -f1)
  test "$branch_sha" = "${old[i]}"
  test "$(git rev-parse "refs/remotes/origin/restack-pr-${prs[i]}")" = "${old[i]}"
  printf 'BEFORE #%s %s %s\n' "${prs[i]}" "${branches[i]}" "$branch_sha"
done
git checkout --detach "$main"
pick() { git -c rerere.enabled=false cherry-pick -x "$@"; }
save() {
  local pr=$1 base=$2
  git diff --check "$base" HEAD
  git diff --stat "$base" HEAD > "$out/pr-$pr.stat"
  git log --format='%H %P %s' "$base"..HEAD > "$out/pr-$pr.log"
  printf 'CANDIDATE #%s %s %s %s\n' "$pr" "$base" "$(git rev-parse HEAD)" "$(git rev-parse HEAD^{tree})"     | tee -a "$out/candidates.txt"
  git push origin "HEAD:refs/heads/verify/restack-pr-$pr-20261121"
}
trap 'git status --short; git diff --cc' ERR
pick 0d57810dd3d4bd3a16a2dc366ae6c7e20d508791.."${old[0]}"
pick 230f9cdf6f3b7f8153eb5ee0a4ed92c94388dcc7
save 941 "$main"
pr941=$(git rev-parse HEAD)

pick "${old[0]}..${old[1]}"
fix=8f780c50528fb753a4e0c32cb5741414b76d635b
git show --format= "$fix" -- docs/adr/0004-index-storage-and-freshness.md tests/golden/schema.txt tests/it/freshness.rs | git apply --index
git show --format= "$fix" -- src/index.rs | python3 -c '
import re, sys
parts = re.split(r"(?m)(?=^@@ )", sys.stdin.read())
assert len(parts) == 5, len(parts)
assert "EXTRACTION_REVISION" in parts[1] and "EXTRACTION_REVISION" in parts[4]
print(parts[0] + parts[1] + parts[4], end="")
' | git apply --index
git commit -m "fix(index): rebuild indexes with stale Go extraction (#946)" -m "Extracted from $fix."
save 943 "$pr941"
pr943=$(git rev-parse HEAD)

pick "${old[1]}..${old[2]}"
git show --format= "$fix" -- src/engine.rs tests/it/manifest.rs | git apply --index
git show --format= "$fix" -- src/index.rs | python3 -c '
import re, sys
parts = re.split(r"(?m)(?=^@@ )", sys.stdin.read())
assert len(parts) == 5, len(parts)
assert "BEGIN DEFERRED" in parts[2] and "format!" in parts[3]
print(parts[0] + parts[2] + parts[3], end="")
' | git apply --index
git commit -m "fix(status): identify the repository behind an unreadable index (#934)" -m "Extracted from $fix."
save 944 "$pr943"
pr944=$(git rev-parse HEAD)

pick 13a3374eb5b0ff475936ab0776d070b29ca87450^..359ba10ab971abdc27cda326496f93c45ecf9bf3
if ! pick "${old[3]}"; then
  conflicts=$(git diff --name-only --diff-filter=U)
  for path in $conflicts; do
    case "$path" in
      scripts/check|scripts/prove-ci)
        git checkout --theirs -- "$path"
        git add "$path"
        ;;
      *) printf 'Unexpected harness conflict: %s\n' "$path"; exit 1 ;;
    esac
  done
  git cherry-pick --continue
fi
save 947 "$pr944"
pr947=$(git rev-parse HEAD)

pick 359ba10ab971abdc27cda326496f93c45ecf9bf3.."${old[4]}"
save 959 "$pr947"
git diff --name-only "$pr947" HEAD > "$out/pr-959-product-paths.txt"
git diff --name-only "${old[3]}" "${old[5]}" > "$out/pr-960-historical-product-paths.txt"
diff -u "$out/pr-960-historical-product-paths.txt" "$out/pr-959-product-paths.txt"
git merge-tree --write-tree "$pr947" HEAD | tee "$out/pr-960-candidate-tree.txt"
test "$(cat "$out/pr-960-candidate-tree.txt")" = "$(git rev-parse HEAD^{tree})"
git diff "${old[5]}" HEAD --stat > "$out/pr-960-restack.stat"
