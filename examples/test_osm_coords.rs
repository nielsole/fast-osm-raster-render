use osmpbf::{Element, ElementReader};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = "/home/nokadmin/projects/go-gl-osm/prepared.osm.pbf";
    let reader = ElementReader::from_path(path)?;

    println!("Testing osmpbf API for coordinates...\n");

    reader.for_each(|element| {
        if let Element::Way(way) = element {
            println!("Way {}", way.id());

            // Try different methods to get coordinates
            println!("  refs() returns: {:?}", way.refs().take(2).collect::<Vec<_>>());

            // Check if there are any location-related methods
            // The osmpbf crate might expose these differently

            // Try raw_refs which might contain more data
            let raw_refs = way.raw_refs();
            println!("  raw_refs len: {}", raw_refs.len());

            // Check the struct fields - maybe there's a coordinates method?
            // Let's see if we can access node data directly

            return; // Just check first way
        }
    })?;

    // Let's also check what the Way struct actually contains
    println!("\n=== Checking Way struct capabilities ===");
    println!("Available on Way:");
    println!("  - id()");
    println!("  - refs() -> Iterator<Item=i64>");
    println!("  - tags()");
    println!("  - raw_refs() -> &[i64]");
    println!("  - info()");

    println!("\nThe osmpbf crate does NOT expose coordinates even from osmium-processed files!");
    println!("We need to use a different approach or crate.");

    Ok(())
}
