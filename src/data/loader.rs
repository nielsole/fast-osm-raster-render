use super::serialization::write_map_object;
use super::spatial::TileIndex;
use super::types::{BoundingBox, MapObject, Point};
use crate::projection::get_tiles_for_bounding_box;
use osmpbf::{Element, ElementReader};
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Tags that indicate a closed way should be treated as an area
const AREA_TAGS: &[&str] = &[
    "building", "natural", "waterway", "landuse", "leisure", "amenity",
];

/// Check if a way should be displayed at zoom levels < 11
/// Only major roads and large features are shown at lower zoom levels
fn is_important(tags: &[(String, String)]) -> bool {
    for (key, value) in tags {
        if key == "highway" {
            match value.as_str() {
                "motorway" | "trunk" | "primary" | "secondary" | "tertiary"
                | "motorway_link" | "trunk_link" | "primary_link"
                | "secondary_link" | "tertiary_link" => return true,
                _ => {}
            }
        }
        // Large water bodies and forests are important at low zoom
        if key == "natural" {
            match value.as_str() {
                "water" | "wood" => return true,
                _ => {}
            }
        }
        if key == "waterway" && value == "riverbank" {
            return true;
        }
        if key == "landuse" {
            match value.as_str() {
                "forest" | "residential" => return true,
                _ => {}
            }
        }
    }
    false
}

/// Detect if a closed way with these tags should be flagged as an area
fn has_area_tag(tags: &[(String, String)]) -> bool {
    for (key, _) in tags {
        if AREA_TAGS.contains(&key.as_str()) {
            return true;
        }
    }
    false
}

/// Intermediate struct for parallel PBF processing.
/// Holds extracted data from a Way element without any file I/O.
struct ProcessedWay {
    bbox: BoundingBox,
    points: Vec<Point>,
    is_area: bool,
    tags: Vec<(String, String)>,
    is_important: bool,
}

/// Metadata about a written way, used for building the tile index.
struct WayMeta {
    offset: u64,
    bbox: BoundingBox,
    is_important: bool,
}

/// Load OSM data from a PBF file and build spatial index (original sequential API).
///
/// Kept for backward compatibility with tests and the render_tile example.
pub fn load_osm_data<P: AsRef<Path>>(
    osm_path: P,
    max_z: u32,
    temp_file: &mut File,
) -> io::Result<TileIndex> {
    let reader = ElementReader::from_path(osm_path)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    let mut tile_index = TileIndex::new();
    let mut way_count = 0u64;

    log::info!("Loading OSM data...");

    reader
        .for_each(|element| {
            if let Element::Way(way) = element {
                // Use node_locations() to get coordinates from osmium-processed files
                let points: Vec<Point> = way
                    .node_locations()
                    .map(|loc| Point::new(loc.lon(), loc.lat()))
                    .collect();

                if points.is_empty() {
                    return;
                }

                // Calculate bounding box
                let bounding_box = match BoundingBox::from_points(&points) {
                    Some(bbox) => bbox,
                    None => return,
                };

                // Extract all tags
                let tags: Vec<(String, String)> = way
                    .tags()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect();

                // Detect closed way: first == last and at least 4 points (triangle + closing)
                let is_closed = points.len() >= 4
                    && (points.first().unwrap().lon - points.last().unwrap().lon).abs() < 1e-10
                    && (points.first().unwrap().lat - points.last().unwrap().lat).abs() < 1e-10;

                let is_area = is_closed && has_area_tag(&tags);

                // Create map object with all tags
                let map_object = MapObject::new(bounding_box, points, is_area, tags.clone());

                // Update max points
                tile_index.update_max_points(map_object.points.len());

                // Write to temp file
                let offset = match write_map_object(temp_file, &map_object) {
                    Ok(offset) => offset,
                    Err(e) => {
                        log::error!("Failed to write map object: {}", e);
                        return;
                    }
                };

                // Check if this is an important way for zoom < 11 filtering
                let important = is_important(&tags);

                // Get all tiles that overlap with this way's bounding box
                let tiles = get_tiles_for_bounding_box(&bounding_box, 0, max_z);

                for tile in tiles {
                    // Skip non-important ways at zoom < 11
                    if !important && tile.z < 11 {
                        continue;
                    }

                    tile_index.insert(tile, offset);
                }

                way_count += 1;
                if way_count % 100_000 == 0 {
                    log::info!("Processed {} ways...", way_count);
                }
            }
        })
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    log::info!(
        "Loaded {} ways, max points: {}, tiles: {}",
        way_count,
        tile_index.max_points,
        tile_index.len()
    );

    Ok(tile_index)
}

/// Get PBF file metadata for cache validation.
fn pbf_metadata(path: &Path) -> io::Result<(u64, i64)> {
    let meta = std::fs::metadata(path)?;
    let size = meta.len();
    let mtime = meta
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    Ok((size, mtime))
}

/// Load OSM data with disk caching.
///
/// On first run: parses PBF in parallel, writes data file, builds tile index,
/// caches everything to disk.
/// On subsequent runs: loads cache in seconds if PBF file hasn't changed.
///
/// Cache files are stored alongside the PBF: `<path>.cache.data` and `<path>.cache.index`.
pub fn load_osm_data_cached<P: AsRef<Path>>(
    osm_path: P,
    max_z: u32,
) -> io::Result<(TileIndex, PathBuf)> {
    let osm_path = osm_path.as_ref();
    let data_cache_path = PathBuf::from(format!("{}.cache.data", osm_path.display()));
    let index_cache_path = PathBuf::from(format!("{}.cache.index", osm_path.display()));

    let (pbf_size, pbf_mtime) = pbf_metadata(osm_path)?;

    // Try loading from cache
    if data_cache_path.exists() && index_cache_path.exists() {
        let cache_start = Instant::now();
        if let Ok(mut index_file) = File::open(&index_cache_path) {
            if let Ok(Some(tile_index)) =
                TileIndex::read_from(&mut index_file, pbf_size, pbf_mtime)
            {
                log::info!(
                    "Loaded from cache in {}ms ({} tiles, max {} points)",
                    cache_start.elapsed().as_millis(),
                    tile_index.len(),
                    tile_index.max_points,
                );
                return Ok((tile_index, data_cache_path));
            }
        }
        log::info!("Cache invalid (PBF changed), rebuilding...");
    }

    // Cache miss — build from scratch
    let total_start = Instant::now();

    // Phase 1: Parallel PBF parsing
    log::info!("Phase 1: Parsing PBF in parallel...");
    let parse_start = Instant::now();
    let reader = ElementReader::from_path(osm_path)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    let ways: Vec<ProcessedWay> = reader
        .par_map_reduce(
            |element| {
                let mut ways = Vec::new();
                if let Element::Way(way) = element {
                    let points: Vec<Point> = way
                        .node_locations()
                        .map(|loc| Point::new(loc.lon(), loc.lat()))
                        .collect();

                    if points.is_empty() {
                        return ways;
                    }

                    let bbox = match BoundingBox::from_points(&points) {
                        Some(bbox) => bbox,
                        None => return ways,
                    };

                    let tags: Vec<(String, String)> = way
                        .tags()
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect();

                    let is_closed = points.len() >= 4
                        && (points.first().unwrap().lon - points.last().unwrap().lon).abs() < 1e-10
                        && (points.first().unwrap().lat - points.last().unwrap().lat).abs() < 1e-10;

                    let is_area = is_closed && has_area_tag(&tags);
                    let important = is_important(&tags);

                    ways.push(ProcessedWay {
                        bbox,
                        points,
                        is_area,
                        tags,
                        is_important: important,
                    });
                }
                ways
            },
            Vec::new,
            |mut a, b| {
                a.extend(b);
                a
            },
        )
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    log::info!(
        "Phase 1 complete: {} ways parsed in {}ms",
        ways.len(),
        parse_start.elapsed().as_millis()
    );

    // Phase 2: Serial binary data write
    log::info!("Phase 2: Writing binary data...");
    let write_start = Instant::now();
    let data_file = File::create(&data_cache_path)?;
    let mut writer = BufWriter::new(data_file);
    let mut max_points: usize = 0;

    let mut way_metas = Vec::with_capacity(ways.len());
    for way in &ways {
        let obj = MapObject::new(way.bbox, way.points.clone(), way.is_area, way.tags.clone());
        if obj.points.len() > max_points {
            max_points = obj.points.len();
        }
        let offset = write_map_object(&mut writer, &obj)?;
        way_metas.push(WayMeta {
            offset,
            bbox: way.bbox,
            is_important: way.is_important,
        });
    }
    writer.flush()?;
    drop(writer);
    drop(ways); // Free memory before building index

    log::info!(
        "Phase 2 complete: {} ways written in {}ms",
        way_metas.len(),
        write_start.elapsed().as_millis()
    );

    // Phase 3: Parallel tile index building
    log::info!("Phase 3: Building tile index in parallel...");
    let index_start = Instant::now();

    let chunk_size = 10_000.max(way_metas.len() / rayon::current_num_threads() / 4);
    let partial_indexes: Vec<HashMap<u64, Vec<u64>>> = way_metas
        .par_chunks(chunk_size)
        .map(|chunk| {
            let mut local: HashMap<u64, Vec<u64>> = HashMap::new();
            for meta in chunk {
                let tiles = get_tiles_for_bounding_box(&meta.bbox, 0, max_z);
                for tile in tiles {
                    if !meta.is_important && tile.z < 11 {
                        continue;
                    }
                    local
                        .entry(tile.index())
                        .or_insert_with(Vec::new)
                        .push(meta.offset);
                }
            }
            local
        })
        .collect();

    // Merge partial indexes
    let mut tile_index = TileIndex::new();
    tile_index.update_max_points(max_points);
    for partial in partial_indexes {
        for (key, offsets) in partial {
            tile_index
                .tiles
                .entry(key)
                .or_insert_with(Vec::new)
                .extend(offsets);
        }
    }

    log::info!(
        "Phase 3 complete: {} tile entries in {}ms",
        tile_index.len(),
        index_start.elapsed().as_millis()
    );

    // Phase 4: Cache tile index to disk
    log::info!("Phase 4: Caching tile index...");
    let cache_start = Instant::now();
    let mut index_file = BufWriter::new(File::create(&index_cache_path)?);
    tile_index.write_to(&mut index_file, pbf_size, pbf_mtime)?;
    index_file.flush()?;

    log::info!(
        "Phase 4 complete: index cached in {}ms",
        cache_start.elapsed().as_millis()
    );

    log::info!(
        "Total loading time: {}ms ({} ways, {} tiles, max {} points)",
        total_start.elapsed().as_millis(),
        way_metas.len(),
        tile_index.len(),
        tile_index.max_points,
    );

    Ok((tile_index, data_cache_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_important() {
        let motorway = vec![("highway".to_string(), "motorway".to_string())];
        assert!(is_important(&motorway));

        let residential = vec![("highway".to_string(), "residential".to_string())];
        assert!(!is_important(&residential));

        let primary = vec![("highway".to_string(), "primary".to_string())];
        assert!(is_important(&primary));

        let footway = vec![("highway".to_string(), "footway".to_string())];
        assert!(!is_important(&footway));

        let water = vec![("natural".to_string(), "water".to_string())];
        assert!(is_important(&water));

        let forest = vec![("landuse".to_string(), "forest".to_string())];
        assert!(is_important(&forest));
    }

    #[test]
    fn test_has_area_tag() {
        let building = vec![("building".to_string(), "yes".to_string())];
        assert!(has_area_tag(&building));

        let water = vec![("natural".to_string(), "water".to_string())];
        assert!(has_area_tag(&water));

        let highway = vec![("highway".to_string(), "primary".to_string())];
        assert!(!has_area_tag(&highway));

        let park = vec![("leisure".to_string(), "park".to_string())];
        assert!(has_area_tag(&park));
    }
}
