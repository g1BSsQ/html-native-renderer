#!/usr/bin/env bash
# Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
set -euo pipefail
if ! command -v cargo >/dev/null 2>&1; then
  source "$HOME/.cargo/env"
fi
export RAYON_NUM_THREADS=4
mkdir -p out
action="${1:-build}"
case "$action" in
  build|test) ;;
  *) echo 'usage: bash tools/native-check.sh [build|test]' >&2; exit 2 ;;
esac
started_at=$(date +%s)
cargo "$action" --locked -j 4 2>&1 |
  sed -u -E 's|/home/[^ /]+|/toolchain|g' |
  tee "out/local-${action}.log"
finished_at=$(date +%s)
echo "$((finished_at - started_at))" > "out/local-${action}-seconds.txt"
if [[ "$action" == build ]]; then
  stat -c '%s' "${CARGO_TARGET_DIR:-target}/debug/isr" > out/binary-bytes.txt
fi
