use rust_osm_renderer::data::loader::load_osm_data;
use rust_osm_renderer::data::mmap::MappedData;
use rust_osm_renderer::data::types::Tile;
use rust_osm_renderer::renderer::{VulkanRenderer, ShaderType};
use std::env;
use tempfile::NamedTempFile;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args: Vec<String> = env::args().collect();
    if args.len() < 5 {
        eprintln!("Usage: {} <osm-file.pbf> <z> <x> <y> [output.png] [--simple-shader|--debug-shader]", args[0]);
        eprintln!("Example: {} prepared.osm.pbf 11 1081 660 hamburg.png", args[0]);
        std::process::exit(1);
    }

    let osm_path = &args[1];
    let z: u32 = args[2].parse()?;
    let x: u32 = args[3].parse()?;
    let y: u32 = args[4].parse()?;
    let output_path = args.get(5).map(|s| s.as_str()).unwrap_or("output.png");

    let shader_type = if args.iter().any(|s| s == "--simple-shader") {
        ShaderType::Simple
    } else if args.iter().any(|s| s == "--debug-shader") {
        ShaderType::Debug
    } else {
        ShaderType::Mercator
    };

    log::info!("Rendering tile {}/{}/{} from {}", z, x, y, osm_path);

    // Load OSM data
    let mut temp_file = NamedTempFile::new()?;
    log::info!("Loading OSM data...");
    // Index up to zoom 15, higher zooms will use parent tile data
    let tile_index = load_osm_data(osm_path, 15, temp_file.as_file_mut())?;
    log::info!("Loaded {} tiles", tile_index.len());

    // Memory-map the data
    let mmap_data = MappedData::new(temp_file.path())?;

    // Create renderer
    log::info!("Creating {:?} shader renderer...", shader_type);
    let mut renderer = VulkanRenderer::new(tile_index.max_points, shader_type)?;

    // Render tile
    let tile = Tile::new(x, y, z);
    log::info!("Rendering...");
    let image = renderer.render_tile(&tile, &tile_index, &mmap_data)?;

    // Save
    image.save(output_path)?;
    log::info!("Saved to {}", output_path);

    // Check if it has content
    let non_white = image.pixels().filter(|p| p[0] != 255 || p[1] != 255 || p[2] != 255).count();
    let total = (image.width() * image.height()) as usize;
    let pct = (non_white as f64 / total as f64) * 100.0;

    println!("\n{}", "=".repeat(60));
    println!("RESULT: {} non-white pixels / {} total ({:.1}%)", non_white, total, pct);
    println!("{}", "=".repeat(60));

    if non_white > 100 {
        println!("✅ SUCCESS! Tile rendered with content!");
    } else {
        println!("❌ WARNING: Tile is mostly white ({} pixels)", non_white);
    }

    Ok(())
}
