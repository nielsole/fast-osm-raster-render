#!/bin/bash
# Safe wrapper to stop rust-osm-renderer server only
pkill -f "rust-osm-renderer.*prepared.osm.pbf"
