"""In-process warm benchmark of Python markitdown (import excluded).

usage: python bench_inproc_py.py <N> <files...>  -> TSV like `mdrs --bench`
"""
import statistics
import sys
import time

t0 = time.perf_counter()
from markitdown import MarkItDown  # noqa: E402

md = MarkItDown()
print(f"# import+init_ms\t{(time.perf_counter() - t0) * 1e3:.1f}")
n = int(sys.argv[1])
print("file\tmedian_ms\tmin_ms\tout_chars")
for f in sys.argv[2:]:
    out = md.convert(f).markdown  # warm-up
    times = []
    for _ in range(n):
        t = time.perf_counter()
        md.convert(f)
        times.append((time.perf_counter() - t) * 1e3)
    print(f"{f}\t{statistics.median(times):.3f}\t{min(times):.3f}\t{len(out)}")
