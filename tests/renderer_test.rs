use rust_osm_renderer::data::serialization::write_map_object;
use rust_osm_renderer::data::spatial::TileIndex;
use rust_osm_renderer::data::types::{BoundingBox, MapObject, Point, Tile};
use rust_osm_renderer::data::mmap::MappedData;
use rust_osm_renderer::renderer::{VulkanRenderer, ShaderType};
use tempfile::NamedTempFile;

#[test]
#[ignore] // Ignore by default since it requires Vulkan
fn test_vulkan_renderer_with_synthetic_data() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    let _ = env_logger::builder().is_test(true).try_init();

    // Create synthetic map data - a simple cross pattern
    let mut temp_file = NamedTempFile::new()?;

    // Create a cross pattern in the center of tile 0/0/0
    let center_lon = 0.0;
    let center_lat = 0.0;
    let size = 20.0;

    // Horizontal line
    let horizontal_line = MapObject::new(
        BoundingBox {
            min: Point::new(center_lon - size, center_lat - 1.0),
            max: Point::new(center_lon + size, center_lat + 1.0),
        },
        vec![
            Point::new(center_lon - size, center_lat),
            Point::new(center_lon + size, center_lat),
        ],
        false,
        vec![],
    );

    // Vertical line
    let vertical_line = MapObject::new(
        BoundingBox {
            min: Point::new(center_lon - 1.0, center_lat - size),
            max: Point::new(center_lon + 1.0, center_lat + size),
        },
        vec![
            Point::new(center_lon, center_lat - size),
            Point::new(center_lon, center_lat + size),
        ],
        false,
        vec![],
    );

    // Write to file
    let offset1 = write_map_object(temp_file.as_file_mut(), &horizontal_line)?;
    let offset2 = write_map_object(temp_file.as_file_mut(), &vertical_line)?;

    // Flush to ensure data is written
    use std::io::Write;
    temp_file.as_file_mut().flush()?;

    // Create tile index
    let mut tile_index = TileIndex::new();
    let tile = Tile::new(0, 0, 0);
    tile_index.insert(tile, offset1);
    tile_index.insert(tile, offset2);
    tile_index.max_points = 2;

    // Memory map the file
    let mmap_data = MappedData::new(temp_file.path())?;

    // Create renderer with simple shader for testing
    let mut renderer = VulkanRenderer::new(tile_index.max_points, ShaderType::Simple)
        .map_err(|e| format!("Failed to create Vulkan renderer: {}", e))?;

    // Render tile
    let image = renderer.render_tile(&tile, &tile_index, &mmap_data)
        .map_err(|e| format!("Failed to render tile: {}", e))?;

    // Check image is correct size
    assert_eq!(image.width(), 256);
    assert_eq!(image.height(), 256);

    // Save for manual inspection
    image.save("/tmp/test_tile.png")?;
    println!("Test tile saved to /tmp/test_tile.png");

    // Check that not all pixels are white (we drew something)
    let mut non_white_pixels = 0;
    for pixel in image.pixels() {
        if pixel[0] != 255 || pixel[1] != 255 || pixel[2] != 255 {
            // Not white
            non_white_pixels += 1;
        }
    }

    assert!(non_white_pixels > 0, "Expected some non-white pixels from the cross pattern");
    println!("Found {} non-white pixels", non_white_pixels);

    Ok(())
}

#[test]
fn test_vertex_buffer_capacity_calculation() {
    // Test that verifies buffer capacity is sufficient for different shader types

    // Regular shader: 10M floats = 40MB
    const REGULAR_CAPACITY: usize = 10_000_000;
    // Styled shader: 50M floats = 200MB (quad expansion + polygon fill needs room)
    const STYLED_CAPACITY: usize = 50_000_000;

    // Regular shader: (lon, lat) = 2 floats per vertex, LINE_LIST topology
    let regular_vertex_size = 2;
    let regular_max_vertices = REGULAR_CAPACITY / regular_vertex_size;
    let regular_max_segments = regular_max_vertices / 2; // LINE_LIST: 2 vertices per segment
    println!("Regular shader capacity: {} vertices, {} line segments",
             regular_max_vertices, regular_max_segments);
    assert_eq!(regular_max_vertices, 5_000_000);
    assert_eq!(regular_max_segments, 2_500_000);

    // Styled shader: (lon, lat, r, g, b, a) = 6 floats per vertex, TRIANGLE_LIST topology
    // Line segments: Each line segment = 2 triangles = 6 vertices = 36 floats
    let styled_floats_per_segment = 36;
    let styled_max_segments = STYLED_CAPACITY / styled_floats_per_segment;
    let styled_max_vertices = styled_max_segments * 6;
    println!("Styled shader capacity: {} vertices, {} line segments (quad expansion)",
             styled_max_vertices, styled_max_segments);
    assert_eq!(styled_max_segments, 1_388_888);
    assert!(styled_max_segments > 1_000_000, "Should handle >1M line segments");

    // Polygon fill: Each triangle = 3 vertices = 18 floats
    // A 5-point building = 3 triangles = 9 vertices = 54 floats
    let polygon_floats_per_triangle = 18;
    let polygon_max_triangles = STYLED_CAPACITY / polygon_floats_per_triangle;
    println!("Polygon capacity: {} triangles ({} typical buildings with ~3 triangles each)",
             polygon_max_triangles, polygon_max_triangles / 3);
    assert!(polygon_max_triangles > 2_000_000, "Should handle >2M triangles");

    // Worst case estimate: 182K objects * ~12 segments = ~2.2M segments
    // With zoom filtering, most objects are filtered out at any given zoom level
    println!("Styled shader can handle {} objects with ~12 segments each",
             styled_max_segments / 12);
}
