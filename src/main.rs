use rust_osm_renderer::data::loader::load_osm_data_cached;
use rust_osm_renderer::data::mmap::MappedData;
use rust_osm_renderer::renderer::ShaderType;
use rust_osm_renderer::server::{create_app, AppState};
use std::env;
use std::path::Path;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // Parse command line arguments
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <osm-file.pbf> [--simple-shader|--debug-shader|--styled-shader]", args[0]);
        eprintln!("  --simple-shader: Use simplified linear projection (better for debugging)");
        eprintln!("  --debug-shader: Output all vertices at center (pipeline test)");
        eprintln!("  --styled-shader: Use MapCSS styling (Phase 0)");
        std::process::exit(1);
    }

    let osm_path = &args[1];
    let shader_type = if args.iter().any(|s| s == "--simple-shader") {
        ShaderType::Simple
    } else if args.iter().any(|s| s == "--debug-shader") {
        ShaderType::Debug
    } else if args.iter().any(|s| s == "--styled-shader") {
        ShaderType::Styled
    } else {
        ShaderType::Mercator
    };
    if !Path::new(osm_path).exists() {
        eprintln!("Error: OSM file not found: {}", osm_path);
        std::process::exit(1);
    }

    log::info!("Starting OSM tile renderer...");
    log::info!("Loading OSM data from: {}", osm_path);

    // Load OSM data and build spatial index (with disk caching)
    // We index up to zoom 15, but can render higher zoom levels by using parent tiles
    let max_z = 15;
    log::info!("Loading OSM data (max zoom: {})...", max_z);
    let (tile_index, data_path) = load_osm_data_cached(osm_path, max_z)?;

    log::info!(
        "OSM data loaded: {} tiles, max {} points per way",
        tile_index.len(),
        tile_index.max_points
    );

    // Memory-map the data file
    log::info!("Memory-mapping data file...");
    let mmap_data = MappedData::new(&data_path)?;
    log::info!("Data file size: {} bytes", mmap_data.len());

    // Set default MapCSS stylesheet for styled shader
    let stylesheet = if shader_type == ShaderType::Styled {
        Some(r#"
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
        "#.to_string())
    } else {
        None
    };

    // Create app state
    let app_state = AppState {
        data: Arc::new(tile_index),
        mmap: Arc::new(mmap_data),
        shader_type,
        stylesheet,
    };

    // Create HTTP server
    let app = create_app(app_state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    log::info!("Server listening on http://0.0.0.0:8080");
    log::info!("Try: http://0.0.0.0:8080/tile/0/0/0.png");

    axum::serve(listener, app).await?;

    Ok(())
}
