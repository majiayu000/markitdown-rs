#!/usr/bin/env bash
# Regenerate tests/snapshots/ from the pinned Python markitdown in oracle/.
# Each snapshot is the exact stdout of `markitdown <fixture>`. When Python
# fails on a fixture, its error is stored as <name>.py-error instead.
#
# usage: scripts/gen_snapshots.sh [fixture...]
set -euo pipefail
cd "$(dirname "$0")/.."
uv sync --project oracle --frozen --quiet
export PYTHONUTF8=1
if [ $# -eq 0 ]; then set -- tests/fixtures/*/*.docx tests/fixtures/*/*.pptx; fi
for f in "$@"; do
  rel=${f#tests/fixtures/}
  out=tests/snapshots/$rel
  mkdir -p "$(dirname "$out")"
  rm -f "$out.md" "$out.py-error"
  if uv run --project oracle --frozen markitdown "$f" > "$out.md" 2> "$out.stderr"; then
    rm "$out.stderr"
  else
    rm "$out.md"
    grep -E '^[A-Za-z_.]+(Error|Exception)' "$out.stderr" | tail -1 > "$out.py-error"
    rm "$out.stderr"
    echo "python failed: $rel ($(cat "$out.py-error"))"
  fi
done
