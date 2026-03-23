"""
Evaluation script for OSM pixel-perfect rendering.
Renders tiles and compares them pixel-by-pixel against OSM reference tiles.

Usage:
    python eval/evaluate.py                # full evaluation
    python eval/evaluate.py --skip-build   # skip cargo build
    python eval/evaluate.py --render-only  # render tiles without comparison
    python eval/evaluate.py --compare-only # compare existing renders (no server)

Outputs results to eval/.cache/results.json and prints a summary.
"""

import os
import sys
import time
import json
import subprocess
import signal
import urllib.request
import argparse
from pathlib import Path
from io import BytesIO

import numpy as np
from PIL import Image

# ---------------------------------------------------------------------------
# Paths and constants
# ---------------------------------------------------------------------------

EVAL_DIR = os.path.dirname(os.path.abspath(__file__))
PROJECT_DIR = os.path.dirname(EVAL_DIR)
CACHE_DIR = os.path.join(EVAL_DIR, ".cache")
DATA_DIR = os.path.join(CACHE_DIR, "data")
REFERENCE_DIR = os.path.join(CACHE_DIR, "reference_tiles")
RENDERED_DIR = os.path.join(CACHE_DIR, "rendered_tiles")
DIFF_DIR = os.path.join(CACHE_DIR, "diff_tiles")
TILE_LIST_PATH = os.path.join(CACHE_DIR, "tile_list.json")
PREPARED_PBF_PATH = os.path.join(DATA_DIR, "hamburg-prepared.osm.pbf")
RESULTS_PATH = os.path.join(CACHE_DIR, "results.json")

SERVER_PORT = 8080
SERVER_STARTUP_TIMEOUT = 120  # seconds
TILE_SIZE = 256

# Scoring targets
RENDER_TARGET_MS = 50.0    # all tiles should render under 50ms
STARTUP_TARGET_S = 30.0    # server should start within 30s

# ---------------------------------------------------------------------------
# Build
# ---------------------------------------------------------------------------

def build_project():
    """Build the Rust project in release mode. Returns build time in seconds."""
    print("=" * 60)
    print("STEP 1: Building project")
    print("=" * 60)
    t0 = time.time()
    result = subprocess.run(
        ["cargo", "build", "--release"],
        cwd=PROJECT_DIR,
        capture_output=True, text=True
    )
    t1 = time.time()

    if result.returncode != 0:
        print(f"Build FAILED (exit code {result.returncode}):")
        print(result.stderr[-2000:] if len(result.stderr) > 2000 else result.stderr)
        sys.exit(1)

    build_time = t1 - t0
    print(f"  Build completed in {build_time:.1f}s")
    return build_time

# ---------------------------------------------------------------------------
# Server management
# ---------------------------------------------------------------------------

def start_server():
    """Start the tile server, return (process, startup_time_seconds)."""
    print()
    print("=" * 60)
    print("STEP 2: Starting tile server")
    print("=" * 60)
    print(f"  PBF: {PREPARED_PBF_PATH}")
    print(f"  Port: {SERVER_PORT}")

    # Kill any existing server on the port
    subprocess.run(
        [os.path.join(PROJECT_DIR, "stop-server.sh")],
        capture_output=True, cwd=PROJECT_DIR
    )
    time.sleep(1)

    t0 = time.time()
    proc = subprocess.Popen(
        [os.path.join(PROJECT_DIR, "target", "release", "fast-osm-raster-render"),
         PREPARED_PBF_PATH],
        cwd=PROJECT_DIR,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )

    # Wait for server to respond
    ready = False
    last_error = None
    while time.time() - t0 < SERVER_STARTUP_TIMEOUT:
        try:
            req = urllib.request.Request(
                f"http://localhost:{SERVER_PORT}/",
                headers={"User-Agent": "eval"}
            )
            urllib.request.urlopen(req, timeout=5)
            ready = True
            break
        except Exception as e:
            last_error = e
            # Check if process died
            if proc.poll() is not None:
                output = proc.stdout.read().decode(errors="replace")
                print(f"  Server process exited with code {proc.returncode}")
                print(f"  Output: {output[-1000:]}")
                sys.exit(1)
            time.sleep(0.5)

    t1 = time.time()
    startup_time = t1 - t0

    if not ready:
        proc.kill()
        print(f"  Server failed to start within {SERVER_STARTUP_TIMEOUT}s")
        print(f"  Last error: {last_error}")
        sys.exit(1)

    print(f"  Server ready in {startup_time:.1f}s")
    return proc, startup_time


def stop_server(proc):
    """Gracefully stop the tile server."""
    if proc is None:
        return
    proc.terminate()
    try:
        proc.wait(timeout=10)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait()
    print("  Server stopped")

# ---------------------------------------------------------------------------
# Tile rendering
# ---------------------------------------------------------------------------

def render_tiles(tile_list):
    """Render all tiles via HTTP. Returns dict of {zoom: [(z,x,y,time_ms), ...]}."""
    print()
    print("=" * 60)
    print("STEP 3: Rendering tiles")
    print("=" * 60)

    os.makedirs(RENDERED_DIR, exist_ok=True)
    render_results = {}
    total_tiles = sum(len(v) for v in tile_list.values())
    rendered_count = 0

    for zoom_str in sorted(tile_list.keys(), key=int):
        tiles = tile_list[zoom_str]
        zoom = int(zoom_str)
        zoom_results = []

        for z, x, y in tiles:
            url = f"http://localhost:{SERVER_PORT}/tile/{z}/{x}/{y}.png"
            try:
                t0 = time.time()
                req = urllib.request.Request(url, headers={"User-Agent": "eval"})
                resp = urllib.request.urlopen(req, timeout=30)
                data = resp.read()
                t1 = time.time()
                render_time_ms = (t1 - t0) * 1000

                # Save rendered tile
                out_path = os.path.join(RENDERED_DIR, f"{z}_{x}_{y}.png")
                with open(out_path, "wb") as f:
                    f.write(data)

                zoom_results.append((z, x, y, render_time_ms, True))

            except Exception as e:
                print(f"\n  Error rendering {z}/{x}/{y}: {e}")
                zoom_results.append((z, x, y, float("inf"), False))

            rendered_count += 1

        render_results[zoom_str] = zoom_results
        # Print progress per zoom level
        times = [r[3] for r in zoom_results if r[4]]
        if times:
            avg = sum(times) / len(times)
            mx = max(times)
            print(f"  Zoom {zoom}: {len(times)}/{len(tiles)} ok, "
                  f"avg={avg:.1f}ms, max={mx:.1f}ms")
        else:
            print(f"  Zoom {zoom}: all failed")

    return render_results

# ---------------------------------------------------------------------------
# Pixel comparison
# ---------------------------------------------------------------------------

def compare_all_tiles(tile_list):
    """Compare rendered tiles against reference tiles.
    Returns dict of {zoom: [(z,x,y,match_ratio), ...]}."""
    print()
    print("=" * 60)
    print("STEP 4: Pixel-by-pixel comparison")
    print("=" * 60)

    os.makedirs(DIFF_DIR, exist_ok=True)
    comparison_results = {}

    for zoom_str in sorted(tile_list.keys(), key=int):
        tiles = tile_list[zoom_str]
        zoom = int(zoom_str)
        zoom_comparisons = []

        for z, x, y in tiles:
            ref_path = os.path.join(REFERENCE_DIR, f"{z}_{x}_{y}.png")
            rendered_path = os.path.join(RENDERED_DIR, f"{z}_{x}_{y}.png")

            if not os.path.exists(ref_path):
                continue
            if not os.path.exists(rendered_path):
                zoom_comparisons.append((z, x, y, 0.0))
                continue

            match_ratio = compare_single_tile(rendered_path, ref_path, z, x, y)
            zoom_comparisons.append((z, x, y, match_ratio))

        comparison_results[zoom_str] = zoom_comparisons

        if zoom_comparisons:
            ratios = [c[3] for c in zoom_comparisons]
            avg = sum(ratios) / len(ratios)
            mn = min(ratios)
            mx = max(ratios)
            print(f"  Zoom {zoom}: avg={avg:.4f}, min={mn:.4f}, max={mx:.4f} "
                  f"({len(zoom_comparisons)} tiles)")

    return comparison_results


def compare_single_tile(rendered_path, reference_path, z, x, y):
    """Compare a single rendered tile against its reference.
    Returns the fraction of exactly matching pixels (0.0 to 1.0)."""
    rendered_img = Image.open(rendered_path).convert("RGBA")
    reference_img = Image.open(reference_path).convert("RGBA")

    # Resize rendered to match reference if needed (our renderer may produce
    # different size tiles than the 256x256 OSM standard)
    if rendered_img.size != reference_img.size:
        rendered_img = rendered_img.resize(reference_img.size, Image.NEAREST)

    rendered = np.array(rendered_img)
    reference = np.array(reference_img)

    # Exact pixel match across all RGBA channels
    pixel_match = np.all(rendered == reference, axis=-1)
    match_ratio = float(pixel_match.sum()) / float(pixel_match.size)

    # Generate diff image for visual debugging
    # Green = match, Red = reference only, Blue = rendered only
    diff = np.zeros((*reference.shape[:2], 3), dtype=np.uint8)
    diff[pixel_match] = [0, 128, 0]       # matching pixels in green
    diff[~pixel_match] = [255, 0, 0]      # mismatched pixels in red

    diff_path = os.path.join(DIFF_DIR, f"{z}_{x}_{y}_diff.png")
    Image.fromarray(diff).save(diff_path)

    return match_ratio

# ---------------------------------------------------------------------------
# Scoring
# ---------------------------------------------------------------------------

def compute_scores(build_time, startup_time, render_results, comparison_results):
    """Compute normalized scores. Returns full results dict."""

    results = {
        "timestamp": time.strftime("%Y-%m-%d %H:%M:%S"),
        "build_time_s": build_time,
        "startup_time_s": startup_time,
        "targets": {
            "render_ms": RENDER_TARGET_MS,
            "startup_s": STARTUP_TARGET_S,
        },
        "zoom_levels": {},
    }

    zoom_visual_scores = {}
    zoom_render_scores = {}

    all_zooms = set(render_results.keys()) | set(comparison_results.keys())

    for zoom_str in sorted(all_zooms, key=int):
        zdata = {}

        # Render times
        if zoom_str in render_results:
            times = [r[3] for r in render_results[zoom_str] if r[4]]  # successful only
            if times:
                zdata["num_rendered"] = len(times)
                zdata["avg_render_time_ms"] = sum(times) / len(times)
                zdata["max_render_time_ms"] = max(times)
                zdata["min_render_time_ms"] = min(times)
                zdata["all_render_times_ms"] = times

                # Render score: target / max(target, actual_max)
                zoom_render_scores[zoom_str] = min(
                    1.0, RENDER_TARGET_MS / max(zdata["max_render_time_ms"], 0.001)
                )
            else:
                zdata["num_rendered"] = 0
                zoom_render_scores[zoom_str] = 0.0

        # Pixel comparisons
        if zoom_str in comparison_results:
            ratios = [c[3] for c in comparison_results[zoom_str]]
            if ratios:
                zdata["num_compared"] = len(ratios)
                zdata["avg_pixel_match"] = sum(ratios) / len(ratios)
                zdata["min_pixel_match"] = min(ratios)
                zdata["max_pixel_match"] = max(ratios)
                zdata["all_match_ratios"] = ratios

                # Visual score: average match ratio at this zoom
                zoom_visual_scores[zoom_str] = zdata["avg_pixel_match"]
            else:
                zdata["num_compared"] = 0
                zoom_visual_scores[zoom_str] = 0.0

        results["zoom_levels"][zoom_str] = zdata

    # Aggregate scores: minimum across zoom levels (worst zoom dominates)
    visual_scores = list(zoom_visual_scores.values())
    render_scores = list(zoom_render_scores.values())

    visual_score = min(visual_scores) if visual_scores else 0.0
    render_score = min(render_scores) if render_scores else 0.0

    # Startup score: target / max(target, actual)
    startup_score = min(1.0, STARTUP_TARGET_S / max(startup_time, 0.001))

    # Combined: minimum of all three (worst dimension always dominates)
    combined_score = min(visual_score, render_score, startup_score)

    results["scores"] = {
        "visual_score": visual_score,
        "render_score": render_score,
        "startup_score": startup_score,
        "combined_score": combined_score,
        "per_zoom_visual": zoom_visual_scores,
        "per_zoom_render": zoom_render_scores,
    }

    return results

# ---------------------------------------------------------------------------
# Summary printing
# ---------------------------------------------------------------------------

def print_summary(results):
    """Print a formatted evaluation summary."""
    scores = results["scores"]

    print()
    print("=" * 60)
    print("EVALUATION RESULTS")
    print("=" * 60)
    print()
    print(f"Build time:   {results['build_time_s']:.1f}s")
    print(f"Startup time: {results['startup_time_s']:.1f}s")
    print()

    print("Per-zoom results:")
    header = f"  {'Zoom':>4} {'Tiles':>5} {'Match':>8} {'Avg ms':>8} {'Max ms':>8} {'V-Score':>8} {'R-Score':>8}"
    print(header)
    print(f"  {'─' * 4:>4} {'─' * 5:>5} {'─' * 8:>8} {'─' * 8:>8} {'─' * 8:>8} {'─' * 8:>8} {'─' * 8:>8}")

    for zoom_str in sorted(results["zoom_levels"].keys(), key=int):
        zdata = results["zoom_levels"][zoom_str]
        n_tiles = zdata.get("num_compared", zdata.get("num_rendered", 0))
        avg_match = zdata.get("avg_pixel_match", 0.0)
        avg_time = zdata.get("avg_render_time_ms", 0.0)
        max_time = zdata.get("max_render_time_ms", 0.0)
        v_score = scores["per_zoom_visual"].get(zoom_str, 0.0)
        r_score = scores["per_zoom_render"].get(zoom_str, 0.0)

        print(f"  {zoom_str:>4} {n_tiles:>5} {avg_match:>8.4f} "
              f"{avg_time:>8.1f} {max_time:>8.1f} "
              f"{v_score:>8.4f} {r_score:>8.4f}")

    print()
    print("Aggregate Scores:")
    print(f"  Visual (pixel match):  {scores['visual_score']:.4f}")
    print(f"  Render (time target):  {scores['render_score']:.4f}")
    print(f"  Startup:               {scores['startup_score']:.4f}")
    print()
    print(f"  COMBINED SCORE:        {scores['combined_score']:.4f}")
    print()
    print(f"Results saved to: {RESULTS_PATH}")
    print()
    print("Tile images:")
    print(f"  Reference: {REFERENCE_DIR}/")
    print(f"  Rendered:  {RENDERED_DIR}/")
    print(f"  Diffs:     {DIFF_DIR}/")

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main():
    parser = argparse.ArgumentParser(
        description="Evaluate OSM pixel-perfect rendering"
    )
    parser.add_argument("--skip-build", action="store_true",
                        help="Skip cargo build step")
    parser.add_argument("--compare-only", action="store_true",
                        help="Only compare existing tiles (no server)")
    args = parser.parse_args()

    # Validate prerequisites
    if not os.path.exists(TILE_LIST_PATH):
        print(f"ERROR: Missing tile list. Run 'python eval/prepare.py' first.")
        sys.exit(1)

    with open(TILE_LIST_PATH) as f:
        tile_list = json.load(f)

    total_tiles = sum(len(v) for v in tile_list.values())
    print(f"Evaluation: {total_tiles} tiles across {len(tile_list)} zoom levels")
    print()

    if args.compare_only:
        # Compare-only mode: no build, no server
        build_time = 0.0
        startup_time = 0.0
        render_results = {}

        # Create dummy render results from existing rendered tiles
        for zoom_str, tiles in tile_list.items():
            zoom_results = []
            for z, x, y in tiles:
                rendered_path = os.path.join(RENDERED_DIR, f"{z}_{x}_{y}.png")
                if os.path.exists(rendered_path):
                    zoom_results.append((z, x, y, 0.0, True))
            render_results[zoom_str] = zoom_results

        comparison_results = compare_all_tiles(tile_list)
        results = compute_scores(build_time, startup_time, render_results, comparison_results)

        with open(RESULTS_PATH, "w") as f:
            json.dump(results, f, indent=2, default=str)

        print_summary(results)
        return

    # Full evaluation
    if not os.path.exists(PREPARED_PBF_PATH):
        print(f"ERROR: Missing prepared PBF. Run 'python eval/prepare.py' first.")
        sys.exit(1)

    # Step 1: Build
    if args.skip_build:
        build_time = 0.0
        print("Skipping build (--skip-build)")
    else:
        build_time = build_project()

    # Step 2: Start server
    proc = None
    try:
        proc, startup_time = start_server()

        # Step 3: Render tiles
        render_results = render_tiles(tile_list)

    finally:
        # Always stop server
        if proc:
            print()
            print("Stopping server...")
            stop_server(proc)

    # Step 4: Compare tiles
    comparison_results = compare_all_tiles(tile_list)

    # Step 5: Compute scores
    results = compute_scores(build_time, startup_time, render_results, comparison_results)

    # Save results
    with open(RESULTS_PATH, "w") as f:
        json.dump(results, f, indent=2, default=str)

    # Print summary
    print_summary(results)

    # Print the key metric line for easy grep
    print()
    print("---")
    print(f"combined_score:  {results['scores']['combined_score']:.6f}")
    print(f"visual_score:    {results['scores']['visual_score']:.6f}")
    print(f"render_score:    {results['scores']['render_score']:.6f}")
    print(f"startup_score:   {results['scores']['startup_score']:.6f}")
    print(f"startup_time_s:  {results['startup_time_s']:.1f}")
    print(f"build_time_s:    {results['build_time_s']:.1f}")


if __name__ == "__main__":
    main()
