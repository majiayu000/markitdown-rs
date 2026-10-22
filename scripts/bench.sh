#!/usr/bin/env bash
# Benchmark markitdown-rs against Python markitdown 0.1.8 on this machine.
# Prints a report to stdout; README numbers come from bench/results-*.txt.
#
# usage: scripts/bench.sh [runs]   (default 10; Python runs on large.docx: 3)
set -euo pipefail
cd "$(dirname "$0")/.."
N=${1:-10}
uv sync --project oracle --frozen --all-groups --quiet
uv run --project oracle --frozen python scripts/gen_bench_corpus.py > /dev/null
cargo build --release -q -p markitdown-rs --bin markitdown
cargo build --release -q -p markitdown-core --example bench
PY=oracle/.venv/bin/markitdown
RS=target/release/markitdown
FILES=(tests/fixtures/docx/markitdown-test.docx bench/corpus/medium.docx bench/corpus/large.docx
       tests/fixtures/pptx/markitdown-test.pptx bench/corpus/deck.pptx
       tests/fixtures/pdf/markitdown-test.pdf bench/corpus/report.pdf)
if [ "$(uname)" = Darwin ]; then
  CPU=$(sysctl -n machdep.cpu.brand_string); TIME=(/usr/bin/time -l); RSS_FIELD="maximum resident set size"
else
  CPU=$(grep -m1 "model name" /proc/cpuinfo | cut -d: -f2); TIME=(/usr/bin/time -v); RSS_FIELD="Maximum resident set size"
fi
export PYTHONUTF8=1

echo "# $(date '+%Y-%m-%d %H:%M %Z') | $CPU | $(uname -sr)"
echo "# $(oracle/.venv/bin/python -V) markitdown 0.1.8 | $(rustc -V)"
echo "# load: $(uptime | sed 's/.*load average[s]*: //')"

echo "## identical output (python CLI vs rust CLI)"
for f in "${FILES[@]}"; do
  if cmp -s <("$PY" "$f") <("$RS" "$f"); then echo "same  $f"; else echo "DIFF  $f"; fi
done

echo "## in-process conversion (file read + convert, warm, median of $N; python: 5)"
target/release/examples/bench "$N" "${FILES[@]}"
oracle/.venv/bin/python scripts/bench_inproc_py.py 5 "${FILES[@]}"

echo "## CLI end to end (new process per run)"
for f in "${FILES[@]}"; do
  oracle/.venv/bin/python scripts/bench_cli.py "$N" "$f" -- "$RS"
  runs=$N; case "$f" in bench/corpus/large.docx|bench/corpus/report.pdf) runs=3 ;; esac
  oracle/.venv/bin/python scripts/bench_cli.py "$runs" "$f" -- "$PY"
done

echo "## peak RSS in bytes"
for f in bench/corpus/large.docx bench/corpus/report.pdf; do
  for cmd in "$RS" "$PY"; do
    rss=$("${TIME[@]}" "$cmd" "$f" 2>&1 >/dev/null | grep "$RSS_FIELD" | grep -oE '[0-9]+' | head -1)
    echo "$f $cmd $rss"
  done
done

echo "## footprint"
echo "binary bytes: $(wc -c < "$RS" | tr -d ' ')"
echo "python venv (markitdown[docx,pptx,xlsx,xls,pdf,outlook] + bench deps): $(du -sh oracle/.venv | cut -f1)"
echo "# load after: $(uptime | sed 's/.*load average[s]*: //')"
