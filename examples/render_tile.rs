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
        eprintln!("Usage: {} <osm-file.pbf> <z> <x> <y> [output.png] [--simple-shader|--debug-shader|--styled-shader]", args[0]);
        eprintln!("Example: {} prepared.osm.pbf 11 1081 660 hamburg.png", args[0]);
        eprintln!("  --styled-shader: Use MapCSS styling (renders primary roads in red)");
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
    } else if args.iter().any(|s| s == "--styled-shader") {
        ShaderType::Styled
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

    // Set MapCSS stylesheet for styled shader
    if shader_type == ShaderType::Styled {
        let stylesheet = r#"
            area[landuse=residential]  { fill-color: #e0dfdf; z-index: -5; }
            area[landuse=forest]       { fill-color: #add19e; z-index: -4; }
            area[landuse=grass]        { fill-color: #cdebb0; z-index: -4; }
            area[landuse=commercial]   { fill-color: #f2dad9; z-index: -5; }
            area[landuse=industrial]   { fill-color: #ebdbe8; z-index: -5; }
            area[natural=water]        { fill-color: #aad3df; z-index: -2; }
            area[waterway=riverbank]   { fill-color: #aad3df; z-index: -2; }
            area[natural=wood]         { fill-color: #add19e; z-index: -3; }
            area[leisure=park]         { fill-color: #c8facc; z-index: -3; }
            area[leisure=garden]       { fill-color: #cdebb0; z-index: -3; }
            area|z13-[building]        { fill-color: #d9d0c9; z-index: 1; }
            area[amenity=parking]      { fill-color: #eeeeee; z-index: -1; }
            way { color: #999999; width: 1; z-index: 0; }
            way|z6-[highway=motorway]       { color: #cf3030; width: 5; z-index: 9; }
            way|z8-[highway=trunk]          { color: #d85f2a; width: 4; z-index: 8; }
            way|z8-[highway=primary]        { color: #d4a012; width: 3; z-index: 7; }
            way|z10-[highway=secondary]     { color: #a4a41a; width: 2.5; z-index: 6; }
            way|z11-[highway=tertiary]      { color: #b0b0b0; width: 2; z-index: 5; }
            way|z12-[highway=residential]   { color: #b0b0b0; width: 1.5; z-index: 4; }
            way|z12-[highway=unclassified]  { color: #b0b0b0; width: 1.5; z-index: 4; }
            way|z14-[highway=service]       { color: #c0c0c0; width: 1; z-index: 3; }
            way|z13-[highway=living_street] { color: #c0c0c0; width: 1; z-index: 3; }
            way|z13-[highway=motorway_link] { color: #cf3030; width: 2; z-index: 8; }
            way|z13-[highway=trunk_link]    { color: #d85f2a; width: 2; z-index: 7; }
            way|z13-[highway=primary_link]  { color: #d4a012; width: 2; z-index: 6; }
        "#;
        log::info!("Setting MapCSS stylesheet with area fills and road hierarchy");
        renderer.set_stylesheet(stylesheet)
            .expect("Failed to set stylesheet");
    }

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
