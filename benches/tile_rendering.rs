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

    // Test with Styled shader (MapCSS) - the overflow case
    if tile_index.get(&overflow_tile).is_some() {
        let mut renderer = VulkanRenderer::new(max_points, ShaderType::Styled)
            .expect("Failed to create renderer");

        // Configure MapCSS
        renderer.set_data_file_path(temp_file_path.to_string());
        renderer.set_stylesheet("way[highway=primary] { color: #ff0000; }")
            .expect("Failed to set stylesheet");

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

        // Configure MapCSS
        renderer.set_data_file_path(temp_file_path.to_string());
        renderer.set_stylesheet("way[highway=primary] { color: #ff0000; }")
            .expect("Failed to set stylesheet");

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
