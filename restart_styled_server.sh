#!/bin/bash
# Stop any running server
./stop-server.sh

# Rebuild release binary if sources are newer
echo "Building release binary..."
cargo build --release 2>&1 | tail -3
if [ ${PIPESTATUS[0]} -ne 0 ]; then
    echo "ERROR: Build failed, not starting server"
    exit 1
fi

# Start with styled shader
./target/release/rust-osm-renderer ../go-gl-osm/germany-prepared.osm.pbf --styled-shader > /tmp/server_debug.log 2>&1 &
echo "Server started with styled shader (PID: $!)"
echo "Logs: /tmp/server_debug.log"
