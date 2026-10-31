#!/usr/bin/env bash
# Publish the SharpeBench crates to crates.io in dependency order.
#
# Publishing is IRREVERSIBLE — a version can be yanked but never deleted, and a
# crate name is claimed forever. Decide names first (see docs/PUBLISHING.md §0).
#
#   cargo login <token>            # once; token from crates.io account settings
#   scripts/publish.sh --check     # package-verify the leaf crates locally (no upload)
#   scripts/publish.sh --execute   # real, ordered publish to crates.io
set -euo pipefail

# Dependency-topological order: a crate must be live before its dependents resolve.
# publish-order.txt is the one inventory, shared with release.yml; scripts/test_release.py
# holds it to the workspace graph.
mapfile -t ORDER < "$(dirname "${BASH_SOURCE[0]}")/publish-order.txt"
if [ "${#ORDER[@]}" -eq 0 ]; then
  echo "publish-order.txt is empty; refusing to report a publish of nothing" >&2
  exit 1
fi

# The leaf crates (no internal deps) can be package-verified standalone.
LEAVES=(sharpebench-stats sharpebench-protocol sharpebench-attest)

case "${1:-}" in
  --check)
    for c in "${LEAVES[@]}"; do
      echo ">>> package-check $c"
      cargo package -p "$c" --locked
    done
    echo "leaf crates package cleanly; dependents verify on publish (see docs/PUBLISHING.md §3)."
    ;;
  --execute)
    for c in "${ORDER[@]}"; do
      echo ">>> publish $c"
      cargo publish -p "$c" --locked
    done
    echo "published all ${#ORDER[@]} crates."
    ;;
  *)
    echo "usage: scripts/publish.sh --check | --execute"
    echo "publish order: ${ORDER[*]}"
    exit 2
    ;;
esac
