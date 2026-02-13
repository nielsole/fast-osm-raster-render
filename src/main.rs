use rust_osm_renderer::data::loader::{load_low_zoom_simplified_cached, load_osm_data_cached};
use rust_osm_renderer::data::mmap::MappedData;
use rust_osm_renderer::labels::load_place_labels_cached;
use rust_osm_renderer::renderer::ShaderType;
use rust_osm_renderer::server::{create_app, AppState};
use std::env;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

fn print_usage(program: &str) {
    eprintln!(
        "Usage: {} <osm-file.pbf> [--simple-shader|--debug-shader|--styled-shader] [--load-stats-only] [--port <PORT>]",
        program
    );
    eprintln!("  --simple-shader: Use simplified linear projection (better for debugging)");
    eprintln!("  --debug-shader: Output all vertices at center (pipeline test)");
    eprintln!("  --styled-shader: Use MapCSS styling");
    eprintln!("  --port <PORT>: HTTP port to bind (default: 8080)");
    eprintln!(
        "  --load-stats-only: Load/cache data, print startup timings, then exit without starting HTTP server"
    );
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // Parse command line arguments
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage(&args[0]);
        std::process::exit(1);
    }

    let mut osm_path: Option<String> = None;
    let mut shader_type = ShaderType::Mercator;
    let mut load_stats_only = false;
    let mut port: u16 = 8080;

    let mut i = 1usize;
    while i < args.len() {
        match args[i].as_str() {
            "--simple-shader" => shader_type = ShaderType::Simple,
            "--debug-shader" => shader_type = ShaderType::Debug,
            "--styled-shader" => shader_type = ShaderType::Styled,
            "--load-stats-only" => load_stats_only = true,
            "--port" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: --port requires a value");
                    print_usage(&args[0]);
                    std::process::exit(1);
                }
                port = match args[i + 1].parse::<u16>() {
                    Ok(p) if p > 0 => p,
                    _ => {
                        eprintln!("Error: invalid port: {}", args[i + 1]);
                        print_usage(&args[0]);
                        std::process::exit(1);
                    }
                };
                i += 1; // consume port value
            }
            _ if args[i].starts_with("--") => {
                eprintln!("Error: unknown flag: {}", args[i]);
                print_usage(&args[0]);
                std::process::exit(1);
            }
            _ => {
                if osm_path.is_none() {
                    osm_path = Some(args[i].clone());
                } else {
                    eprintln!("Error: multiple OSM file paths provided");
                    print_usage(&args[0]);
                    std::process::exit(1);
                }
            }
        }
        i += 1;
    }

    let osm_path = match osm_path {
        Some(p) => p,
        None => {
            eprintln!("Error: missing OSM file path");
            print_usage(&args[0]);
            std::process::exit(1);
        }
    };

    if !Path::new(&osm_path).exists() {
        eprintln!("Error: OSM file not found: {}", osm_path);
        std::process::exit(1);
    }

    log::info!("Starting OSM tile renderer...");
    log::info!("Loading OSM data from: {}", osm_path);

    // Load OSM data and build spatial index (with disk caching)
    // We index up to zoom 15, but can render higher zoom levels by using parent tiles
    let max_z = 15;
    log::info!("Loading OSM data (max zoom: {})...", max_z);
    let startup_start = Instant::now();
    let load_start = Instant::now();
    let (tile_index, data_path) = load_osm_data_cached(&osm_path, max_z)?;
    let load_ms = load_start.elapsed().as_millis();

    log::info!(
        "OSM data loaded: {} tiles, max {} points per way",
        tile_index.len(),
        tile_index.max_points
    );

    // Memory-map the data file
    log::info!("Memory-mapping data file...");
    let mmap_start = Instant::now();
    let mmap_data = MappedData::new(&data_path)?;
    let mmap_ms = mmap_start.elapsed().as_millis();
    let low_zoom_max = 7u32;
    let lowz_start = Instant::now();
    let lowz_loaded = load_low_zoom_simplified_cached(&osm_path, max_z, low_zoom_max)?;
    let lowz_ms = lowz_start.elapsed().as_millis();
    let (low_zoom_index, low_zoom_mmap) = if let Some((idx, low_data_path)) = lowz_loaded {
        let low_mmap = MappedData::new(&low_data_path)?;
        (Some(Arc::new(idx)), Some(Arc::new(low_mmap)))
    } else {
        (None, None)
    };
    let labels_start = Instant::now();
    let labels = load_place_labels_cached(&osm_path, max_z)?;
    let labels_ms = labels_start.elapsed().as_millis();
    let total_ms = startup_start.elapsed().as_millis();
    log::info!("Data file size: {} bytes", mmap_data.len());
    log::info!(
        "Startup timing: load={}ms mmap={}ms lowz={}ms labels={}ms total={}ms",
        load_ms,
        mmap_ms,
        lowz_ms,
        labels_ms,
        total_ms
    );
    println!(
        "LOAD_STATS osm={} max_z={} tiles={} max_points={} data_bytes={} load_ms={} mmap_ms={} lowz_ms={} labels_ms={} total_ms={}",
        osm_path,
        max_z,
        tile_index.len(),
        tile_index.max_points,
        mmap_data.len(),
        load_ms,
        mmap_ms,
        lowz_ms,
        labels_ms,
        total_ms
    );

    if load_stats_only {
        log::info!("Load stats only mode: exiting before server startup");
        return Ok(());
    }

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
            way { color: #999999; width: 1; casing-color: #666666; casing-width: 0.4; z-index: 0; }
            way|z6-[highway=motorway]       { color: #cf3030; width: 5; casing-color: #7a1f1f; casing-width: 0.9; z-index: 9; }
            way|z8-[highway=trunk]          { color: #d85f2a; width: 4; casing-color: #7a3b1a; casing-width: 0.8; z-index: 8; }
            way|z8-[highway=primary]        { color: #d4a012; width: 3; casing-color: #7a6214; casing-width: 0.7; z-index: 7; }
            way|z10-[highway=secondary]     { color: #a4a41a; width: 2.5; casing-color: #5f5f16; casing-width: 0.6; z-index: 6; }
            way|z11-[highway=tertiary]      { color: #b0b0b0; width: 2; casing-color: #666666; casing-width: 0.5; z-index: 5; }
            way|z12-[highway=residential]   { color: #b0b0b0; width: 1.5; casing-color: #666666; casing-width: 0.45; z-index: 4; }
            way|z12-[highway=unclassified]  { color: #b0b0b0; width: 1.5; casing-color: #666666; casing-width: 0.45; z-index: 4; }
            way|z14-[highway=service]       { color: #c0c0c0; width: 1; casing-color: #6e6e6e; casing-width: 0.35; z-index: 3; }
            way|z13-[highway=living_street] { color: #c0c0c0; width: 1; casing-color: #6e6e6e; casing-width: 0.35; z-index: 3; }
            way|z13-[highway=motorway_link] { color: #cf3030; width: 2; casing-color: #7a1f1f; casing-width: 0.6; z-index: 8; }
            way|z13-[highway=trunk_link]    { color: #d85f2a; width: 2; casing-color: #7a3b1a; casing-width: 0.55; z-index: 7; }
            way|z13-[highway=primary_link]  { color: #d4a012; width: 2; casing-color: #7a6214; casing-width: 0.55; z-index: 6; }
        "#.to_string())
    } else {
        None
    };

    // Create app state
    let app_state = AppState {
        data: Arc::new(tile_index),
        mmap: Arc::new(mmap_data),
        low_zoom_data: low_zoom_mmap,
        low_zoom_index,
        low_zoom_max,
        labels: Arc::new(labels),
        shader_type,
        stylesheet,
    };

    // Create HTTP server
    let app = create_app(app_state);

    let bind_addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    log::info!("Server listening on http://{}", bind_addr);
    log::info!("Try: http://{}/tile/0/0/0.png", bind_addr);

    axum::serve(listener, app).await?;

    Ok(())
}
