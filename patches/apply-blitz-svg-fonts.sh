#!/usr/bin/env bash
# Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cargo fetch --locked
cargo_home="${CARGO_HOME:-$HOME/.cargo}"
src="$(find "$cargo_home/git/checkouts" -path '*/blitz-*/92b11f1/packages/blitz-dom/src/util.rs' -type f -print -quit)"
if [[ -z "$src" ]]; then echo "pinned Blitz checkout missing" >&2; exit 1; fi
src="${src%/packages/blitz-dom/src/util.rs}"
patch="$root/patches/blitz-svg-scoped-fonts.patch"
if git -C "$src" apply --reverse --check "$patch" 2>/dev/null; then
  echo "SVG font patch already applied"
else
  if ! git -C "$src" apply --check "$patch"; then
    # Cargo cache entries can contain an earlier version of our local patch.
    # Reset only the pinned upstream target file, then check the new patch.
    git -C "$src" restore --source=HEAD -- packages/blitz-dom/src/util.rs
    git -C "$src" apply --check "$patch"
  fi
  git -C "$src" apply "$patch"
  echo "SVG font patch applied"
fi
