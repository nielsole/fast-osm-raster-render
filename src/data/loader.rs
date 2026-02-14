use super::serialization::write_map_object;
use super::spatial::TileIndex;
use super::types::{BoundingBox, MapObject, Point};
use crate::projection::get_tiles_for_bounding_box;
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use memmap2::Mmap;
use osmpbf::{BlobDecode, BlobReader, Element, ElementReader};
use rayon::prelude::*;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::collections::BinaryHeap;
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::hash::{Hash, Hasher};
use std::cmp::Ordering;
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

/// Tags that indicate a closed way should be treated as an area
const AREA_TAGS: &[&str] = &[
    "building", "natural", "waterway", "landuse", "leisure", "amenity",
];

/// Compute the minimum zoom at which this way should be included in the tile index.
/// Lower values mean "show earlier" (more important at low zoom).
///
/// Defaults to 11, which effectively drops local/detail-heavy features for z<11.
fn low_zoom_min(tags: &[(String, String)]) -> u8 {
    let mut min_z = 11u8;

    for (key, value) in tags {
        if key == "highway" {
            let z = match value.as_str() {
                "motorway" | "motorway_link" => Some(5),
                "trunk" | "trunk_link" => Some(6),
                "primary" | "primary_link" => Some(7),
                "secondary" | "secondary_link" => Some(8),
                "tertiary" | "tertiary_link" => Some(9),
                _ => None,
            };
            if let Some(z) = z {
                min_z = min_z.min(z);
            }
        } else if key == "natural" {
            let z = match value.as_str() {
                "water" => Some(6),
                "wood" => Some(8),
                _ => None,
            };
            if let Some(z) = z {
                min_z = min_z.min(z);
            }
        } else if key == "waterway" {
            let z = match value.as_str() {
                "riverbank" => Some(6),
                "river" | "canal" => Some(7),
                "stream" => Some(9),
                _ => None,
            };
            if let Some(z) = z {
                min_z = min_z.min(z);
            }
        } else if key == "railway" {
            let z = match value.as_str() {
                "rail" => Some(8),
                "light_rail" | "subway" | "tram" => Some(9),
                _ => None,
            };
            if let Some(z) = z {
                min_z = min_z.min(z);
            }
        } else if key == "boundary" && value == "administrative" {
            min_z = min_z.min(7);
        } else if key == "landuse" {
            let z = match value.as_str() {
                "forest" => Some(8),
                "residential" | "commercial" | "industrial" => Some(10),
                _ => None,
            };
            if let Some(z) = z {
                min_z = min_z.min(z);
            }
        }
    }

    min_z
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

/// Metadata about a written way, used for building the tile index.
struct WayMeta {
    offset: u64,
    bbox: BoundingBox,
    low_zoom_min: u8,
}

#[derive(Clone, Copy, Debug)]
struct NodeCoord {
    id: i64,
    lon_dm7: i32,
    lat_dm7: i32,
}

const META_CACHE_MAGIC: &[u8; 8] = b"OSMMETA2";
const NODE_RECORD_SIZE: usize = 16; // i64 node_id + i32 lon_dm7 + i32 lat_dm7
const INDEX_CACHE_VERSION: u32 = 2;
const SIMPLIFIED_CACHE_VERSION: u32 = 3;
const SIMPLIFIED_MIN_POINTS: usize = 24;
const SIMPLIFIED_MIN_AREA_POINTS: usize = 12;
const DEFAULT_CACHE_DIR: &str = ".osm-cache";
const LEGACY_CACHE_DIR: &str = "/tmp/rust-osm-renderer-cache";

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

                let min_zoom = low_zoom_min(&tags);

                // Get all tiles that overlap with this way's bounding box
                let tiles = get_tiles_for_bounding_box(&bounding_box, 0, max_z);

                for tile in tiles {
                    // Skip features that are below their low-zoom visibility tier.
                    if tile.z < min_zoom as u32 {
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

fn cache_prefix(osm_path: &Path) -> String {
    let canonical = std::fs::canonicalize(osm_path).unwrap_or_else(|_| osm_path.to_path_buf());
    let mut hasher = DefaultHasher::new();
    canonical.hash(&mut hasher);
    let hash = hasher.finish();

    let stem = osm_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("osm");
    format!("{}-{:016x}", stem, hash)
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else if from.is_file() {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

fn move_with_copy_fallback(src: &Path, dst: &Path) -> io::Result<()> {
    match fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(e) if e.raw_os_error() == Some(18) => {
            if src.is_dir() {
                copy_dir_recursive(src, dst)?;
                fs::remove_dir_all(src)?;
            } else if src.is_file() {
                fs::copy(src, dst)?;
                fs::remove_file(src)?;
            }
            Ok(())
        }
        Err(e) => Err(e),
    }
}

fn maybe_migrate_legacy_cache(cache_root: &Path) -> io::Result<()> {
    if cache_root != Path::new(DEFAULT_CACHE_DIR) {
        return Ok(());
    }

    let legacy_root = Path::new(LEGACY_CACHE_DIR);
    if !legacy_root.exists() || !legacy_root.is_dir() {
        return Ok(());
    }

    fs::create_dir_all(cache_root)?;
    for entry in fs::read_dir(legacy_root)? {
        let entry = entry?;
        let src = entry.path();
        let dst = cache_root.join(entry.file_name());
        if dst.exists() {
            continue;
        }
        move_with_copy_fallback(&src, &dst)?;
    }

    if fs::read_dir(legacy_root)?.next().is_none() {
        let _ = fs::remove_dir(legacy_root);
    }
    Ok(())
}

fn cache_root_dir() -> io::Result<PathBuf> {
    let cache_root = std::env::var("RUST_OSM_CACHE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_CACHE_DIR));
    fs::create_dir_all(&cache_root)?;
    maybe_migrate_legacy_cache(&cache_root)?;
    Ok(cache_root)
}

fn cache_paths(osm_path: &Path, max_z: u32) -> io::Result<(PathBuf, PathBuf, PathBuf, PathBuf)> {
    let cache_root = cache_root_dir()?;

    let prefix = cache_prefix(osm_path);

    let data_cache_path = cache_root.join(format!("{}.cache.data", prefix));
    let meta_cache_path = cache_root.join(format!("{}.cache.meta", prefix));
    let index_cache_path = cache_root.join(format!(
        "{}.cache.v{}.z{}.index",
        prefix, INDEX_CACHE_VERSION, max_z
    ));
    let toc_cache_path = cache_root.join(format!(
        "{}.cache.v{}.z{}.toc",
        prefix, INDEX_CACHE_VERSION, max_z
    ));
    Ok((data_cache_path, meta_cache_path, index_cache_path, toc_cache_path))
}

fn low_zoom_cache_paths(
    osm_path: &Path,
    low_zoom_max: u32,
) -> io::Result<(PathBuf, PathBuf, PathBuf)> {
    let cache_root = cache_root_dir()?;
    let prefix = cache_prefix(osm_path);
    let suffix = format!(
        "{}.cache.lowz{}.v{}",
        prefix, low_zoom_max, SIMPLIFIED_CACHE_VERSION
    );
    let data_cache_path = cache_root.join(format!("{}.data", suffix));
    let index_cache_path = cache_root.join(format!("{}.index", suffix));
    let toc_cache_path = cache_root.join(format!("{}.toc", suffix));
    Ok((data_cache_path, index_cache_path, toc_cache_path))
}

#[inline(always)]
fn mercator_unit_x(lon: f64) -> f64 {
    (lon + 180.0) / 360.0
}

#[inline(always)]
fn mercator_unit_y(lat: f64) -> f64 {
    let clamped = lat.clamp(-85.05112878, 85.05112878);
    let rad = clamped.to_radians();
    (1.0 - ((rad.tan() + 1.0 / rad.cos()).ln() / std::f64::consts::PI)) * 0.5
}

#[inline(always)]
fn sqr(v: f64) -> f64 {
    v * v
}

fn perpendicular_distance_sq_px(p: Point, a: Point, b: Point, zoom: u32) -> f64 {
    let scale = 256.0 * 2f64.powi(zoom as i32);
    let px = mercator_unit_x(p.lon) * scale;
    let py = mercator_unit_y(p.lat) * scale;
    let ax = mercator_unit_x(a.lon) * scale;
    let ay = mercator_unit_y(a.lat) * scale;
    let bx = mercator_unit_x(b.lon) * scale;
    let by = mercator_unit_y(b.lat) * scale;

    let abx = bx - ax;
    let aby = by - ay;
    let len_sq = abx * abx + aby * aby;
    if len_sq <= 1e-12 {
        return sqr(px - ax) + sqr(py - ay);
    }

    let t = (((px - ax) * abx + (py - ay) * aby) / len_sq).clamp(0.0, 1.0);
    let proj_x = ax + t * abx;
    let proj_y = ay + t * aby;
    sqr(px - proj_x) + sqr(py - proj_y)
}

fn mark_sharp_turns(points: &[Point], keep: &mut [bool], min_turn_deg: f64) {
    if points.len() < 3 {
        return;
    }
    let cos_limit = (180.0 - min_turn_deg).to_radians().cos();
    for i in 1..(points.len() - 1) {
        let a = points[i - 1];
        let b = points[i];
        let c = points[i + 1];
        let v1x = b.lon - a.lon;
        let v1y = b.lat - a.lat;
        let v2x = c.lon - b.lon;
        let v2y = c.lat - b.lat;
        let n1 = (v1x * v1x + v1y * v1y).sqrt();
        let n2 = (v2x * v2x + v2y * v2y).sqrt();
        if n1 <= 1e-12 || n2 <= 1e-12 {
            continue;
        }
        let cos_angle = ((v1x * v2x + v1y * v2y) / (n1 * n2)).clamp(-1.0, 1.0);
        if cos_angle > cos_limit {
            continue;
        }
        keep[i] = true;
    }
}

fn simplify_segment_rdp(
    points: &[Point],
    keep: &mut [bool],
    start: usize,
    end: usize,
    tol_sq: f64,
    zoom: u32,
) {
    if end <= start + 1 {
        return;
    }
    let mut stack = vec![(start, end)];
    while let Some((s, e)) = stack.pop() {
        if e <= s + 1 {
            continue;
        }
        let a = points[s];
        let b = points[e];
        let mut max_dist = 0.0;
        let mut idx = None;
        for i in (s + 1)..e {
            let d = perpendicular_distance_sq_px(points[i], a, b, zoom);
            if d > max_dist {
                max_dist = d;
                idx = Some(i);
            }
        }
        if let Some(i) = idx {
            if max_dist > tol_sq {
                keep[i] = true;
                stack.push((s, i));
                stack.push((i, e));
            }
        }
    }
}

fn simplify_line_points(points: &[Point], zoom: u32) -> Vec<Point> {
    if points.len() <= 2 {
        return points.to_vec();
    }

    let tolerance_px = 1.0;
    let tol_sq = tolerance_px * tolerance_px;
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    mark_sharp_turns(points, &mut keep, 20.0);

    let mut anchors: Vec<usize> = keep
        .iter()
        .enumerate()
        .filter_map(|(i, &k)| if k { Some(i) } else { None })
        .collect();
    anchors.sort_unstable();

    for w in anchors.windows(2) {
        simplify_segment_rdp(points, &mut keep, w[0], w[1], tol_sq, zoom);
    }

    let simplified: Vec<Point> = points
        .iter()
        .enumerate()
        .filter_map(|(i, p)| if keep[i] { Some(*p) } else { None })
        .collect();
    if simplified.len() < 2 {
        points.to_vec()
    } else {
        simplified
    }
}

#[inline(always)]
fn points_equal(a: Point, b: Point) -> bool {
    (a.lon - b.lon).abs() < 1e-10 && (a.lat - b.lat).abs() < 1e-10
}

fn simplify_area_points(points: &[Point], zoom: u32) -> Vec<Point> {
    if points.len() < 4 {
        return points.to_vec();
    }

    let is_closed = points_equal(points[0], points[points.len() - 1]);
    if !is_closed {
        return simplify_line_points(points, zoom);
    }

    let ring = &points[..points.len() - 1];
    if ring.len() < 3 {
        return points.to_vec();
    }

    // Reuse the line simplifier on the open ring, then re-close the polygon.
    let mut simplified = simplify_line_points(ring, zoom);
    if simplified.len() < 3 {
        return points.to_vec();
    }

    if !points_equal(simplified[0], *simplified.last().unwrap()) {
        simplified.push(simplified[0]);
    }

    if simplified.len() < 4 {
        return points.to_vec();
    }

    simplified
}

fn write_way_meta_cache<W: Write>(
    writer: &mut W,
    way_metas: &[WayMeta],
    max_points: usize,
    pbf_size: u64,
    pbf_mtime: i64,
) -> io::Result<()> {
    writer.write_all(META_CACHE_MAGIC)?;
    writer.write_u64::<LittleEndian>(pbf_size)?;
    writer.write_i64::<LittleEndian>(pbf_mtime)?;
    writer.write_u64::<LittleEndian>(max_points as u64)?;
    writer.write_u64::<LittleEndian>(way_metas.len() as u64)?;

    for meta in way_metas {
        writer.write_u64::<LittleEndian>(meta.offset)?;
        writer.write_f64::<LittleEndian>(meta.bbox.min.lon)?;
        writer.write_f64::<LittleEndian>(meta.bbox.min.lat)?;
        writer.write_f64::<LittleEndian>(meta.bbox.max.lon)?;
        writer.write_f64::<LittleEndian>(meta.bbox.max.lat)?;
        writer.write_u8(meta.low_zoom_min)?;
    }

    Ok(())
}

fn read_way_meta_cache<R: Read>(
    reader: &mut R,
    expected_pbf_size: u64,
    expected_pbf_mtime: i64,
) -> io::Result<Option<(Vec<WayMeta>, usize)>> {
    let mut magic = [0u8; 8];
    if reader.read_exact(&mut magic).is_err() {
        return Ok(None);
    }
    if &magic != META_CACHE_MAGIC {
        return Ok(None);
    }

    let pbf_size = reader.read_u64::<LittleEndian>()?;
    let pbf_mtime = reader.read_i64::<LittleEndian>()?;
    if pbf_size != expected_pbf_size || pbf_mtime != expected_pbf_mtime {
        return Ok(None);
    }

    let max_points = reader.read_u64::<LittleEndian>()? as usize;
    let count = reader.read_u64::<LittleEndian>()? as usize;
    let mut metas = Vec::with_capacity(count);

    for _ in 0..count {
        let offset = reader.read_u64::<LittleEndian>()?;
        let min_lon = reader.read_f64::<LittleEndian>()?;
        let min_lat = reader.read_f64::<LittleEndian>()?;
        let max_lon = reader.read_f64::<LittleEndian>()?;
        let max_lat = reader.read_f64::<LittleEndian>()?;
        let low_zoom_min = reader.read_u8()?;
        metas.push(WayMeta {
            offset,
            bbox: BoundingBox {
                min: Point::new(min_lon, min_lat),
                max: Point::new(max_lon, max_lat),
            },
            low_zoom_min,
        });
    }

    Ok(Some((metas, max_points)))
}

fn build_tile_index_from_way_metas(
    way_metas: &[WayMeta],
    max_points: usize,
    max_z: u32,
) -> TileIndex {
    let chunk_size = 10_000.max(way_metas.len() / rayon::current_num_threads() / 4);
    let partial_indexes: Vec<HashMap<u64, Vec<u64>>> = way_metas
        .par_chunks(chunk_size)
        .map(|chunk| {
            let mut local: HashMap<u64, Vec<u64>> = HashMap::new();
            for meta in chunk {
                let tiles = get_tiles_for_bounding_box(&meta.bbox, 0, max_z);
                for tile in tiles {
                    if tile.z < meta.low_zoom_min as u32 {
                        continue;
                    }
                    local.entry(tile.index()).or_default().push(meta.offset);
                }
            }
            local
        })
        .collect();

    let mut tile_index = TileIndex::new();
    tile_index.update_max_points(max_points);
    for partial in partial_indexes {
        for (key, offsets) in partial {
            tile_index.insert_tile_key_offsets(key, offsets);
        }
    }
    tile_index
}

fn has_locations_on_ways<P: AsRef<Path>>(osm_path: P) -> io::Result<bool> {
    let file = File::open(osm_path)?;
    let blob_reader = BlobReader::new(BufReader::new(file));

    for blob in blob_reader {
        let blob = blob.map_err(io::Error::other)?;
        match blob.decode().map_err(io::Error::other)? {
            BlobDecode::OsmHeader(header) => {
                return Ok(header
                    .optional_features()
                    .iter()
                    .any(|f| f == "LocationsOnWays"));
            }
            BlobDecode::OsmData(_) | BlobDecode::Unknown(_) => {}
        }
    }

    Ok(false)
}

fn write_node_coord<W: Write>(writer: &mut W, node: &NodeCoord) -> io::Result<()> {
    writer.write_i64::<LittleEndian>(node.id)?;
    writer.write_i32::<LittleEndian>(node.lon_dm7)?;
    writer.write_i32::<LittleEndian>(node.lat_dm7)?;
    Ok(())
}

fn read_node_coord<R: Read>(reader: &mut R) -> io::Result<Option<NodeCoord>> {
    let id = match reader.read_i64::<LittleEndian>() {
        Ok(v) => v,
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    };
    let lon_dm7 = reader.read_i32::<LittleEndian>()?;
    let lat_dm7 = reader.read_i32::<LittleEndian>()?;
    Ok(Some(NodeCoord {
        id,
        lon_dm7,
        lat_dm7,
    }))
}

fn spill_sorted_node_chunk(
    temp_dir: &Path,
    chunk_id: usize,
    nodes: &mut Vec<NodeCoord>,
) -> io::Result<PathBuf> {
    nodes.sort_unstable_by_key(|n| n.id);
    let path = temp_dir.join(format!("nodes_chunk_{:06}.bin", chunk_id));
    let file = File::create(&path)?;
    let mut writer = BufWriter::new(file);
    for node in nodes.iter() {
        write_node_coord(&mut writer, node)?;
    }
    writer.flush()?;
    nodes.clear();
    Ok(path)
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
struct HeapItem {
    id: i64,
    chunk_idx: usize,
    lon_dm7: i32,
    lat_dm7: i32,
}

impl Ord for HeapItem {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse for min-heap behavior over BinaryHeap
        other
            .id
            .cmp(&self.id)
            .then_with(|| other.chunk_idx.cmp(&self.chunk_idx))
    }
}

impl PartialOrd for HeapItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn merge_sorted_node_chunks(chunk_paths: &[PathBuf], out_path: &Path) -> io::Result<()> {
    let mut readers: Vec<BufReader<File>> = chunk_paths
        .iter()
        .map(File::open)
        .collect::<io::Result<Vec<_>>>()?
        .into_iter()
        .map(BufReader::new)
        .collect();

    let out_file = File::create(out_path)?;
    let mut out = BufWriter::new(out_file);
    let mut heap = BinaryHeap::new();

    for (idx, reader) in readers.iter_mut().enumerate() {
        if let Some(node) = read_node_coord(reader)? {
            heap.push(HeapItem {
                id: node.id,
                chunk_idx: idx,
                lon_dm7: node.lon_dm7,
                lat_dm7: node.lat_dm7,
            });
        }
    }

    while let Some(item) = heap.pop() {
        write_node_coord(
            &mut out,
            &NodeCoord {
                id: item.id,
                lon_dm7: item.lon_dm7,
                lat_dm7: item.lat_dm7,
            },
        )?;

        if let Some(next) = read_node_coord(&mut readers[item.chunk_idx])? {
            heap.push(HeapItem {
                id: next.id,
                chunk_idx: item.chunk_idx,
                lon_dm7: next.lon_dm7,
                lat_dm7: next.lat_dm7,
            });
        }
    }

    out.flush()?;
    Ok(())
}

struct SortedNodeLookup {
    mmap: Mmap,
    count: usize,
}

impl SortedNodeLookup {
    fn open(path: &Path) -> io::Result<Self> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };
        if mmap.len() % NODE_RECORD_SIZE != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Sorted node index has invalid size",
            ));
        }
        let count = mmap.len() / NODE_RECORD_SIZE;
        Ok(Self { mmap, count })
    }

    #[inline(always)]
    fn record_at(&self, index: usize) -> NodeCoord {
        let base = index * NODE_RECORD_SIZE;
        unsafe {
            let ptr = self.mmap.as_ptr().add(base);
            let id = ptr.cast::<i64>().read_unaligned();
            let lon_dm7 = ptr.add(8).cast::<i32>().read_unaligned();
            let lat_dm7 = ptr.add(12).cast::<i32>().read_unaligned();
            NodeCoord {
                id,
                lon_dm7,
                lat_dm7,
            }
        }
    }

    fn get(&self, target_id: i64) -> Option<NodeCoord> {
        let mut lo = 0usize;
        let mut hi = self.count;
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let node = self.record_at(mid);
            if node.id < target_id {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo < self.count {
            let node = self.record_at(lo);
            if node.id == target_id {
                return Some(node);
            }
        }
        None
    }
}

fn build_cache_from_locations_on_ways(
    osm_path: &Path,
    data_cache_path: &Path,
) -> io::Result<(Vec<WayMeta>, usize)> {
    let phase_start = Instant::now();
    let reader = ElementReader::from_path(osm_path).map_err(io::Error::other)?;
    let data_file = File::create(data_cache_path)?;
    let mut writer = BufWriter::new(data_file);

    let mut way_count: u64 = 0;
    let mut max_points = 0usize;
    let mut way_metas = Vec::new();

    reader
        .for_each(|element| {
            if let Element::Way(way) = element {
                let points: Vec<Point> = way
                    .node_locations()
                    .map(|loc| Point::new(loc.lon(), loc.lat()))
                    .collect();
                if points.is_empty() {
                    return;
                }

                let bbox = match BoundingBox::from_points(&points) {
                    Some(b) => b,
                    None => return,
                };

                let tags: Vec<(String, String)> = way
                    .tags()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect();

                let first = &points[0];
                let last = &points[points.len() - 1];
                let is_closed = points.len() >= 4
                    && (first.lon - last.lon).abs() < 1e-10
                    && (first.lat - last.lat).abs() < 1e-10;
                let is_area = is_closed && has_area_tag(&tags);
                let min_zoom = low_zoom_min(&tags);

                let points_len = points.len();
                let map_object = MapObject::new(bbox, points, is_area, tags);
                let offset = match write_map_object(&mut writer, &map_object) {
                    Ok(o) => o,
                    Err(e) => {
                        log::error!("Failed to write map object: {}", e);
                        return;
                    }
                };

                if points_len > max_points {
                    max_points = points_len;
                }
                way_metas.push(WayMeta {
                    offset,
                    bbox,
                    low_zoom_min: min_zoom,
                });

                way_count += 1;
                if way_count.is_multiple_of(100_000) {
                    log::info!(
                        "Phase 1 progress: {} ways parsed/written ({}ms elapsed)",
                        way_count,
                        phase_start.elapsed().as_millis()
                    );
                }
            }
        })
        .map_err(io::Error::other)?;

    writer.flush()?;
    log::info!(
        "Phase 1 complete: {} ways parsed/written in {}ms",
        way_count,
        phase_start.elapsed().as_millis()
    );
    Ok((way_metas, max_points))
}

fn build_cache_from_raw_pbf(
    osm_path: &Path,
    data_cache_path: &Path,
) -> io::Result<(Vec<WayMeta>, usize)> {
    log::info!("LocationsOnWays missing. Using raw PBF mode (node refs resolution).");

    // Pass A: build sorted node index on disk using external merge sort
    let nodes_start = Instant::now();
    let reader_nodes = ElementReader::from_path(osm_path).map_err(io::Error::other)?;
    let temp_dir = std::env::temp_dir().join(format!(
        "rust-osm-renderer-nodesort-{}-{}",
        std::process::id(),
        nodes_start.elapsed().as_nanos()
    ));
    std::fs::create_dir_all(&temp_dir)?;

    let chunk_size = std::env::var("RUST_OSM_NODE_SORT_CHUNK")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(1_000_000);

    let mut chunk_paths: Vec<PathBuf> = Vec::new();
    let mut chunk_buf: Vec<NodeCoord> = Vec::with_capacity(chunk_size);
    let mut next_chunk_id: usize = 0;
    let mut node_count: u64 = 0;

    let worker_count = std::env::var("RUST_OSM_NODE_SORT_WORKERS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or_else(|| {
            thread::available_parallelism()
                .map(|n| n.get().saturating_sub(1).max(1))
                .unwrap_or(1)
        });
    let queue_depth = std::env::var("RUST_OSM_NODE_SORT_QUEUE")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(2);

    log::info!(
        "Raw pass A: chunk_size={}, workers={}, queue_depth={}",
        chunk_size,
        worker_count,
        queue_depth
    );

    let (result_tx, result_rx) = mpsc::channel::<io::Result<PathBuf>>();
    let mut work_senders = Vec::with_capacity(worker_count);
    let mut workers = Vec::with_capacity(worker_count);

    for worker_idx in 0..worker_count {
        let (tx, rx) = mpsc::sync_channel::<Option<(usize, Vec<NodeCoord>)>>(queue_depth);
        work_senders.push(tx);

        let worker_temp_dir = temp_dir.clone();
        let worker_result_tx = result_tx.clone();
        workers.push(thread::spawn(move || {
            while let Ok(work) = rx.recv() {
                match work {
                    Some((chunk_id, mut nodes)) => {
                        let res = spill_sorted_node_chunk(&worker_temp_dir, chunk_id, &mut nodes);
                        let _ = worker_result_tx.send(res);
                    }
                    None => break,
                }
            }
            log::debug!("Node sort worker {} exited", worker_idx);
        }));
    }
    drop(result_tx);

    let mut parse_error: Option<io::Error> = None;
    let mut chunks_submitted: usize = 0;

    reader_nodes
        .for_each(|element| match element {
            Element::Node(node) => {
                if parse_error.is_some() {
                    return;
                }
                chunk_buf.push(NodeCoord {
                    id: node.id(),
                    lon_dm7: node.decimicro_lon(),
                    lat_dm7: node.decimicro_lat(),
                });
                node_count += 1;
                if chunk_buf.len() >= chunk_size {
                    let worker_ix = next_chunk_id % worker_count;
                    let nodes = std::mem::replace(&mut chunk_buf, Vec::with_capacity(chunk_size));
                    if let Err(e) = work_senders[worker_ix].send(Some((next_chunk_id, nodes))) {
                        parse_error = Some(io::Error::new(
                            io::ErrorKind::BrokenPipe,
                            format!("Failed to dispatch chunk {}: {}", next_chunk_id, e),
                        ));
                        return;
                    }
                    next_chunk_id += 1;
                    chunks_submitted += 1;
                }
                if node_count.is_multiple_of(5_000_000) {
                    log::info!(
                        "Raw pass A progress: {} nodes indexed, {} chunks queued ({}ms elapsed)",
                        node_count,
                        chunks_submitted,
                        nodes_start.elapsed().as_millis()
                    );
                }
            }
            Element::DenseNode(node) => {
                if parse_error.is_some() {
                    return;
                }
                chunk_buf.push(NodeCoord {
                    id: node.id(),
                    lon_dm7: node.decimicro_lon(),
                    lat_dm7: node.decimicro_lat(),
                });
                node_count += 1;
                if chunk_buf.len() >= chunk_size {
                    let worker_ix = next_chunk_id % worker_count;
                    let nodes = std::mem::replace(&mut chunk_buf, Vec::with_capacity(chunk_size));
                    if let Err(e) = work_senders[worker_ix].send(Some((next_chunk_id, nodes))) {
                        parse_error = Some(io::Error::new(
                            io::ErrorKind::BrokenPipe,
                            format!("Failed to dispatch chunk {}: {}", next_chunk_id, e),
                        ));
                        return;
                    }
                    next_chunk_id += 1;
                    chunks_submitted += 1;
                }
                if node_count.is_multiple_of(5_000_000) {
                    log::info!(
                        "Raw pass A progress: {} nodes indexed, {} chunks queued ({}ms elapsed)",
                        node_count,
                        chunks_submitted,
                        nodes_start.elapsed().as_millis()
                    );
                }
            }
            _ => {}
        })
        .map_err(io::Error::other)?;

    if let Some(err) = parse_error {
        return Err(err);
    }

    if !chunk_buf.is_empty() {
        let worker_ix = next_chunk_id % worker_count;
        let nodes = std::mem::replace(&mut chunk_buf, Vec::with_capacity(chunk_size));
        work_senders[worker_ix]
            .send(Some((next_chunk_id, nodes)))
            .map_err(|e| io::Error::new(io::ErrorKind::BrokenPipe, e.to_string()))?;
        chunks_submitted += 1;
    }

    // Signal shutdown to workers
    for tx in work_senders {
        let _ = tx.send(None);
    }

    // Collect worker results
    for _ in 0..chunks_submitted {
        let path = result_rx
            .recv()
            .map_err(|e| io::Error::new(io::ErrorKind::BrokenPipe, e.to_string()))??;
        chunk_paths.push(path);
    }

    for worker in workers {
        worker
            .join()
            .map_err(|_| io::Error::other("Node sort worker thread panicked"))?;
    }

    if chunk_paths.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "No nodes found while building raw node index",
        ));
    }

    let sorted_nodes_path = temp_dir.join("nodes_sorted.bin");
    log::info!(
        "Raw pass A: merging {} sorted node chunks...",
        chunk_paths.len()
    );
    merge_sorted_node_chunks(&chunk_paths, &sorted_nodes_path)?;
    for p in &chunk_paths {
        let _ = std::fs::remove_file(p);
    }

    log::info!(
        "Raw pass A complete: {} nodes indexed in {}ms",
        node_count,
        nodes_start.elapsed().as_millis()
    );

    // Pass B: process ways and resolve refs against on-disk sorted node lookup
    let ways_start = Instant::now();
    let reader_ways = ElementReader::from_path(osm_path).map_err(io::Error::other)?;
    let node_lookup = SortedNodeLookup::open(&sorted_nodes_path)?;

    let data_file = File::create(data_cache_path)?;
    let mut writer = BufWriter::new(data_file);

    let mut way_count: u64 = 0;
    let mut missing_refs: u64 = 0;
    let mut max_points = 0usize;
    let mut way_metas = Vec::new();

    reader_ways
        .for_each(|element| {
            if let Element::Way(way) = element {
                let refs: Vec<i64> = way.refs().collect();
                if refs.is_empty() {
                    return;
                }

                let mut points = Vec::with_capacity(refs.len());
                let mut complete = true;
                for node_id in refs {
                    if let Some(node) = node_lookup.get(node_id) {
                        points.push(Point::new(
                            node.lon_dm7 as f64 / 10_000_000.0,
                            node.lat_dm7 as f64 / 10_000_000.0,
                        ));
                    } else {
                        missing_refs += 1;
                        complete = false;
                        break;
                    }
                }

                if !complete || points.len() < 2 {
                    return;
                }

                let bbox = match BoundingBox::from_points(&points) {
                    Some(b) => b,
                    None => return,
                };

                let tags: Vec<(String, String)> = way
                    .tags()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect();

                let first = &points[0];
                let last = &points[points.len() - 1];
                let is_closed = points.len() >= 4
                    && (first.lon - last.lon).abs() < 1e-10
                    && (first.lat - last.lat).abs() < 1e-10;
                let is_area = is_closed && has_area_tag(&tags);
                let min_zoom = low_zoom_min(&tags);

                let points_len = points.len();
                let map_object = MapObject::new(bbox, points, is_area, tags);
                let offset = match write_map_object(&mut writer, &map_object) {
                    Ok(o) => o,
                    Err(e) => {
                        log::error!("Failed to write map object: {}", e);
                        return;
                    }
                };

                if points_len > max_points {
                    max_points = points_len;
                }
                way_metas.push(WayMeta {
                    offset,
                    bbox,
                    low_zoom_min: min_zoom,
                });

                way_count += 1;
                if way_count.is_multiple_of(100_000) {
                    log::info!(
                        "Raw pass B progress: {} ways parsed/written ({}ms elapsed)",
                        way_count,
                        ways_start.elapsed().as_millis()
                    );
                }
            }
        })
        .map_err(io::Error::other)?;

    writer.flush()?;
    drop(node_lookup);
    let _ = std::fs::remove_file(&sorted_nodes_path);
    let _ = std::fs::remove_dir_all(&temp_dir);

    log::info!(
        "Raw pass B complete: {} ways parsed/written, missing_refs={} in {}ms",
        way_count,
        missing_refs,
        ways_start.elapsed().as_millis()
    );
    Ok((way_metas, max_points))
}

/// Load OSM data with disk caching.
///
/// On first run: parses PBF in parallel, writes data file, builds tile index,
/// caches everything to disk.
/// On subsequent runs: loads cache in seconds if PBF file hasn't changed.
///
/// Cache files are stored under `RUST_OSM_CACHE_DIR` (default: `.osm-cache`):
/// - data: `<name>-<hash>.cache.data` (independent of `max_z`)
/// - meta: `<name>-<hash>.cache.meta` (way metadata for fast index rebuild)
/// - index: `<name>-<hash>.cache.v<policy>.z<max_z>.index` (depends on zoom coverage/policy)
/// - toc: `<name>-<hash>.cache.v<policy>.z<max_z>.toc` (tile->offset table for fast lazy index startup)
pub fn load_osm_data_cached<P: AsRef<Path>>(
    osm_path: P,
    max_z: u32,
) -> io::Result<(TileIndex, PathBuf)> {
    let osm_path = osm_path.as_ref();
    let (data_cache_path, meta_cache_path, index_cache_path, toc_cache_path) =
        cache_paths(osm_path, max_z)?;
    log::info!(
        "Cache paths: data={}, meta={}, index={}, toc={}",
        data_cache_path.display(),
        meta_cache_path.display(),
        index_cache_path.display(),
        toc_cache_path.display()
    );

    let (pbf_size, pbf_mtime) = pbf_metadata(osm_path)?;

    // Fast path: direct index cache hit
    if data_cache_path.exists() && index_cache_path.exists() {
        if !toc_cache_path.exists() {
            log::info!("TOC cache missing, bootstrapping TOC from index cache...");
            let _ = TileIndex::build_toc_from_index(
                &index_cache_path,
                &toc_cache_path,
                pbf_size,
                pbf_mtime,
            );
        }
        let cache_start = Instant::now();
        if let Ok(Some(tile_index)) =
            TileIndex::read_from_mmap_with_toc(
                &index_cache_path,
                &toc_cache_path,
                pbf_size,
                pbf_mtime,
            )
        {
            log::info!(
                "Loaded from cache in {}ms ({} tiles, max {} points)",
                cache_start.elapsed().as_millis(),
                tile_index.len(),
                tile_index.max_points,
            );
            return Ok((tile_index, data_cache_path));
        }
        log::info!("Index cache invalid (or stale), trying metadata cache...");
    }

    // Optimization 1: rebuild index from metadata cache without reparsing PBF
    if data_cache_path.exists() && meta_cache_path.exists() {
        let meta_start = Instant::now();
        if let Ok(mut meta_file) = File::open(&meta_cache_path) {
            if let Ok(Some((way_metas, max_points))) =
                read_way_meta_cache(&mut meta_file, pbf_size, pbf_mtime)
            {
                log::info!(
                    "Loaded metadata cache in {}ms ({} ways); rebuilding z{} index...",
                    meta_start.elapsed().as_millis(),
                    way_metas.len(),
                    max_z
                );

                let index_start = Instant::now();
                let tile_index = build_tile_index_from_way_metas(&way_metas, max_points, max_z);
                log::info!(
                    "Index rebuilt from metadata in {}ms ({} tiles)",
                    index_start.elapsed().as_millis(),
                    tile_index.len()
                );

                let mut index_file = BufWriter::new(File::create(&index_cache_path)?);
                let mut toc_file = BufWriter::new(File::create(&toc_cache_path)?);
                tile_index.write_to_with_toc(
                    &mut index_file,
                    &mut toc_file,
                    pbf_size,
                    pbf_mtime,
                )?;
                index_file.flush()?;
                toc_file.flush()?;

                return Ok((tile_index, data_cache_path));
            }
        }
        log::info!("Metadata cache invalid (or stale), rebuilding from PBF...");
    }

    // Cache miss — build from scratch
    let total_start = Instant::now();
    log::info!("Phase 1: Building data + metadata caches from PBF...");

    let locations_on_ways = has_locations_on_ways(osm_path)?;
    log::info!("Detected LocationsOnWays: {}", locations_on_ways);

    let (way_metas, max_points) = if locations_on_ways {
        build_cache_from_locations_on_ways(osm_path, &data_cache_path)?
    } else {
        build_cache_from_raw_pbf(osm_path, &data_cache_path)?
    };

    // Write metadata cache for fast future index rebuilds
    let meta_start = Instant::now();
    let mut meta_writer = BufWriter::new(File::create(&meta_cache_path)?);
    write_way_meta_cache(
        &mut meta_writer,
        &way_metas,
        max_points,
        pbf_size,
        pbf_mtime,
    )?;
    meta_writer.flush()?;
    log::info!(
        "Phase 1b complete: metadata cached in {}ms",
        meta_start.elapsed().as_millis()
    );

    // Phase 2: Parallel tile index building
    log::info!("Phase 2: Building tile index in parallel...");
    let index_start = Instant::now();
    let tile_index = build_tile_index_from_way_metas(&way_metas, max_points, max_z);

    log::info!(
        "Phase 2 complete: {} tile entries in {}ms",
        tile_index.len(),
        index_start.elapsed().as_millis()
    );

    // Phase 3: Cache tile index to disk
    log::info!("Phase 3: Caching tile index...");
    let cache_start = Instant::now();
    let mut index_file = BufWriter::new(File::create(&index_cache_path)?);
    let mut toc_file = BufWriter::new(File::create(&toc_cache_path)?);
    tile_index.write_to_with_toc(&mut index_file, &mut toc_file, pbf_size, pbf_mtime)?;
    index_file.flush()?;
    toc_file.flush()?;

    log::info!(
        "Phase 3 complete: index cached in {}ms",
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

/// Build/load a low-zoom simplified geometry cache (disk-backed) for faster rendering at z<=low_zoom_max.
pub fn load_low_zoom_simplified_cached<P: AsRef<Path>>(
    osm_path: P,
    max_z: u32,
    low_zoom_max: u32,
) -> io::Result<Option<(TileIndex, PathBuf)>> {
    let osm_path = osm_path.as_ref();
    let (full_data_path, full_meta_path, _, _) = cache_paths(osm_path, max_z)?;
    if !full_data_path.exists() || !full_meta_path.exists() {
        return Ok(None);
    }

    let (low_data_path, low_index_path, low_toc_path) = low_zoom_cache_paths(osm_path, low_zoom_max)?;
    let (pbf_size, pbf_mtime) = pbf_metadata(osm_path)?;

    if low_data_path.exists() && low_index_path.exists() {
        if !low_toc_path.exists() {
            let _ = TileIndex::build_toc_from_index(&low_index_path, &low_toc_path, pbf_size, pbf_mtime);
        }
        if let Ok(Some(idx)) =
            TileIndex::read_from_mmap_with_toc(&low_index_path, &low_toc_path, pbf_size, pbf_mtime)
        {
            log::info!(
                "Loaded low-zoom simplified cache (z<= {}) with {} tiles",
                low_zoom_max,
                idx.len()
            );
            return Ok(Some((idx, low_data_path)));
        }
    }

    log::info!("Building low-zoom simplified cache (z<= {})...", low_zoom_max);
    let mut meta_reader = BufReader::new(File::open(&full_meta_path)?);
    let (way_metas, _) = match read_way_meta_cache(&mut meta_reader, pbf_size, pbf_mtime)? {
        Some(v) => v,
        None => return Ok(None),
    };

    let source_mmap = super::mmap::MappedData::new(&full_data_path)?;
    let mut writer = BufWriter::new(File::create(&low_data_path)?);
    let mut simplified_metas = Vec::with_capacity(way_metas.len());
    let mut max_points = 0usize;
    let batch_size = std::env::var("RUST_OSM_LOWZ_BATCH")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(20_000);

    for (batch_idx, chunk) in way_metas.chunks(batch_size).enumerate() {
        let simplified_batch: Vec<Option<(MapObject, u8)>> = chunk
            .par_iter()
            .map(|meta| {
                if meta.low_zoom_min > low_zoom_max as u8 {
                    return None;
                }
                let view = source_mmap.read_map_object(meta.offset);
                let points = view.points();
                if points.len() < 2 {
                    return None;
                }

                let mut out_points = if view.is_area() && points.len() >= SIMPLIFIED_MIN_AREA_POINTS {
                    simplify_area_points(points, low_zoom_max)
                } else if !view.is_area() && points.len() >= SIMPLIFIED_MIN_POINTS {
                    simplify_line_points(points, low_zoom_max)
                } else {
                    points.to_vec()
                };

                if out_points.len() < 2 {
                    out_points = points.to_vec();
                }

                let bbox = BoundingBox::from_points(&out_points)?;
                let tags: Vec<(String, String)> = view.tags().into_iter().collect();
                let obj = MapObject::new(bbox, out_points, view.is_area(), tags);
                Some((obj, meta.low_zoom_min))
            })
            .collect();

        for entry in simplified_batch.into_iter().flatten() {
            let (obj, low_zoom_min) = entry;
            max_points = max_points.max(obj.points.len());
            let offset = write_map_object(&mut writer, &obj)?;
            simplified_metas.push(WayMeta {
                offset,
                bbox: obj.bounding_box,
                low_zoom_min,
            });
        }

        let done = ((batch_idx + 1) * batch_size).min(way_metas.len());
        if done.is_multiple_of(250_000) || done == way_metas.len() {
            log::info!("Low-zoom simplify progress: {}/{} ways", done, way_metas.len());
        }
    }
    writer.flush()?;

    let tile_index = build_tile_index_from_way_metas(&simplified_metas, max_points, low_zoom_max);
    let mut index_writer = BufWriter::new(File::create(&low_index_path)?);
    let mut toc_writer = BufWriter::new(File::create(&low_toc_path)?);
    tile_index.write_to_with_toc(&mut index_writer, &mut toc_writer, pbf_size, pbf_mtime)?;
    index_writer.flush()?;
    toc_writer.flush()?;

    log::info!(
        "Low-zoom simplified cache built: {} ways, {} tiles",
        simplified_metas.len(),
        tile_index.len()
    );
    Ok(Some((tile_index, low_data_path)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_low_zoom_min() {
        let motorway = vec![("highway".to_string(), "motorway".to_string())];
        assert_eq!(low_zoom_min(&motorway), 5);

        let residential = vec![("highway".to_string(), "residential".to_string())];
        assert_eq!(low_zoom_min(&residential), 11);

        let primary = vec![("highway".to_string(), "primary".to_string())];
        assert_eq!(low_zoom_min(&primary), 7);

        let footway = vec![("highway".to_string(), "footway".to_string())];
        assert_eq!(low_zoom_min(&footway), 11);

        let water = vec![("natural".to_string(), "water".to_string())];
        assert_eq!(low_zoom_min(&water), 6);

        let forest = vec![("landuse".to_string(), "forest".to_string())];
        assert_eq!(low_zoom_min(&forest), 8);

        let rail = vec![("railway".to_string(), "rail".to_string())];
        assert_eq!(low_zoom_min(&rail), 8);

        let admin = vec![("boundary".to_string(), "administrative".to_string())];
        assert_eq!(low_zoom_min(&admin), 7);
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

    #[test]
    fn test_simplify_area_points_keeps_closed_ring() {
        let polygon = vec![
            Point::new(0.0, 0.0),
            Point::new(1.0, 0.0),
            Point::new(2.0, 0.01),
            Point::new(3.0, 0.0),
            Point::new(3.0, 1.0),
            Point::new(2.0, 1.0),
            Point::new(1.0, 1.0),
            Point::new(0.0, 1.0),
            Point::new(0.0, 0.0),
        ];

        let simplified = simplify_area_points(&polygon, 7);
        assert!(simplified.len() >= 4);
        assert!(points_equal(simplified[0], *simplified.last().unwrap()));
    }

    #[test]
    fn test_simplify_area_points_fallback_for_tiny_polygon() {
        let tiny = vec![
            Point::new(0.0, 0.0),
            Point::new(1.0, 0.0),
            Point::new(0.0, 1.0),
            Point::new(0.0, 0.0),
        ];

        let simplified = simplify_area_points(&tiny, 7);
        assert!(simplified.len() >= 4);
        assert!(points_equal(simplified[0], *simplified.last().unwrap()));
    }
}
