#!/usr/bin/env bash
# Build the Python wheel and sdist of crates/markitdown-py, install each into
# a fresh virtualenv (the sdist built from source, as pip does where no
# wheel fits) and check that the start-up guard against Microsoft's
# markitdown is installed next to the package.
#
# usage: scripts/check_python_dist.sh   (needs uv and Rust)
set -euo pipefail
cd "$(dirname "$0")/../crates/markitdown-py"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

uv venv -q "$work/build"
VIRTUAL_ENV="$work/build" uv pip install -q maturin
VIRTUAL_ENV="$work/build" "$work/build/bin/maturin" build --release --out "$work/dist"
VIRTUAL_ENV="$work/build" "$work/build/bin/maturin" sdist --out "$work/dist"

check() {
  local name=$1 artifact=$2
  shift 2
  uv venv -q "$work/$name"
  VIRTUAL_ENV="$work/$name" uv pip install -q --no-cache "$@" "$artifact"
  local site
  site=$("$work/$name/bin/python" -c "import sysconfig; print(sysconfig.get_paths()['purelib'])")
  for f in markitdown_rs.pth markitdown_rs_guard.py markitdown/_markitdown_rs.abi3.so; do
    if [ ! -f "$site/$f" ]; then
      echo "$name: $f missing from site-packages" >&2
      exit 1
    fi
  done
  "$work/$name/bin/python" -c "import markitdown, markitdown_rs_guard; assert 'markitdown-rs' in markitdown.__doc__"
  echo "$name: markitdown_rs.pth, markitdown_rs_guard.py and the extension installed"
}

check wheel "$(ls "$work"/dist/*.whl)"
check sdist "$(ls "$work"/dist/*.tar.gz)" --no-binary markitdown-rs
