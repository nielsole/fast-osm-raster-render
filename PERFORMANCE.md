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

**Fix Applied**: Increased vertex buffer capacity from 10M to 30M floats for styled shader (3x increase to maintain same vertex capacity)

### Tile 11/1080/661 - Hamburg City Center (High Complexity)
- **Objects**: 182,284 map objects
- **Vertices**: 2.3 million vertices
- **Description**: Extremely detailed urban tile with dense road network
- **Issue**: Took 2.7 seconds to render (54x over budget)

**Fix Applied**: Eliminated file I/O bottleneck by reading tags from memory-mapped data

## Performance Results

### Current Performance (After Optimization)

| Tile | Shader | Objects | Render Time | vs 50ms Target | Status |
|------|--------|---------|-------------|----------------|--------|
| 11/1082/661 | Simple | 430 | **123.83 µs** (0.12 ms) | 416x faster | ✅ PASS |
| 11/1082/661 | Styled | 430 | **10.988 ms** (11 ms) | 4.5x faster | ✅ PASS |
| 11/1080/661 | Styled | 182,284 | **15.681 ms** (16 ms) | 3.2x faster | ✅ PASS |

### Historical Performance Issues

#### Issue 1: Vertex Buffer Overflow (FIXED)
**Date**: 2026-01-11
**Tile**: 11/1082/661
**Problem**: Styled shader caused buffer overflow due to 3x larger vertex size
**Root Cause**: Hardcoded 10M float capacity insufficient for styled shader's 6-float vertices
**Solution**: Dynamic capacity based on shader type (30M floats for styled)
**Files Changed**: `src/renderer/renderer.rs:126-129`

#### Issue 2: Extreme Slowdown on Dense Tiles (FIXED)
**Date**: 2026-01-11
**Tile**: 11/1080/661
**Problem**: 2.7 second render time (54x over budget)
**Root Cause**: Opening file 182,284 times (once per object) to read tags
**Solution**: Read tags directly from memory-mapped data
**Performance Gain**: **172x speedup** (2700ms → 15.7ms)
**Files Changed**:
- `src/data/mmap.rs:50` - Added `tags_ptr` field to MapObjectView
- `src/data/mmap.rs:124-147` - Added `highway_tag()` method
- `src/renderer/renderer.rs:365-388` - Use memory-mapped tags instead of file I/O

## Optimization Techniques Applied

### 1. Buffer Sizing Strategy
**Problem**: Fixed buffer size doesn't account for different vertex formats
**Solution**: Calculate capacity based on shader type:
```rust
let base_capacity = 10_000_000;  // 10M floats for regular shader
let vertex_buffer_capacity = match shader_type {
    ShaderType::Styled => base_capacity * 3,  // 30M floats (maintain same vertex count)
    _ => base_capacity,
};
```
**Benefit**: Same effective capacity across all shader types

### 2. Memory-Mapped Tag Reading
**Problem**: Per-object file opens create massive I/O overhead
**Solution**: Read tags directly from mmap via pointer arithmetic:
```rust
pub fn highway_tag(&self) -> Option<String> {
    unsafe {
        let tag_present = self.tags_ptr.read();
        if tag_present == 1 {
            let tag_len = self.tags_ptr.add(1).cast::<u32>().read_unaligned() as usize;
            let tag_bytes = std::slice::from_raw_parts(self.tags_ptr.add(5), tag_len);
            String::from_utf8(tag_bytes.to_vec()).ok()
        } else {
            None
        }
    }
}
```
**Benefit**: Zero file I/O during rendering, 172x speedup on dense tiles

## Performance Target Compliance

**Requirement**: All tiles must render in **<50ms** (per plan requirements)

**Status**: ✅ **ALL TILES PASSING**

- Best case: 0.12ms (Simple shader, sparse tile)
- Worst case: 15.7ms (Styled shader, 182K objects)
- **Headroom**: 68% under budget even for worst case

## Testing Recommendations

### Regular Performance Testing
Run benchmarks after any changes to:
- Vertex buffer building (`build_vertex_buffer`)
- MapCSS evaluation (`evaluate_style`)
- Memory-mapped data reading
- Shader pipeline configuration

### Test Coverage
Current benchmark covers:
- ✅ Overflow edge case (large vertices per object)
- ✅ High object count (182K objects)
- ✅ Both shader types (Simple and Styled)

Consider adding:
- Medium complexity tiles (1K-10K objects)
- Higher zoom levels (16-18)
- Different geometry types (polygons when Phase 3 implemented)

## Future Optimization Opportunities

As noted in the plan, if performance degrades during Phase 1+ implementation:

1. **Selector Evaluation Caching** - Cache evaluated styles per object
2. **Parallel Processing** - Use Rayon for parallel rule evaluation
3. **GPU Culling** - Move bounding box checks to GPU
4. **Draw Indirect** - GPU-driven rendering with compute shader

Current implementation leaves significant headroom (68% under target), allowing these optimizations to be deferred until actually needed.

## Related Documentation

- Benchmark code: `benches/tile_rendering.rs`
- MapCSS plan: `/home/nokadmin/.claude/plans/glowing-strolling-dragon.md`
- Test case: `tests/renderer_test.rs::test_vertex_buffer_capacity_calculation`
