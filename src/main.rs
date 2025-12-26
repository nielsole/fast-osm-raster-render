use rust_osm_renderer::data::loader::load_osm_data;
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
        eprintln!("Usage: {} <osm-file.pbf> [--simple-shader|--debug-shader]", args[0]);
        eprintln!("  --simple-shader: Use simplified linear projection (better for debugging)");
        eprintln!("  --debug-shader: Output all vertices at center (pipeline test)");
        std::process::exit(1);
    }

    let osm_path = &args[1];
    let shader_type = if args.iter().any(|s| s == "--simple-shader") {
        ShaderType::Simple
    } else if args.iter().any(|s| s == "--debug-shader") {
        ShaderType::Debug
    } else {
        ShaderType::Mercator
    };
    if !Path::new(osm_path).exists() {
        eprintln!("Error: OSM file not found: {}", osm_path);
        std::process::exit(1);
    }

    log::info!("Starting OSM tile renderer...");
    log::info!("Loading OSM data from: {}", osm_path);

    // Create temporary file for map objects
    let temp_file_path = "/tmp/rust-osm-renderer-data.bin";
    let mut temp_file = std::fs::File::create(temp_file_path)?;

    // Load OSM data and build spatial index
    // We index up to zoom 15, but can render higher zoom levels by using parent tiles
    let max_z = 15;
    log::info!("Loading OSM data (max zoom: {})...", max_z);
    let tile_index = load_osm_data(osm_path, max_z, &mut temp_file)?;

    // Ensure data is flushed
    use std::io::Write;
    temp_file.flush()?;
    drop(temp_file);

    log::info!(
        "OSM data loaded: {} tiles, max {} points per way",
        tile_index.len(),
        tile_index.max_points
    );

    // Memory-map the temp file
    log::info!("Memory-mapping data file...");
    let mmap_data = MappedData::new(temp_file_path)?;
    log::info!("Data file size: {} bytes", mmap_data.len());

    // Create app state
    let app_state = AppState {
        data: Arc::new(tile_index),
        mmap: Arc::new(mmap_data),
        shader_type,
    };

    // Create HTTP server
    let app = create_app(app_state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    log::info!("Server listening on http://0.0.0.0:8080");
    log::info!("Try: http://0.0.0.0:8080/tile/0/0/0.png");

    axum::serve(listener, app).await?;

    Ok(())
}
