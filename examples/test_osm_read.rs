use osmpbf::{Element, ElementReader};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = "/home/nokadmin/projects/go-gl-osm/prepared.osm.pbf";
    let reader = ElementReader::from_path(path)?;

    let mut node_count = 0;
    let mut way_count = 0;

    println!("Reading {}...", path);

    reader.for_each(|element| {
        match element {
            Element::Node(node) => {
                node_count += 1;
                if node_count <= 5 {
                    println!("Node {}: lat={}, lon={}", node.id(), node.lat(), node.lon());
                }
            }
            Element::Way(way) => {
                way_count += 1;
                if way_count <= 5 {
                    println!("\nWay {}: {} node refs", way.id(), way.refs().count());

                    // Check what refs() actually returns
                    let refs: Vec<i64> = way.refs().collect();
                    println!("  First 3 refs (node IDs): {:?}", &refs[..refs.len().min(3)]);
                }
            }
            _ => {}
        }

        if node_count >= 100 && way_count >= 10 {
            return;
        }
    })?;

    println!("\n=== Summary ===");
    println!("Nodes found: {}", node_count);
    println!("Ways found: {}", way_count);

    Ok(())
}
