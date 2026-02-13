#!/bin/bash
# Stop any running server
./stop-server.sh

# Start with styled shader via cargo run (incremental build when needed)
cargo run --release -- ../go-gl-osm/germany-prepared.osm.pbf --styled-shader > /tmp/server_debug.log 2>&1 &
echo "Server started with styled shader (PID: $!)"
echo "Logs: /tmp/server_debug.log"
