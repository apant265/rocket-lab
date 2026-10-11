"""Compare exported GPU thermal history with an independent CPU run.
python scripts/check_thermal.py [gpu_csv] [cpu_csv]
"""
import csv
import json
from pathlib import Path
import sys

gpu = Path(sys.argv[1] if len(sys.argv) > 1 else 'output/thermal/thermal.csv')
cpu = Path(sys.argv[2] if len(sys.argv) > 2 else 'output/thermal-cpu/thermal.csv')
maximum = 0.0
total = 0
worst = None
with gpu.open(newline='') as a, cpu.open(newline='') as b:
    for ga, cb in zip(csv.DictReader(a), csv.DictReader(b), strict=True):
        assert all(ga[k] == cb[k] for k in ['stage', 'region', 'station_m']), 'Different thermal grids'
        assert abs(float(ga['time_s']) - float(cb['time_s'])) < 1e-9, 'Different flight histories'
        for field in ['outer_c', 'inner_c']:
            difference = abs(float(ga[field]) - float(cb[field]))
            total += 1
            if difference > maximum:
                maximum = difference
                worst = {'time_s': float(ga['time_s']), 'stage': ga['stage'], 'region': ga['region'], 'field': field}
report = {'checked_temperatures': total, 'max_absolute_difference_k': maximum, 'worst': worst}
print(json.dumps(report, indent=2))
(gpu.parent / 'full-reference-check.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
assert total > 0 and maximum < 0.25, 'GPU / CPU error exceeds the 0.25 K reference threshold'
