# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build and Run Commands

```bash
# Build release (optimized for performance)
cargo build --release

# Build debug (includes Vulkan validation layers)
cargo build

# Run tests
cargo test

# Run specific test
cargo test test_name

# Start HTTP server
cargo run --release -- /path/to/prepared.osm.pbf

# Start with alternative shader (for debugging)
cargo run --release -- /path/to/prepared.osm.pbf --simple-shader
cargo run --release -- /path/to/prepared.osm.pbf --debug-shader

# Stop the server (ALWAYS use this script, not pkill directly)
./stop-server.sh

# Render a single tile directly (MUCH faster for testing than curl)
cargo run --example render_tile -- /path/to/prepared.osm.pbf <z> <x> <y> [output.png] [--simple-shader|--debug-shader]
# Example: cargo run --example render_tile -- prepared.osm.pbf 11 1081 660 hamburg.png
```

## Testing Workflow

**IMPORTANT: For rapid iteration, use the `render_tile` example instead of the HTTP server:**

```bash
# Fast testing cycle (no server startup/teardown overhead)
cargo run --example render_tile -- prepared.osm.pbf 11 1081 660 test.png

# Test different shaders
cargo run --example render_tile -- prepared.osm.pbf 11 1081 660 test.png --debug-shader
cargo run --example render_tile -- prepared.osm.pbf 11 1081 660 test.png --simple-shader
```

The render_tile example:
- Loads OSM data once
- Renders directly to file
- No HTTP overhead
- Shows detailed logging
- Reports non-white pixel count for validation

Test tile: `11/1081/660` contains Hamburg and is good for testing (lon: 10.092224, lat: 53.677150).

## Architecture Overview

### Data Flow

```
OSM PBF → node_locations() API → Spatial Index (HashMap<TileKey, Vec<offset>>)
                                              ↓
                                   Memory-Mapped Binary File
                                              ↓
HTTP Request → Tile Lookup → Build Vertex Buffer → Vulkan Render → PNG
```

### Critical Components

**Vulkan Rendering Pipeline:**
- Headless rendering (no VK_KHR_surface, no swapchain)
- LINE_LIST topology for road segments
- Render to framebuffer → copy to staging buffer → read to CPU
- Thread-local renderers (one per HTTP handler thread)

**Shader Architecture:**
- GLSL 450 vertex shaders compiled to SPIR-V at build time (build.rs)
- Three shader variants: Mercator (default), Simple, Debug
- Mercator projection computed on GPU in vertex shader
- Direct NDC mapping (no projection matrix) - learned the hard way that GLSL column-major ordering is tricky

**Memory Management:**
- gpu-allocator for Vulkan memory (device-local and host-visible)
- Descriptor sets MUST be freed after each render to avoid pool exhaustion
- Struct field ordering matters for drop order: memory_manager before context

**OSM Data Loading:**
- Uses osmpbf crate v0.3 with node_locations() API
- Requires pre-processed OSM data: `osmium add-locations-to-ways input.osm.pbf -o prepared.osm.pbf`
- Spatial indexing by tile quadtree (zoom 0-15)
- Binary format: BoundingBox (32 bytes) + points array

### Key File Locations

**Shaders (compiled at build time):**
- `shaders/tile.vert` - Mercator projection shader (default)
- `shaders/tile_simple.vert` - Linear projection (for debugging)
- `shaders/tile_debug.vert` - Fixed pattern output (pipeline testing)
- `shaders/tile.frag` - Fragment shader (black lines)
- `build.rs` - Compiles shaders to SPIR-V

**Renderer:**
- `src/renderer/renderer.rs` - Main VulkanRenderer, render_tile() function
- `src/renderer/pipeline.rs` - Graphics pipeline setup, ShaderType enum
- `src/renderer/vulkan.rs` - Vulkan context initialization
- `src/renderer/memory.rs` - Buffer/image allocation helpers
- `src/renderer/command.rs` - Command buffer helpers

**Data Pipeline:**
- `src/data/loader.rs` - OSM PBF parsing with node_locations() API
- `src/data/spatial.rs` - Tile indexing (critical: tile.index() algorithm)
- `src/data/serialization.rs` - Binary format (Go-compatible)
- `src/data/mmap.rs` - Memory-mapped file access
- `src/data/types.rs` - Core data structures

**Server:**
- `src/main.rs` - Entry point, OSM loading, server startup
- `src/server/mod.rs` - AppState with shader_type field
- `src/server/handlers.rs` - Tile request handler with thread-local renderers

## Common Pitfalls

### Descriptor Pool Exhaustion
Descriptor sets MUST be freed after each render:
```rust
unsafe {
    self.context.device.free_descriptor_sets(self.descriptor_pool, &[descriptor_set])?;
}
```
Without this, you get `ERROR_OUT_OF_POOL_MEMORY` after ~10 renders.

### Struct Drop Order
VulkanRenderer fields MUST be ordered correctly:
```rust
pub struct VulkanRenderer {
    // ... other fields first
    memory_manager: Arc<Mutex<Allocator>>, // Drop before context
    context: VulkanContext,                 // Drop last
}
```
Wrong order causes SIGSEGV during cleanup.

### Shader Coordinate Systems
- GLSL uses column-major matrices (different from row-major Rust arrays)
- Current solution: bypass projection matrix, use direct NDC mapping
- NDC in Vulkan: -1 to 1 (not 0 to 1 like some APIs)

### OSM Data Requirements
OSM data MUST be preprocessed with osmium to embed node coordinates:
```bash
osmium add-locations-to-ways input.osm.pbf -o prepared.osm.pbf
```
The osmpbf crate's Way.refs() only returns node IDs, not coordinates.
Use Way.node_locations() instead.

### Vertex Buffer Building
Lines are pairs of vertices (LINE_LIST topology):
```rust
// For each line segment, push TWO vertices
vertices[index] = points[i-1].lon;     // Start point
vertices[index+1] = points[i-1].lat;
vertices[index+2] = points[i].lon;     // End point
vertices[index+3] = points[i].lat;
```

## Development Workflow

1. Make changes to code/shaders
2. Test with render_tile example for fast iteration
3. If shader changes, rebuild (build.rs recompiles shaders automatically)
4. For server testing: `./stop-server.sh` then `cargo run --release`
5. Check logs in `/tmp/server_debug.log` when server runs in background

## Server Management

Server stores data in `/tmp/rust-osm-renderer-data.bin` (memory-mapped file).
Port 8080 is hardcoded.
Leaflet viewer available at http://localhost:8080/ (if static/ directory exists).

## Debugging Shaders

Three shader variants for debugging:
- `--debug-shader`: Outputs fixed X pattern to verify pipeline works
- `--simple-shader`: Linear projection (no Mercator) for simpler math
- Default: Full Mercator projection

Test point for Hamburg tile 11/1081/660: lon=10.092224, lat=53.677150
