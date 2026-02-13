#!/bin/bash
echo "Stopping old server..."
./stop-server.sh
sleep 1
echo "Starting server with per-object styling..."
./target/release/rust-osm-renderer ../go-gl-osm/prepared.osm.pbf --styled-shader > /tmp/server_debug.log 2>&1 &
SERVER_PID=$!
echo "Server started (PID: $SERVER_PID)"
sleep 2
echo ""
echo "Testing tile rendering..."
echo ""
echo "Tile 1: http://localhost:8080/tile/15/17291/10584@2x.png"
echo "Tile 2: http://localhost:8080/tile/15/17291/10586@2x.png"
echo ""
echo "Both tiles should now show correct per-road colors!"
echo "Primary roads = RED, all others = BLACK"
echo ""
echo "Check logs: tail -f /tmp/server_debug.log"
