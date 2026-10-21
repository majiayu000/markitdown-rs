"""End-to-end CLI benchmark: wall time and child CPU time per process run.

usage: python bench_cli.py <N> <file> -- <cmd...>   (file is appended to cmd)
"""
import resource
import statistics
import subprocess
import sys
import time

n, f = int(sys.argv[1]), sys.argv[2]
cmd = sys.argv[sys.argv.index("--") + 1:] + [f]
subprocess.run(cmd, check=True, stdout=subprocess.DEVNULL)  # warm OS cache
walls, cpus = [], []
for _ in range(n):
    r0 = resource.getrusage(resource.RUSAGE_CHILDREN)
    t = time.perf_counter()
    subprocess.run(cmd, check=True, stdout=subprocess.DEVNULL)
    walls.append((time.perf_counter() - t) * 1e3)
    r1 = resource.getrusage(resource.RUSAGE_CHILDREN)
    cpus.append(((r1.ru_utime - r0.ru_utime) + (r1.ru_stime - r0.ru_stime)) * 1e3)
print(f"{cmd[0].split('/')[-1]}\t{f.split('/')[-1]}\twall_median_ms={statistics.median(walls):.1f}\tcpu_median_ms={statistics.median(cpus):.1f}\twall_min_ms={min(walls):.1f}")
