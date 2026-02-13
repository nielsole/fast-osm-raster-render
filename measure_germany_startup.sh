#!/bin/bash
set -euo pipefail

OSM_PATH="${1:-../go-gl-osm/germany-prepared.osm.pbf}"

echo "Measuring startup load time for: ${OSM_PATH}"
echo "Command: cargo run --release -- ${OSM_PATH} --load-stats-only"
echo ""

cargo run --release -- "${OSM_PATH}" --load-stats-only | tee /tmp/rust_osm_load_stats.log

echo ""
echo "Latest LOAD_STATS line:"
grep "LOAD_STATS" /tmp/rust_osm_load_stats.log | tail -1
