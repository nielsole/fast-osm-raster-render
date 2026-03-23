"""
One-time data preparation for OSM pixel-perfect rendering evaluation.
Downloads Hamburg OSM PBF from Geofabrik, prepares it with osmium,
and fetches reference tiles from the OSM tile server.

Usage:
    python eval/prepare.py                    # full prep
    python eval/prepare.py --skip-reference   # skip reference tile download
    python eval/prepare.py --skip-osmium      # skip osmium step (if already prepared)

Data is stored in eval/.cache/.
"""

import os
import sys
import time
import json
import math
import argparse
import subprocess
import random
from pathlib import Path

import requests
from PIL import Image

# ---------------------------------------------------------------------------
# Constants (fixed, do not modify)
# ---------------------------------------------------------------------------

HAMBURG_BBOX = {
    "west": 9.7279,
    "south": 53.3949,
    "east": 10.3252,
    "north": 53.7370,
}

GEOFABRIK_URL = "https://download.geofabrik.de/europe/germany/hamburg-latest.osm.pbf"

EVAL_DIR = os.path.dirname(os.path.abspath(__file__))
CACHE_DIR = os.path.join(EVAL_DIR, ".cache")
DATA_DIR = os.path.join(CACHE_DIR, "data")
REFERENCE_DIR = os.path.join(CACHE_DIR, "reference_tiles")
TILE_LIST_PATH = os.path.join(CACHE_DIR, "tile_list.json")
PBF_PATH = os.path.join(DATA_DIR, "hamburg-latest.osm.pbf")
PREPARED_PBF_PATH = os.path.join(DATA_DIR, "hamburg-prepared.osm.pbf")

ZOOM_LEVELS = list(range(10, 17))  # zoom 10 through 16
TILES_PER_ZOOM = 100
TILE_SIZE = 256  # standard OSM tile size

OSM_TILE_URL = "https://tile.openstreetmap.org/{z}/{x}/{y}.png"
USER_AGENT = "OSMPixelPerfectEval/1.0 (https://github.com/nielsole/fast-osm-raster-render; research project; one-time download)"
REQUEST_DELAY = 0.5  # seconds between tile requests per OSM tile usage policy

# ---------------------------------------------------------------------------
# Tile coordinate math
# ---------------------------------------------------------------------------

def deg2num(lat_deg, lon_deg, zoom):
    """Convert lat/lon to tile x/y numbers."""
    lat_rad = math.radians(lat_deg)
    n = 2 ** zoom
    x = int((lon_deg + 180.0) / 360.0 * n)
    y = int((1.0 - math.asinh(math.tan(lat_rad)) / math.pi) / 2.0 * n)
    return x, y


def num2deg(x, y, zoom):
    """Convert tile x/y numbers to lat/lon of the NW corner."""
    n = 2 ** zoom
    lon_deg = x / n * 360.0 - 180.0
    lat_rad = math.atan(math.sinh(math.pi * (1 - 2 * y / n)))
    lat_deg = math.degrees(lat_rad)
    return lat_deg, lon_deg


def get_tiles_fully_inside_bbox(bbox, zoom):
    """Get all tile coordinates fully contained within the bounding box."""
    # Get the tile range covering the bbox
    # Note: in tile coords, y increases southward
    x_min, y_min_area = deg2num(bbox["north"], bbox["west"], zoom)
    x_max, y_max_area = deg2num(bbox["south"], bbox["east"], zoom)

    tiles = []
    for x in range(x_min, x_max + 1):
        for y in range(y_min_area, y_max_area + 1):
            # Get the four corners of this tile
            nw_lat, nw_lon = num2deg(x, y, zoom)
            se_lat, se_lon = num2deg(x + 1, y + 1, zoom)

            # Check that the entire tile is inside the bbox
            if (nw_lon >= bbox["west"] and se_lon <= bbox["east"] and
                    se_lat >= bbox["south"] and nw_lat <= bbox["north"]):
                tiles.append([zoom, x, y])

    return tiles

# ---------------------------------------------------------------------------
# Data download
# ---------------------------------------------------------------------------

def download_pbf():
    """Download Hamburg OSM PBF from Geofabrik."""
    os.makedirs(DATA_DIR, exist_ok=True)

    if os.path.exists(PBF_PATH):
        size_mb = os.path.getsize(PBF_PATH) / 1e6
        print(f"PBF already downloaded: {PBF_PATH} ({size_mb:.1f} MB)")
        return

    print(f"Downloading Hamburg PBF from Geofabrik...")
    print(f"  URL: {GEOFABRIK_URL}")

    max_attempts = 3
    for attempt in range(1, max_attempts + 1):
        try:
            response = requests.get(
                GEOFABRIK_URL, stream=True, timeout=120,
                headers={"User-Agent": USER_AGENT}
            )
            response.raise_for_status()

            total = int(response.headers.get("content-length", 0))
            downloaded = 0
            temp_path = PBF_PATH + ".tmp"

            with open(temp_path, "wb") as f:
                for chunk in response.iter_content(chunk_size=1024 * 1024):
                    if chunk:
                        f.write(chunk)
                        downloaded += len(chunk)
                        if total:
                            pct = 100 * downloaded / total
                            print(f"\r  {downloaded / 1e6:.1f} / {total / 1e6:.1f} MB ({pct:.0f}%)",
                                  end="", flush=True)

            os.rename(temp_path, PBF_PATH)
            size_mb = os.path.getsize(PBF_PATH) / 1e6
            print(f"\n  Saved to {PBF_PATH} ({size_mb:.1f} MB)")
            return

        except (requests.RequestException, IOError) as e:
            print(f"\n  Attempt {attempt}/{max_attempts} failed: {e}")
            for path in [PBF_PATH + ".tmp", PBF_PATH]:
                if os.path.exists(path):
                    try:
                        os.remove(path)
                    except OSError:
                        pass
            if attempt < max_attempts:
                time.sleep(2 ** attempt)

    print("Failed to download PBF after all attempts")
    sys.exit(1)


def prepare_pbf():
    """Run osmium add-locations-to-ways to embed node coordinates."""
    if os.path.exists(PREPARED_PBF_PATH):
        size_mb = os.path.getsize(PREPARED_PBF_PATH) / 1e6
        print(f"Prepared PBF already exists: {PREPARED_PBF_PATH} ({size_mb:.1f} MB)")
        return

    print("Running osmium add-locations-to-ways...")
    t0 = time.time()
    result = subprocess.run(
        ["osmium", "add-locations-to-ways", PBF_PATH, "-o", PREPARED_PBF_PATH],
        capture_output=True, text=True
    )
    t1 = time.time()

    if result.returncode != 0:
        print(f"  osmium failed (exit code {result.returncode}):")
        print(f"  stderr: {result.stderr}")
        print(f"  stdout: {result.stdout}")
        sys.exit(1)

    size_mb = os.path.getsize(PREPARED_PBF_PATH) / 1e6
    print(f"  Done in {t1 - t0:.1f}s: {PREPARED_PBF_PATH} ({size_mb:.1f} MB)")

# ---------------------------------------------------------------------------
# Tile list computation
# ---------------------------------------------------------------------------

def compute_tile_list(force=False):
    """Compute which tiles to evaluate at each zoom level."""
    if not force and os.path.exists(TILE_LIST_PATH):
        with open(TILE_LIST_PATH) as f:
            tile_list = json.load(f)
        total = sum(len(v) for v in tile_list.values())
        print(f"Tile list already computed: {total} tiles across {len(tile_list)} zoom levels")
        return tile_list

    print("Computing tile list (tiles fully inside Hamburg bbox)...")
    tile_list = {}
    random.seed(42)  # reproducible selection

    for zoom in ZOOM_LEVELS:
        tiles = get_tiles_fully_inside_bbox(HAMBURG_BBOX, zoom)
        total_available = len(tiles)

        if len(tiles) > TILES_PER_ZOOM:
            tiles = sorted(random.sample(tiles, TILES_PER_ZOOM))

        tile_list[str(zoom)] = tiles
        print(f"  Zoom {zoom}: {len(tiles)} tiles selected (of {total_available} available)")

    os.makedirs(CACHE_DIR, exist_ok=True)
    with open(TILE_LIST_PATH, "w") as f:
        json.dump(tile_list, f, indent=2)

    total = sum(len(v) for v in tile_list.values())
    print(f"  Total: {total} tiles saved to {TILE_LIST_PATH}")
    return tile_list

# ---------------------------------------------------------------------------
# Reference tile fetching
# ---------------------------------------------------------------------------

def fetch_reference_tiles(tile_list):
    """Fetch reference tiles from the OSM tile server."""
    os.makedirs(REFERENCE_DIR, exist_ok=True)

    # Count existing vs needed
    needed_tiles = []
    existing = 0
    for zoom_str, tiles in tile_list.items():
        for z, x, y in tiles:
            path = os.path.join(REFERENCE_DIR, f"{z}_{x}_{y}.png")
            if os.path.exists(path):
                existing += 1
            else:
                needed_tiles.append((z, x, y))

    if not needed_tiles:
        print(f"All {existing} reference tiles already downloaded")
        return

    print(f"Fetching {len(needed_tiles)} reference tiles ({existing} already exist)...")
    print(f"  Rate limit: {REQUEST_DELAY}s between requests")
    print(f"  Estimated time: {len(needed_tiles) * REQUEST_DELAY / 60:.1f} minutes")

    session = requests.Session()
    session.headers["User-Agent"] = USER_AGENT

    fetched = 0
    failed = 0
    for z, x, y in needed_tiles:
        path = os.path.join(REFERENCE_DIR, f"{z}_{x}_{y}.png")
        url = OSM_TILE_URL.format(z=z, x=x, y=y)

        max_attempts = 3
        success = False
        for attempt in range(1, max_attempts + 1):
            try:
                resp = session.get(url, timeout=30)
                resp.raise_for_status()

                # Validate it's actually a PNG
                if not resp.content[:4] == b'\x89PNG':
                    raise ValueError("Response is not a valid PNG")

                temp_path = path + ".tmp"
                with open(temp_path, "wb") as f:
                    f.write(resp.content)
                os.rename(temp_path, path)

                fetched += 1
                success = True
                total_done = existing + fetched + failed
                total_all = existing + len(needed_tiles)
                print(f"\r  {total_done}/{total_all} tiles ({fetched} fetched, {failed} failed)",
                      end="", flush=True)
                break

            except Exception as e:
                if attempt < max_attempts:
                    time.sleep(2 ** attempt)
                else:
                    print(f"\n  Failed: {z}/{x}/{y}: {e}")
                    failed += 1

        time.sleep(REQUEST_DELAY)

    print(f"\n  Done: {fetched} fetched, {failed} failed")

# ---------------------------------------------------------------------------
# Validation
# ---------------------------------------------------------------------------

def validate_setup():
    """Validate that all required data is present."""
    errors = []

    if not os.path.exists(PREPARED_PBF_PATH):
        errors.append(f"Missing prepared PBF: {PREPARED_PBF_PATH}")

    if not os.path.exists(TILE_LIST_PATH):
        errors.append(f"Missing tile list: {TILE_LIST_PATH}")
    else:
        with open(TILE_LIST_PATH) as f:
            tile_list = json.load(f)

        total_tiles = sum(len(v) for v in tile_list.values())
        missing_refs = 0
        for zoom_str, tiles in tile_list.items():
            for z, x, y in tiles:
                path = os.path.join(REFERENCE_DIR, f"{z}_{x}_{y}.png")
                if not os.path.exists(path):
                    missing_refs += 1

        if missing_refs > 0:
            errors.append(f"Missing {missing_refs}/{total_tiles} reference tiles")

    if errors:
        print("Validation FAILED:")
        for e in errors:
            print(f"  - {e}")
        return False

    print("Validation passed: all data present")
    return True

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

if __name__ == "__main__":
    parser = argparse.ArgumentParser(
        description="Prepare data for OSM pixel-perfect rendering evaluation"
    )
    parser.add_argument("--skip-reference", action="store_true",
                        help="Skip reference tile download")
    parser.add_argument("--skip-osmium", action="store_true",
                        help="Skip osmium preparation step")
    parser.add_argument("--recompute-tiles", action="store_true",
                        help="Force recomputation of tile list")
    args = parser.parse_args()

    print(f"Cache directory: {CACHE_DIR}")
    print(f"Hamburg bbox: {HAMBURG_BBOX}")
    print(f"Zoom levels: {ZOOM_LEVELS}")
    print(f"Tiles per zoom: {TILES_PER_ZOOM}")
    print()

    # Step 1: Download PBF
    download_pbf()
    print()

    # Step 2: Prepare with osmium
    if not args.skip_osmium:
        prepare_pbf()
    print()

    # Step 3: Compute tile list
    tile_list = compute_tile_list(force=args.recompute_tiles)
    print()

    # Step 4: Fetch reference tiles
    if not args.skip_reference:
        fetch_reference_tiles(tile_list)
    print()

    # Step 5: Validate
    validate_setup()
    print()
    print("Done! Ready to evaluate with: python eval/evaluate.py")
