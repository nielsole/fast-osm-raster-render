# Phase 0 Completion Guide - MapCSS Proof of Concept

## Status: ~75% Complete

### ✅ What's Done

**WP1: Parser (Complete)**
- `src/style/` module created with parser, types, color, evaluator
- Parses: `way[highway=primary] { color: #ff0000; }`
- 25 unit tests passing
- To use: `parse_mapcss(mapcss_string)` returns `Result<StyleSheet, ParseError>`

**WP2: Data Model (Complete)**
- `MapObject` has `highway_tag: Option<String>` field
- Binary format v1 with 8-byte version header (maintains alignment)
- Loader extracts highway tag from OSM data
- Serialization reads/writes tags correctly

**WP3: Evaluator (Complete)**
- `evaluate_style(stylesheet, ObjectType::Way, tags_map, zoom)` returns `Option<EvaluatedStyle>`
- `EvaluatedStyle` has: `color`, `width`, `opacity`, `z_index` (all Option<>)
- Works with tag HashMap

**WP4: Fragment Shader (Complete)**
- `shaders/tile_styled.frag` created
- Expects `binding = 1` for style uniform (vec4 color)

### 🔨 What's Left - WP4 Integration

## Step 1: Update build.rs to Compile New Shader

**File**: `build.rs`

Find the shader compilation section and add:

```rust
// After compiling tile.frag:
compile_shader(
    &mut compiler,
    "shaders/tile_styled.frag",
    "tile_styled.frag.spv",
    shaderc::ShaderKind::Fragment,
)?;
```

**Test**: Run `cargo build` - should compile shader without errors.

---

## Step 2: Add Style Uniform Buffer Type

**File**: `src/renderer/renderer.rs`

After the existing `UniformBufferObject` struct (around line 15), add:

```rust
/// Style uniform buffer for dynamic coloring (Phase 0)
#[repr(C, align(256))]
#[derive(Copy, Clone)]
struct StyleBufferObject {
    color: [f32; 4],      // RGBA color
    _padding: [f32; 60],  // Pad to 256 bytes
}

impl Default for StyleBufferObject {
    fn default() -> Self {
        StyleBufferObject {
            color: [0.0, 0.0, 0.0, 1.0], // Black default
            _padding: [0.0; 60],
        }
    }
}
```

---

## Step 3: Update Pipeline to Use New Shader

**File**: `src/renderer/pipeline.rs`

### 3a. Update ShaderType Enum

Add a new variant:

```rust
pub enum ShaderType {
    Mercator,
    Simple,
    Debug,
    Styled, // NEW: Phase 0 styled rendering
}
```

### 3b. Update shader loading in `create_graphics_pipeline`

Find the section that loads vertex shader (around line 24-30):

```rust
let vert_shader_module = match shader_type {
    ShaderType::Mercator => load_shader(&device, "tile.vert.spv")?,
    ShaderType::Simple => load_shader(&device, "tile_simple.vert.spv")?,
    ShaderType::Debug => load_shader(&device, "tile_debug.vert.spv")?,
    ShaderType::Styled => load_shader(&device, "tile.vert.spv")?, // Use Mercator
};
```

Then find fragment shader loading (should be after vertex):

```rust
let frag_shader_module = match shader_type {
    ShaderType::Styled => load_shader(&device, "tile_styled.frag.spv")?, // NEW
    _ => load_shader(&device, "tile.frag.spv")?,
};
```

### 3c. Update descriptor set layout

Find `create_descriptor_set_layout` function. It currently creates binding 0 for UBO.

Add binding 1 for style:

```rust
pub fn create_descriptor_set_layout(device: &Device) -> Result<vk::DescriptorSetLayout, VulkanError> {
    let bindings = [
        // Binding 0: Uniform buffer (bbox, tile_size, projection)
        vk::DescriptorSetLayoutBinding::builder()
            .binding(0)
            .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::VERTEX)
            .build(),
        // Binding 1: Style uniform buffer (color) - NEW
        vk::DescriptorSetLayoutBinding::builder()
            .binding(1)
            .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::FRAGMENT)
            .build(),
    ];

    let layout_info = vk::DescriptorSetLayoutCreateInfo::builder()
        .bindings(&bindings);

    // ... rest unchanged
}
```

### 3d. Update descriptor pool

Find descriptor pool creation - add more UNIFORM_BUFFER descriptors:

```rust
let pool_sizes = [
    vk::DescriptorPoolSize {
        ty: vk::DescriptorType::UNIFORM_BUFFER,
        descriptor_count: 20, // Increase from 10 to 20 (for both UBO and Style)
    },
];
```

---

## Step 4: Integrate MapCSS Evaluation in Renderer

**File**: `src/renderer/renderer.rs`

### 4a. Import style module

At the top of the file:

```rust
use crate::style::{parse_mapcss, evaluate_style, ObjectType, StyleSheet};
use std::collections::HashMap;
```

### 4b. Add stylesheet field to VulkanRenderer

In the `VulkanRenderer` struct:

```rust
pub struct VulkanRenderer {
    // ... existing fields ...

    // Phase 0: Optional stylesheet for styling
    stylesheet: Option<StyleSheet>,
}
```

### 4c. Add method to set stylesheet

```rust
impl VulkanRenderer {
    // ... existing methods ...

    /// Set MapCSS stylesheet for rendering (Phase 0)
    pub fn set_stylesheet(&mut self, mapcss: &str) -> Result<(), String> {
        match parse_mapcss(mapcss) {
            Ok(stylesheet) => {
                self.stylesheet = Some(stylesheet);
                Ok(())
            }
            Err(e) => Err(format!("Failed to parse MapCSS: {}", e)),
        }
    }
}
```

### 4d. Update `new` methods to initialize stylesheet

```rust
// In new() and new_with_tile_size(), add to return:
Ok(VulkanRenderer {
    // ... all existing fields ...
    stylesheet: None, // NEW
})
```

### 4e. Update `render_tile` to use MapCSS

Find `render_tile` function. After loading map objects but before building vertex buffer:

```rust
pub fn render_tile(
    &mut self,
    tile: &Tile,
    tile_index: &TileIndex,
    mmap_data: &MappedData,
) -> Result<RgbaImage, VulkanError> {
    // ... existing tile lookup code ...

    // Phase 0: Evaluate style if stylesheet is set
    let default_color = [0.0, 0.0, 0.0, 1.0]; // Black default
    let mut style_color = default_color;

    if let Some(ref stylesheet) = self.stylesheet {
        // For Phase 0, we'll use the first offset to get tags
        // This is simplified - Phase 1 will style per-object
        if let Some(&first_offset) = offsets.first() {
            // Read map object to get tags (using serialization, not mmap for now)
            let mut temp_file = std::fs::File::open("/tmp/rust-osm-renderer-data.bin")
                .map_err(|e| VulkanError::InitializationFailed(format!("Failed to open data: {}", e)))?;

            use crate::data::serialization::read_map_object;
            if let Ok(map_obj) = read_map_object(&mut temp_file, first_offset) {
                // Build tags HashMap from highway_tag
                let mut tags = HashMap::new();
                if let Some(ref highway) = map_obj.highway_tag {
                    tags.insert("highway".to_string(), highway.clone());
                }

                // Evaluate style
                if let Some(evaluated) = evaluate_style(stylesheet, ObjectType::Way, &tags, tile.z) {
                    if let Some(color) = evaluated.color {
                        style_color = color.to_array();
                    }
                }
            }
        }
    }

    // ... continue with vertex buffer building ...
```

### 4f. Create and bind style uniform buffer

After creating the main uniform buffer, create style buffer:

```rust
// Create style uniform buffer (Phase 0)
let style_ubo = StyleBufferObject {
    color: style_color,
    _padding: [0.0; 60],
};

let (style_buffer, style_allocation) = create_buffer(
    &self.context.device,
    &mut self.memory_manager.lock().unwrap(),
    std::mem::size_of::<StyleBufferObject>() as u64,
    vk::BufferUsageFlags::UNIFORM_BUFFER,
    MemoryLocation::CpuToGpu,
)?;

// Write style data
unsafe {
    let style_ptr = self.memory_manager
        .lock()
        .unwrap()
        .get_allocation_info(&style_allocation)
        .unwrap()
        .mapped_ptr
        .unwrap()
        .as_ptr() as *mut StyleBufferObject;
    style_ptr.write(style_ubo);
}
```

### 4g. Update descriptor set to include style buffer

Find where descriptor set is created and written. Add second binding:

```rust
let descriptor_writes = [
    // Binding 0: Uniform buffer (existing)
    vk::WriteDescriptorSet::builder()
        .dst_set(descriptor_set)
        .dst_binding(0)
        .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
        .buffer_info(&[uniform_buffer_info])
        .build(),
    // Binding 1: Style buffer (NEW)
    vk::WriteDescriptorSet::builder()
        .dst_set(descriptor_set)
        .dst_binding(1)
        .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
        .buffer_info(&[vk::DescriptorBufferInfo {
            buffer: style_buffer,
            offset: 0,
            range: std::mem::size_of::<StyleBufferObject>() as u64,
        }])
        .build(),
];

unsafe {
    self.context.device.update_descriptor_sets(&descriptor_writes, &[]);
}
```

### 4h. Clean up style buffer

After rendering completes, free the style buffer:

```rust
// Clean up style buffer
unsafe {
    self.memory_manager
        .lock()
        .unwrap()
        .free(style_allocation)?;
    self.context.device.destroy_buffer(style_buffer, None);
}
```

---

## Step 5: Enable Styled Rendering in Server

**File**: `src/main.rs`

Find where `ShaderType` is selected (around line 23-29):

```rust
let shader_type = if args.contains(&"--debug-shader".to_string()) {
    ShaderType::Debug
} else if args.contains(&"--simple-shader".to_string()) {
    ShaderType::Simple
} else if args.contains(&"--styled-shader".to_string()) {
    ShaderType::Styled  // NEW
} else {
    ShaderType::Mercator
};
```

---

## Step 6: Test Phase 0

### 6a. Build and check for errors

```bash
cargo build --release
```

Fix any compilation errors (types, missing imports, etc.)

### 6b. Create test MapCSS file

```bash
cat > test_style.mapcss << 'EOF'
way[highway=primary] { color: #ff0000; }
EOF
```

### 6c. Test with styled shader

Start server with styled shader:

```bash
cargo run --release -- prepared.osm.pbf --styled-shader
```

In the renderer initialization, load the test stylesheet:

```rust
// In main.rs or wherever renderer is created:
renderer.set_stylesheet("way[highway=primary] { color: #ff0000; }").unwrap();
```

### 6d. Render test tile

```bash
# Use render_tile example for fast testing
cargo run --example render_tile -- prepared.osm.pbf 11 1081 660 test.png

# View the image
# Primary roads should be RED instead of black!
```

### 6e. Performance benchmark

```bash
# Time a single render
time cargo run --release --example render_tile -- prepared.osm.pbf 11 1081 660 test.png

# Should still be < 50ms
```

---

## Step 7: Verification Checklist

- [ ] All code compiles without errors
- [ ] Shader compiles in build.rs
- [ ] Can parse MapCSS: `way[highway=primary] { color: #ff0000; }`
- [ ] Hamburg tile (11/1081/660) renders with **red** primary roads (not black)
- [ ] Rendering time < 50ms
- [ ] No crashes or Vulkan errors
- [ ] Run `cargo test` - existing tests still pass

---

## Common Issues & Fixes

### Issue: "Descriptor pool out of memory"
**Fix**: Increase descriptor pool size in pipeline.rs (descriptor_count in pool_sizes)

### Issue: "Shader binding mismatch"
**Fix**: Ensure binding numbers match between:
- `tile_styled.frag`: `layout(set = 0, binding = 1)`
- Descriptor set layout: `.binding(1)`
- Descriptor write: `.dst_binding(1)`

### Issue: "All roads still black"
**Fix**: Check that:
1. StyleBufferObject color is being set correctly
2. Descriptor set is written with style buffer
3. Using ShaderType::Styled (not Mercator)
4. MapCSS is parsed and evaluated

### Issue: Rendering slower than 50ms
**Fix**:
- Ensure reading map objects is efficient (don't read all objects)
- Cache stylesheet parsing (don't re-parse every frame)
- Profile with `cargo flamegraph`

---

## Next Steps After Phase 0

Once Phase 0 works:

1. **Verify success criteria**:
   - ✓ Parse MapCSS
   - ✓ Red primary roads
   - ✓ <50ms rendering
   - ✓ Tests pass

2. **Start Phase 1**:
   - Expand to full tag storage (HashMap)
   - Per-object styling (not just first object)
   - Multiple rules and selectors
   - Width, opacity, z-index properties
   - Remove is_important_way() filter
   - Performance optimization!

3. **Update TodoList**:
   ```bash
   # Mark Phase 0 complete
   # Begin Phase 1 todos
   ```

---

## Code Review Tips

Before committing:
- Run `cargo fmt`
- Run `cargo clippy`
- Verify tests: `cargo test`
- Test visual output manually
- Check performance: `time cargo run --example render_tile ...`
- Document any changes to binary format
- Update CLAUDE.md if needed

---

## Phase 0 Complete When...

✅ You can run:
```bash
cargo run --release -- prepared.osm.pbf --styled-shader
```

And primary roads render in **RED** (not black) while maintaining <50ms performance.

**That's the proof of concept! 🎉**

Then Phase 1 begins the real work of full MapCSS support with performance optimization.

---

## Questions?

If stuck:
1. Check Vulkan validation layers: `cargo run` (debug build)
2. Add log statements: `log::info!("Color: {:?}", style_color);`
3. Use RenderDoc to capture frame and inspect uniforms
4. Verify descriptor sets are bound correctly
5. Check shader was compiled: `ls target/*/build/*/out/*.spv`

Good luck! This is the hardest part - once rendering works, everything else is incremental.
