use rust_osm_renderer::data::loader::load_osm_data;
use rust_osm_renderer::data::types::Tile;
use tempfile::NamedTempFile;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .init();

    let osm_path = "/home/nokadmin/projects/go-gl-osm/prepared.osm.pbf";
    let mut temp_file = NamedTempFile::new()?;

    println!("Loading OSM data from {}...", osm_path);
    let tile_index = load_osm_data(osm_path, 15, temp_file.as_file_mut())?;

    println!("\n=== Loading Summary ===");
    println!("Total tiles indexed: {}", tile_index.len());
    println!("Max points in any way: {}", tile_index.max_points);

    // Check tile 0/0/0
    let root_tile = Tile::new(0, 0, 0);
    if let Some(offsets) = tile_index.get(&root_tile) {
        println!("\n✅ Tile 0/0/0 has {} map objects", offsets.len());
    } else {
        println!("\n❌ Tile 0/0/0 has NO data!");
    }

    // Check a few other tiles
    for z in 0..5 {
        let tile = Tile::new(z, 0, 0);
        if let Some(offsets) = tile_index.get(&tile) {
            println!("Tile {}/{}/{} has {} map objects", z, 0, 0, offsets.len());
        }
    }

    println!("\n✅ SUCCESS! OSM data loaded successfully!");

    Ok(())
}
