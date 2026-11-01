#!/usr/bin/env python3
"""Measure a running instance using interval CPU time and resident memory; no UI input."""
import argparse
import json
import subprocess
import time


def cpu_seconds(value):
    days, sep, clock = value.partition('-')
    if not sep:
        clock, days = days, '0'
    return int(days) * 86400 + sum(float(v) * 60 ** i for i, v in enumerate(reversed(clock.split(':'))))


def snapshot(pid):
    fields = subprocess.check_output(['/bin/ps', '-p', str(pid), '-o', 'time=,rss='], text=True).split()
    if len(fields) != 2:
        raise SystemExit('Process is no longer running')
    return cpu_seconds(fields[0]), int(fields[1]) * 1024


if __name__ == '__main__':
    assert cpu_seconds('1:02.50') == 62.5
    assert cpu_seconds('01:02:03') == 3723
    assert cpu_seconds('1-01:02:03') == 90123
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('pid', type=int)
    parser.add_argument('seconds', type=int, nargs='?', default=30)
    args = parser.parse_args()
    if args.pid <= 0 or not 1 <= args.seconds <= 3600:
        parser.error('PID must be positive and seconds must be between 1 and 3600')
    a = snapshot(args.pid)
    started = time.monotonic()
    time.sleep(args.seconds)
    b = snapshot(args.pid)
    elapsed = time.monotonic() - started
    print(json.dumps({
        'elapsed_seconds': elapsed,
        'cpu_pct_one_core': 100 * (b[0] - a[0]) / elapsed,
        'rss_start_bytes': a[1], 'rss_end_bytes': b[1],
        'note': 'Child process CPU and GPU allocations are excluded; control focus and workload yourself.'
    }, indent=2))
