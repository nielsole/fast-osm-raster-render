use osmpbf::{Element, ElementReader};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = "/home/nokadmin/projects/go-gl-osm/prepared.osm.pbf";
    let reader = ElementReader::from_path(path)?;

    println!("Testing node_locations() method...\n");

    let mut way_count = 0;
    let mut ways_with_locations = 0;

    reader.for_each(|element| {
        if let Element::Way(way) = element {
            way_count += 1;

            // Try the node_locations() method
            let locations: Vec<_> = way.node_locations().collect();

            if !locations.is_empty() {
                ways_with_locations += 1;

                if ways_with_locations <= 3 {
                    println!("Way {} has {} locations:", way.id(), locations.len());
                    for (i, loc) in locations.iter().take(3).enumerate() {
                        println!("  Point {}: lat={}, lon={}", i, loc.lat(), loc.lon());
                    }
                    println!();
                }
            }

            if way_count >= 1000 {
                return;
            }
        }
    })?;

    println!("=== Summary ===");
    println!("Ways checked: {}", way_count);
    println!("Ways with locations: {}", ways_with_locations);
    println!("Success rate: {:.1}%", (ways_with_locations as f64 / way_count as f64) * 100.0);

    if ways_with_locations > 0 {
        println!("\n✅ SUCCESS! The osmpbf crate CAN read coordinates from osmium-processed files!");
    } else {
        println!("\n❌ FAILED: No coordinates found");
    }

    Ok(())
}
