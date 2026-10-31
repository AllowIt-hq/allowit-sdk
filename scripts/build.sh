#!/usr/bin/env bash
# Check cross-repository pins, then build.
#
#   scripts/build.sh            fail on a stale pin, then build
#   scripts/build.sh --check    only check the pins
#   scripts/build.sh --update   rewrite stale pins and Cargo.lock, then build
#
# [workspace.metadata.allowit.track] says which revision each git repository
# must be pinned to. Keys are repository names:
#
#   repo = "head"                  the tip of the remote default branch
#   repo = { branch = "name" }     the tip of that branch
#   repo = { version = "1.2.3" }   the commit of tag v1.2.3
#
# Every git dependency needs an entry and a full `rev` in Cargo.toml.
# Packages inside this repository use path dependencies, share one version
# and are not pinned.
set -euo pipefail
cd "$(dirname "$0")/.."

mode=build
case "${1-}" in
  "") ;;
  --check) mode=check ;;
  --update) mode=update ;;
  *) echo "usage: scripts/build.sh [--check | --update]" >&2; exit 2 ;;
esac

workspaces=$(git ls-files -- '*Cargo.toml' | while read -r m; do
  if grep -q '^\[workspace\]' "$m"; then echo "$m"; fi
done)
meta=$(for m in $workspaces; do
  cargo metadata --no-deps --format-version 1 --manifest-path "$m"
done | jq -s '{packages: (map(.packages) | add), track: (map(.metadata.allowit.track // {}) | add)}')

if [ "$(jq '[.packages[].version] | unique | length' <<<"$meta")" != 1 ]; then
  echo "packages must share one version:" >&2
  jq -r '.packages[] | "  \(.name) \(.version)"' <<<"$meta" >&2
  exit 1
fi

# url, rev and package name of every git dependency.
pins=$(jq -r '
  [.packages[].dependencies[] | select((.source // "") | startswith("git+"))
   | [(.source | ltrimstr("git+") | sub("[?#].*"; "")),
      ([.source | capture("[?&]rev=(?<r>[^&#]+)").r] | first // "-"),
      .name] | @tsv] | unique[]' <<<"$meta")

failed=0
stale=""
for url in $(cut -f1 <<<"$pins" | sort -u); do
  repo=$(basename "$url" .git)
  spec=$(jq -c --arg r "$repo" '.track[$r] // empty' <<<"$meta")
  case "$spec" in
    '"head"') ref=HEAD ;;
    '{"branch":'*) ref=refs/heads/$(jq -r .branch <<<"$spec") ;;
    '{"version":'*) ref=refs/tags/v$(jq -r .version <<<"$spec") ;;
    "") echo "$repo: no entry in [workspace.metadata.allowit.track]" >&2; failed=1; continue ;;
    *) echo "$repo: unknown tracking $spec" >&2; failed=1; continue ;;
  esac
  # An annotated tag lists its commit last, as <ref>^{}.
  want=$(git ls-remote "$url" "$ref" "$ref^{}" | awk 'END { print $1 }')
  if [ -z "$want" ]; then
    echo "$repo: $ref not found at $url" >&2
    failed=1
    continue
  fi
  while IFS=$'\t' read -r _ rev name; do
    [ "$rev" = "$want" ] && continue
    echo "$name: $repo $spec is $want, pinned ${rev/#-/no rev}" >&2
    if [ "$rev" = - ]; then failed=1; else stale="$stale $rev:$want"; fi
  done < <(awk -F'\t' -v u="$url" '$1 == u' <<<"$pins")
done
[ "$failed" = 0 ] || exit 1

if [ -n "$stale" ]; then
  if [ "$mode" != update ]; then
    echo "stale pins: run scripts/build.sh --update" >&2
    exit 1
  fi
  for manifest in $(git ls-files -- '*Cargo.toml'); do
    for pair in $stale; do
      sed -i.orig "s/${pair%%:*}/${pair##*:}/g" "$manifest" && rm "$manifest.orig"
    done
  done
  for m in $workspaces; do cargo update --workspace --manifest-path "$m"; done
fi
[ "$mode" = check ] && exit 0

cargo build --workspace --locked
cargo rustc --locked -p allowit-sdk --release --lib --target wasm32-unknown-unknown --crate-type cdylib
