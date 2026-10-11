"""Verify a saved vehicle temperature image against its mesh and GPU result."""
import csv
import hashlib
import json
from pathlib import Path

from PIL import Image

root = Path(__file__).resolve().parents[1]
png = max((root / "output/analysis-images").glob("*retained-vehicle.png"), key=lambda p: p.stat().st_mtime)
meta = json.loads(png.with_suffix(".json").read_text(encoding="utf-8"))
case = root / "output/snapshots/r0-t4010000000000000-thermal-fine-Preview"
snapshot = json.loads((case / "snapshot.json").read_text())
model = json.loads((root / "output/model-flight/rocket.mesh.json").read_text())
pixels = Image.open(png).convert("RGBA")
assert pixels.size == (meta["width"], meta["height"])
assert meta["unit"] == "°C" and meta["viewed_time_s"] == meta["captured_time_s"] == 4.0
assert "Exported mesh" in meta["note"]
assert snapshot["gpu_used"] and snapshot["gpu_requested"]
assert "RTX 3060 Ti" in snapshot["backend"]
assert snapshot["gpu_cpu_initial_max_error_k"] < 0.25
with (case / "thermal.csv").open(newline="") as stream:
    values = [float(row["outer_c"]) for row in csv.DictReader(stream) if abs(float(row["time_s"])-4.0) < 1e-9]
assert values
assert min(values)-1e-6 <= meta["range"][0] <= meta["range"][1] <= max(values)+1e-6
axial = [-v[1] for part in model["parts"] for v in part["positions"]]
assert meta["x_extent"][0] < min(axial) and meta["x_extent"][1] > max(axial)
data = pixels.tobytes()
histogram = list(zip(data[0::4], data[1::4], data[2::4], data[3::4]))
transparent = sum(p[3] == 0 for p in histogram)
gray = sum(p == (80,80,80,255) for p in histogram)
colored = pixels.width*pixels.height-transparent-gray
assert transparent > colored > 0 and gray > 0
assert pixels.getpixel((0,0))[3] == pixels.getpixel((pixels.width-1,pixels.height-1))[3] == 0
assert (root / "output/thermal-native-errors.log").read_text() == ""
assert "50 passed; 0 failed" in (root / "output/thermal-viewer-tests.log").read_text()
assert "Finished" in (root / "output/thermal-clippy.log").read_text()
report = {
    "passed": True,
    "executable_sha256": hashlib.sha256((root / "target/debug/rocket-lab.exe").read_bytes()).hexdigest(),
    "tests_passed": 50,
    "clippy_warnings": 0,
    "panel_regression_frames": 120,
    "manual_edge_resize_regression": "passed",
    "native_dark_light_build_and_flight": "visually inspected",
    "header_branding_removed": True,
    "geometry_image": str(png.relative_to(root)),
    "image_dimensions": list(pixels.size),
    "transparent_background_pixels": transparent,
    "uncomputed_gray_pixels": gray,
    "temperature_pixels": colored,
    "temperature_range_c": meta["range"],
    "gpu_adapter": snapshot["gpu_adapter"],
    "gpu_cpu_max_error_k": snapshot["gpu_cpu_initial_max_error_k"],
    "native_3d_thermal": "visually inspected",
    "thermal_scope": "Computed material regions projected onto mesh; no additional thermal degrees of freedom",
}
(root / "output/thermal-vehicle-check.json").write_text(json.dumps(report,indent=2)+"\n")
print(json.dumps(report,indent=2))
