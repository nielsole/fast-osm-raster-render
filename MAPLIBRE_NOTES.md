# MapLibre Tile Quality Optimization

## The Problem

MapLibre GL JS uses **fractional zoom levels** (12.3, 13.7, etc.) for smooth zoom animations. When rendering at fractional zooms:
- Zoom 12.5 = between zoom 12 and zoom 13
- MapLibre fetches zoom 12 tiles and scales them up by ~1.4x
- Result: **Blurry/washed out appearance**

## ✅ IMPLEMENTED SOLUTION: 512px Backend Rendering

We've implemented proper 512px (@2x) tile rendering in the backend!

### Backend Changes

The backend now supports two tile sizes:
- **256px tiles**: `/tile/{z}/{x}/{y}.png` (standard)
- **512px tiles**: `/tile/{z}/{x}/{y}@2x.png` (high resolution)

**Implementation details:**
- Added `tile_size` parameter to VulkanRenderer
- Separate thread-local renderers for 256px and 512px
- Dynamic viewport/framebuffer sizing based on request
- Zero bandwidth overhead (only fetch @2x when requested)

### Frontend Usage

**MapLibre (RECOMMENDED):**
```javascript
sources: {
    'osm-tiles': {
        tiles: ['/tile/{z}/{x}/{y}@2x.png'],  // Request 512px tiles
        tileSize: 512                          // Tell MapLibre they're 512px
    }
}
```

Result: Crisp tiles at all zoom levels, including fractional zoom!

## Alternative Approaches (For Reference)

### Solution 1: 512px Tilesize Trick ⭐ RECOMMENDED
**File:** `static/index-maplibre-512.html`

Tell MapLibre tiles are 512px instead of 256px. This forces it to fetch from **one zoom level higher** and scale down instead of up.

```javascript
sources: {
    'osm-tiles': {
        tileSize: 512,  // Instead of 256
        maxzoom: 17     // One less since we overzoom by 1
    }
}
```

**How it works:**
- Display zoom 12.5 → Fetches tiles from zoom 11 (but with 512 tileSize)
- Since tiles are really 256px, they render at half size (crisp!)
- Effectively you're always rendering higher-res tiles scaled down

**Pros:**
- ✅ Simple one-line config change
- ✅ Always crisp tiles (scale down vs scale up)
- ✅ No backend changes needed

**Cons:**
- ⚠️ Uses 4x more bandwidth (fetching higher zoom tiles)
- ⚠️ Display zoom != tile zoom (can be confusing)

---

### Solution 2: Disable Fractional Zoom
**Add to map config:**

```javascript
const map = new maplibregl.Map({
    ...
    // Force integer zoom levels only
    renderWorldCopies: false,
    // OR handle zoom rounding manually
});

// Round zoom to nearest integer
map.on('zoomend', () => {
    const currentZoom = map.getZoom();
    const roundedZoom = Math.round(currentZoom);
    if (Math.abs(currentZoom - roundedZoom) > 0.01) {
        map.setZoom(roundedZoom);
    }
});
```

**Pros:**
- ✅ No blurriness (always exact zoom levels)
- ✅ Bandwidth efficient

**Cons:**
- ❌ Loses smooth zoom animation (jumpy)
- ❌ Less modern UX

---

### Solution 3: Backend Fractional Zoom Support
**Most complex, but best quality**

Modify the backend to support URLs like:
- `/tile/12.5/1234/5678.png` (fractional zoom)
- `/tile/12/1234/5678.png@2x` (retina tiles at 512px)

**Backend changes needed:**
1. Parse fractional zoom from URL
2. Calculate bounding box for fractional zoom
3. Render at higher resolution and scale

**Implementation sketch:**
```rust
// In server/handlers.rs
fn parse_tile_params(path: &str) -> (f64, u32, u32) {
    // Parse z as f64 instead of u32
    let z_float = parts[1].parse::<f64>()?;
    (z_float, x, y)
}

// In projection.rs
fn get_bounding_box_fractional(tile_x: u32, tile_y: u32, zoom: f64) -> BoundingBox {
    let n = 2.0_f64.powf(zoom);  // Use powf for fractional
    // ... calculate bbox
}

// In renderer
fn render_tile_fractional(tile_z: f64, tile_x: u32, tile_y: u32) {
    // Render at higher resolution based on fractional zoom
    let actual_zoom = tile_z.ceil() as u32;
    let scale_factor = 2.0_f64.powf(tile_z - actual_zoom as f64);
    // ... render and scale
}
```

**Pros:**
- ✅ Perfect quality at all zoom levels
- ✅ Bandwidth efficient (exact tiles needed)
- ✅ Smooth zoom with crisp rendering

**Cons:**
- ❌ Complex backend implementation
- ❌ More CPU usage (rendering intermediate zooms)
- ❌ Harder to cache (infinite zoom levels)

---

## Current Implementation

**File:** `static/index.html`

The default viewer uses:
- **512px (@2x) tiles** via `/tile/{z}/{x}/{y}@2x.png`
- **Nearest-neighbor resampling** (`raster-resampling: 'nearest'`)
- **Instant tile display** (`fadeDuration: 0`)
- **MapLibre GL JS 3.6.2**

**To test:**
```bash
# Restart server
./stop-server.sh
cargo run --release -- /path/to/prepared.osm.pbf

# Open default viewer
http://localhost:8080/
```

### Grid Pattern Fix (Dec 2024)

The nearest-neighbor resampling eliminates the grid pattern that was visible with linear interpolation:
- **Before:** Linear resampling created blurry tile boundaries (visible grid during pan)
- **After:** Nearest-neighbor provides sharp edges with no grid artifacts
- **Tradeoff:** Very slight pixelation during fractional zoom (acceptable for line-based map data)

## Performance Impact

**Memory:** Each thread has two renderers (256px + 512px), but only creates them on demand

**Bandwidth:** 512px tiles are 4x larger (512²/256² = 4), but:
- Only used when explicitly requested via @2x URL
- Standard clients still get 256px tiles
- Worth it for the visual quality improvement

**CPU/GPU:** Rendering 512px tiles takes ~4x longer, but:
- Still fast enough for interactive use (headless Vulkan is efficient)
- Thread-local renderers prevent contention
- Can cache rendered tiles on disk if needed

## Recommendation

**Use 512px tiles (@2x)** for best quality:
- Set `tileSize: 512` in MapLibre config
- Request tiles via `/tile/{z}/{x}/{y}@2x.png`
- Enjoy crisp rendering at all zoom levels!

For bandwidth-constrained scenarios, standard 256px tiles are still available and work well.
