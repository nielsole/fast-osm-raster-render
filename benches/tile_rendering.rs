use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use rust_osm_renderer::data::mmap::MappedData;
use rust_osm_renderer::data::types::Tile;
use rust_osm_renderer::data::loader::load_osm_data;
use rust_osm_renderer::renderer::{VulkanRenderer, ShaderType};
use std::path::Path;
use std::sync::Arc;
use std::fs::File;

const DATA_FILE: &str = "../go-gl-osm/prepared.osm.pbf";
const MAX_ZOOM: u32 = 15;

fn bench_tile_rendering(c: &mut Criterion) {
    // Check if data file exists
    if !Path::new(DATA_FILE).exists() {
        eprintln!("Skipping benchmark: {} not found", DATA_FILE);
        return;
    }

    // Load OSM data once for all benchmarks
    let temp_file_path = "/tmp/rust-osm-renderer-bench-data.bin";
    eprintln!("Loading OSM data from {}...", DATA_FILE);

    let mut temp_file = File::create(temp_file_path)
        .expect("Failed to create temp file");
    let tile_index = load_osm_data(DATA_FILE, MAX_ZOOM, &mut temp_file)
        .expect("Failed to load OSM data");
    let max_points = tile_index.max_points;

    eprintln!("Loaded {} tiles, max {} points per way", tile_index.len(), max_points);

    // Memory map the data
    let mmap_data = MappedData::new(temp_file_path)
        .expect("Failed to memory-map data file");
    let mmap_data = Arc::new(mmap_data);

    // Test tiles: overflow tile and slow tile
    let overflow_tile = Tile::new(1082, 661, 11);  // 430 objects, caused overflow
    let slow_tile = Tile::new(1080, 661, 11);       // 182K objects, very slow

    let mut group = c.benchmark_group("tile_rendering");

    // Ultra-fast configuration: complete in <15s total
    group.warm_up_time(std::time::Duration::from_secs(1));
    group.measurement_time(std::time::Duration::from_secs(1));
    group.sample_size(10);

    // Test with Simple shader (baseline)
    if tile_index.get(&overflow_tile).is_some() {
        let mut renderer = VulkanRenderer::new(max_points, ShaderType::Simple)
            .expect("Failed to create renderer");

        group.bench_function("11/1082/661@Simple", |b| {
            b.iter(|| {
                renderer.render_tile(black_box(&overflow_tile), &tile_index, &mmap_data)
                    .expect("Failed to render tile")
            });
        });
    }

    // Shared stylesheet with area fills and road hierarchy
    let styled_css = r#"
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

    // Test with Styled shader (MapCSS) - the overflow case
    if tile_index.get(&overflow_tile).is_some() {
        let mut renderer = VulkanRenderer::new(max_points, ShaderType::Styled)
            .expect("Failed to create renderer");

        renderer.set_stylesheet(styled_css).expect("Failed to set stylesheet");

        group.bench_function("11/1082/661@Styled", |b| {
            b.iter(|| {
                renderer.render_tile(black_box(&overflow_tile), &tile_index, &mmap_data)
                    .expect("Failed to render tile")
            });
        });
    }

    // Test with Styled shader (MapCSS) - the SLOW tile (182K objects)
    if tile_index.get(&slow_tile).is_some() {
        let mut renderer = VulkanRenderer::new(max_points, ShaderType::Styled)
            .expect("Failed to create renderer");

        renderer.set_stylesheet(styled_css).expect("Failed to set stylesheet");

        // Very limited samples for this slow tile
        group.sample_size(10);
        group.bench_function("11/1080/661@Styled (182K objects)", |b| {
            b.iter(|| {
                renderer.render_tile(black_box(&slow_tile), &tile_index, &mmap_data)
                    .expect("Failed to render tile")
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_tile_rendering);
criterion_main!(benches);
