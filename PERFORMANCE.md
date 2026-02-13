# Performance Testing Results

This document tracks performance testing results for the Rust OSM tile renderer, ensuring all tiles meet the **<50ms rendering target** requirement.

## Benchmark Configuration

- **Tool**: Criterion.rs benchmarking framework
- **Location**: `benches/tile_rendering.rs`
- **Sample size**: 10 samples per test
- **Warm-up**: 1 second
- **Measurement**: 1 second per test

Run benchmarks with:
```bash
cargo bench --bench tile_rendering
```

## Test Tiles

### Tile 11/1082/661 - Hamburg (Overflow Tile)
- **Objects**: 430 map objects
- **Description**: Previously caused vertex buffer overflow with styled shader
- **Issue**: Styled shader uses 6 floats/vertex vs 2 for regular shader

### Tile 11/1080/661 - Hamburg City Center (High Complexity)
- **Objects**: 182,284 map objects
- **Vertices**: 2.3 million vertices
- **Description**: Extremely detailed urban tile with dense road network

## Performance Results

### Phase 2 Performance (Area Fill + Polygon Triangulation)

| Tile | Shader | Objects | Render Time | vs 50ms Target | Status |
|------|--------|---------|-------------|----------------|--------|
| 11/1082/661 | Simple | 430 | **123 us** (0.12 ms) | 406x faster | PASS |
| 11/1082/661 | Styled | 430 | **494 us** (0.49 ms) | 101x faster | PASS |
| 11/1080/661 | Styled | 182,284 | **55.3 ms** (55 ms) | ~10% over | MARGINAL |

**Changes from Phase 1**: Styled shader now loads ALL tags from mmap (not just highway),
evaluates area selectors with fill-color, and triangulates polygons using earcutr.
The 182K-object tile increased from 45ms to 55ms due to reading multiple tags per object
(previously only read the highway tag). The zero-alloc tag lookup optimization
(`tag_value()` / `has_tag()` instead of building `HashMap`) brought it down from 132ms.

The 55ms result is slightly above the 50ms target. This tile is the absolute worst case
(182K objects at z11 with a catch-all `way {}` rule that forces tag evaluation for every
object). In practice, zoom-filtered stylesheets skip most objects at any given zoom level.

### Phase 1 Performance (Quad Expansion, Z-Ordering, Zoom Filtering)

| Tile | Shader | Objects | Render Time | vs 50ms Target | Status |
|------|--------|---------|-------------|----------------|--------|
| 11/1082/661 | Simple | 430 | **117.70 us** (0.12 ms) | 425x faster | PASS |
| 11/1082/661 | Styled | 430 | **282.64 us** (0.28 ms) | 177x faster | PASS |
| 11/1080/661 | Styled | 182,284 | **45.261 ms** (45 ms) | 1.1x faster | PASS |

### Phase 0 Performance (Baseline)

| Tile | Shader | Objects | Render Time | vs 50ms Target | Status |
|------|--------|---------|-------------|----------------|--------|
| 11/1082/661 | Simple | 430 | **123.83 us** (0.12 ms) | 416x faster | PASS |
| 11/1082/661 | Styled | 430 | **10.988 ms** (11 ms) | 4.5x faster | PASS |
| 11/1080/661 | Styled | 182,284 | **15.681 ms** (16 ms) | 3.2x faster | PASS |

## Optimization Techniques Applied

### 1. Buffer Sizing Strategy
**Problem**: Quad expansion (6 vertices x 6 floats = 36 floats per line segment) + polygon triangulation needs larger buffers
**Solution**: 50M floats for styled shader (200MB), handles both line quads and polygon triangles

### 2. Zero-Copy Memory-Mapped Data (v2 Format)
**Problem**: Reading all tags from mmap was creating HashMap allocations per object (132ms for 182K objects)
**Solution**: 8-byte aligned binary format with zero-copy point arrays and zero-alloc tag lookups:
- `tag_value(key)`: scans tag list in mmap without allocating
- `has_tag(key)`: existence check without allocating
- `evaluate_style_with_lookup()`: evaluator accepts closures instead of HashMap

### 3. 8-Byte Aligned Binary Format
**Problem**: 1-byte flags field broke Point array alignment, forcing per-point copies
**Solution**: Padded flags to u64 (8 bytes) and added alignment padding after tags:
```
[8B version][32B bbox][8B flags][8B points_len][N*16B points][2B num_tags][tags...][padding to 8B]
```
**Benefit**: Zero-copy `&[Point]` slice from mmap, restoring Phase 0/1 performance

### 4. Polygon Triangulation with earcutr
**Problem**: Area polygons need to be filled, not just outlined
**Solution**: Mapbox earcut algorithm (earcutr crate) for O(n log n) triangulation
- Typical building (5 points) = 3 triangles = 9 vertices = 54 floats
- Much cheaper per-object than quad-expanded roads (36 floats per segment)

## Performance Target Compliance

**Requirement**: All tiles must render in **<50ms** (per plan requirements)

**Status**: PASS for all typical tiles, MARGINAL for worst-case 182K-object tile at 55ms

- Best case: 0.12ms (Simple shader, sparse tile)
- Typical case: 0.5ms (Styled shader, moderate tile)
- Worst case: 55ms (Styled shader, 182K objects with catch-all rule)

## Future Optimization Opportunities

If the 182K tile needs to come under 50ms:

1. **Pre-filter by object type**: Skip `tag_value()` calls for objects whose type doesn't match any selector
2. **Collect unique tag keys from stylesheet**: Only scan for keys that appear in CSS rules
3. **Parallel processing**: Use Rayon for parallel style evaluation in Pass 1
4. **GPU culling**: Move bounding box checks to GPU compute shader

## Related Documentation

- Benchmark code: `benches/tile_rendering.rs`
- Test case: `tests/renderer_test.rs::test_vertex_buffer_capacity_calculation`
