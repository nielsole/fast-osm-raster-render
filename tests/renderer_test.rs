use rust_osm_renderer::data::serialization::write_map_object;
use rust_osm_renderer::data::spatial::TileIndex;
use rust_osm_renderer::data::types::{BoundingBox, MapObject, Point, Tile};
use rust_osm_renderer::data::mmap::MappedData;
use rust_osm_renderer::renderer::VulkanRenderer;
use std::fs::File;
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
    let horizontal_line = MapObject {
        bounding_box: BoundingBox {
            min: Point::new(center_lon - size, center_lat - 1.0),
            max: Point::new(center_lon + size, center_lat + 1.0),
        },
        points: vec![
            Point::new(center_lon - size, center_lat),
            Point::new(center_lon + size, center_lat),
        ],
    };

    // Vertical line
    let vertical_line = MapObject {
        bounding_box: BoundingBox {
            min: Point::new(center_lon - 1.0, center_lat - size),
            max: Point::new(center_lon + 1.0, center_lat + size),
        },
        points: vec![
            Point::new(center_lon, center_lat - size),
            Point::new(center_lon, center_lat + size),
        ],
    };

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
    let mut renderer = VulkanRenderer::new(tile_index.max_points, true)
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
