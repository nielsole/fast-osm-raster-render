use rust_osm_renderer::data::types::Tile;
use rust_osm_renderer::projection::get_bounding_box;

fn main() {
    // Hamburg tile that should contain our test point
    let tile = Tile::new(1081, 660, 11);
    let bbox = get_bounding_box(&tile);

    println!("Tile 11/1081/660 bounding box:");
    println!("  min: lon={}, lat={}", bbox.min.lon, bbox.min.lat);
    println!("  max: lon={}, lat={}", bbox.max.lon, bbox.max.lat);

    // Known point in Hamburg
    let test_lon = 10.092224;
    let test_lat = 53.677150;

    println!("\nTest point: lon={}, lat={}", test_lon, test_lat);

    // Check if point is within bbox
    let in_bbox = test_lon >= bbox.min.lon && test_lon <= bbox.max.lon
               && test_lat >= bbox.min.lat && test_lat <= bbox.max.lat;
    println!("Point in bbox: {}", in_bbox);

    if !in_bbox {
        println!("❌ ERROR: Test point is NOT in the tile bounding box!");
        return;
    }

    // Test shader transformation
    println!("\n=== Testing Shader Math ===");

    // Step 1: Normalize longitude to 0..1 within bbox
    let x_norm = (test_lon - bbox.min.lon) / (bbox.max.lon - bbox.min.lon);
    println!("1. x normalized (0..1): {}", x_norm);

    // Step 2: Apply Mercator to latitude
    const PI: f64 = 3.14159265359;
    const MAX_LAT: f64 = 85.0511287798;

    let lat2y_mercator = |lat: f64| {
        let lat_clamped = lat.max(-MAX_LAT).min(MAX_LAT);
        let lat_rad = lat_clamped * PI / 180.0;
        (PI / 4.0 + lat_rad / 2.0).tan().ln()
    };

    let y_merc = lat2y_mercator(test_lat);
    let min_y_merc = lat2y_mercator(bbox.min.lat);
    let max_y_merc = lat2y_mercator(bbox.max.lat);
    let y_norm = (y_merc - min_y_merc) / (max_y_merc - min_y_merc);

    println!("2. Mercator values:");
    println!("   test point y_merc: {}", y_merc);
    println!("   bbox min y_merc: {}", min_y_merc);
    println!("   bbox max y_merc: {}", max_y_merc);
    println!("   y normalized (0..1): {}", y_norm);

    // Step 3: Test different NDC conversions
    println!("\n3. NDC conversion options:");

    // Option A: Using projection matrix (current approach)
    let pixel_x = x_norm * 256.0;
    let pixel_y = (1.0 - y_norm) * 256.0;
    println!("   Pixel coords: ({}, {})", pixel_x, pixel_y);

    // Ortho projection: maps 0..256 to -1..1
    let ndc_x_ortho = pixel_x * (2.0 / 256.0) - 1.0;
    let ndc_y_ortho = -(pixel_y * (2.0 / 256.0) - 1.0);
    println!("   Option A (via pixels+ortho): NDC=({}, {})", ndc_x_ortho, ndc_y_ortho);

    // Option B: Direct 0..1 to -1..1
    let ndc_x_direct = x_norm * 2.0 - 1.0;
    let ndc_y_direct = 1.0 - y_norm * 2.0;
    println!("   Option B (direct): NDC=({}, {})", ndc_x_direct, ndc_y_direct);

    // Check if in NDC range
    let in_ndc_a = ndc_x_ortho >= -1.0 && ndc_x_ortho <= 1.0
                && ndc_y_ortho >= -1.0 && ndc_y_ortho <= 1.0;
    let in_ndc_b = ndc_x_direct >= -1.0 && ndc_x_direct <= 1.0
                && ndc_y_direct >= -1.0 && ndc_y_direct <= 1.0;

    println!("\n=== Results ===");
    println!("Option A in NDC range: {} {}", in_ndc_a, if in_ndc_a { "✅" } else { "❌" });
    println!("Option B in NDC range: {} {}", in_ndc_b, if in_ndc_b { "✅" } else { "❌" });

    if in_ndc_a || in_ndc_b {
        println!("\n✅ SUCCESS! Shader math produces valid NDC coordinates!");
        if in_ndc_a {
            println!("Use Option A (current shader with projection matrix)");
        } else {
            println!("Use Option B (direct NDC conversion without projection matrix)");
        }
    } else {
        println!("\n❌ PROBLEM: Both options produce coordinates outside NDC range!");
    }
}
