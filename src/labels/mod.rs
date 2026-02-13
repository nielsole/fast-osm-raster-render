use crate::data::types::Tile;
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use fontdue::Font;
use fontdue::layout::{CoordinateSystem, Layout, LayoutSettings, TextStyle};
use image::{Rgba, RgbaImage};
use osmpbf::{Element, ElementReader};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct PlaceLabel {
    pub lon: f64,
    pub lat: f64,
    pub name: String,
    pub priority: i16,
    pub min_zoom: u8,
    pub font_px: f32,
}

pub struct PlaceLabelStore {
    labels: Vec<PlaceLabel>,
    by_tile: HashMap<u64, Vec<usize>>,
    max_zoom: u32,
    font: Font,
}

impl PlaceLabelStore {
    pub fn labels_for_tile(&self, tile: &Tile) -> Vec<&PlaceLabel> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();

        let n = 1u32 << tile.z;
        for dy in -1i32..=1 {
            let y_i = tile.y as i32 + dy;
            if y_i < 0 || y_i >= n as i32 {
                continue;
            }
            let y = y_i as u32;

            for dx in -1i32..=1 {
                let x_i = tile.x as i64 + dx as i64;
                let x = x_i.rem_euclid(n as i64) as u32;
                let key = Tile::new(x, y, tile.z).index();

                if let Some(indices) = self.by_tile.get(&key) {
                    for idx in indices {
                        if !seen.insert(*idx) {
                            continue;
                        }
                        if let Some(label) = self.labels.get(*idx) {
                            if tile.z >= label.min_zoom as u32 {
                                out.push(label);
                            }
                        }
                    }
                }
            }
        }
        out
    }

    pub fn font(&self) -> &Font {
        &self.font
    }

    pub fn max_zoom(&self) -> u32 {
        self.max_zoom
    }
}

const LABEL_MAGIC: &[u8; 8] = b"OSMLBL01";
const DEFAULT_CACHE_DIR: &str = ".osm-cache";
const LEGACY_CACHE_DIR: &str = "/tmp/rust-osm-renderer-cache";

fn cache_prefix(osm_path: &Path) -> io::Result<String> {
    let canonical = std::fs::canonicalize(osm_path).unwrap_or_else(|_| osm_path.to_path_buf());
    let mut hasher = DefaultHasher::new();
    canonical.hash(&mut hasher);
    let hash = hasher.finish();
    let stem = osm_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("osm");
    Ok(format!("{}-{:016x}", stem, hash))
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
        match fs::rename(&src, &dst) {
            Ok(()) => {}
            Err(e) if e.raw_os_error() == Some(18) => {
                if src.is_file() {
                    fs::copy(&src, &dst)?;
                    fs::remove_file(&src)?;
                }
            }
            Err(e) => return Err(e),
        }
    }

    if fs::read_dir(legacy_root)?.next().is_none() {
        let _ = fs::remove_dir(legacy_root);
    }
    Ok(())
}

fn labels_cache_path(osm_path: &Path) -> io::Result<PathBuf> {
    let cache_root = std::env::var("RUST_OSM_CACHE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_CACHE_DIR));
    fs::create_dir_all(&cache_root)?;
    maybe_migrate_legacy_cache(&cache_root)?;
    Ok(cache_root.join(format!("{}.cache.labels", cache_prefix(osm_path)?)))
}

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

fn place_style(place: &str, capital: bool) -> Option<(i16, u8, f32)> {
    let base: (i16, u8, f32) = match place {
        "city" => (100, 5, 20.0),
        "town" => (80, 8, 17.0),
        "village" => (65, 10, 15.0),
        "hamlet" => (50, 12, 13.5),
        "suburb" => (40, 11, 13.0),
        "neighbourhood" | "neighborhood" => (30, 12, 12.0),
        _ => return None,
    };
    if capital {
        Some((base.0 + 20, base.1.saturating_sub(1), base.2 + 1.0))
    } else {
        Some(base)
    }
}

fn load_system_font() -> io::Result<Font> {
    let candidates = [
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
    ];
    for path in candidates {
        if Path::new(path).exists() {
            let bytes = std::fs::read(path)?;
            let font = Font::from_bytes(bytes, fontdue::FontSettings::default())
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
            return Ok(font);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "No supported system font found for label rendering",
    ))
}

fn build_tile_index(labels: &[PlaceLabel], max_zoom: u32) -> HashMap<u64, Vec<usize>> {
    let mut by_tile: HashMap<u64, Vec<usize>> = HashMap::new();
    for (idx, label) in labels.iter().enumerate() {
        for z in label.min_zoom as u32..=max_zoom {
            let n = 2.0_f64.powi(z as i32);
            let x = ((label.lon + 180.0) / 360.0 * n).floor() as i64;
            let lat_rad = label.lat.to_radians();
            let y = ((1.0 - (lat_rad.tan() + (1.0 / lat_rad.cos())).ln() / std::f64::consts::PI)
                / 2.0
                * n)
                .floor() as i64;

            if x >= 0 && y >= 0 {
                let tile = Tile::new(x as u32, y as u32, z);
                by_tile.entry(tile.index()).or_default().push(idx);
            }
        }
    }
    by_tile
}

fn read_labels_cache(
    cache_path: &Path,
    expected_size: u64,
    expected_mtime: i64,
) -> io::Result<Option<(Vec<PlaceLabel>, u32)>> {
    let file = match File::open(cache_path) {
        Ok(f) => f,
        Err(_) => return Ok(None),
    };
    let mut r = BufReader::new(file);
    let mut magic = [0u8; 8];
    if r.read_exact(&mut magic).is_err() || &magic != LABEL_MAGIC {
        return Ok(None);
    }
    let size = r.read_u64::<LittleEndian>()?;
    let mtime = r.read_i64::<LittleEndian>()?;
    if size != expected_size || mtime != expected_mtime {
        return Ok(None);
    }
    let max_zoom = r.read_u32::<LittleEndian>()?;
    let count = r.read_u64::<LittleEndian>()? as usize;
    let mut labels = Vec::with_capacity(count);
    for _ in 0..count {
        let lon = r.read_f64::<LittleEndian>()?;
        let lat = r.read_f64::<LittleEndian>()?;
        let priority = r.read_i16::<LittleEndian>()?;
        let min_zoom = r.read_u8()?;
        let font_px = r.read_f32::<LittleEndian>()?;
        let name_len = r.read_u16::<LittleEndian>()? as usize;
        let mut bytes = vec![0u8; name_len];
        r.read_exact(&mut bytes)?;
        let name = String::from_utf8(bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        labels.push(PlaceLabel {
            lon,
            lat,
            name,
            priority,
            min_zoom,
            font_px,
        });
    }
    Ok(Some((labels, max_zoom)))
}

fn write_labels_cache(
    cache_path: &Path,
    labels: &[PlaceLabel],
    pbf_size: u64,
    pbf_mtime: i64,
    max_zoom: u32,
) -> io::Result<()> {
    let file = File::create(cache_path)?;
    let mut w = BufWriter::new(file);
    w.write_all(LABEL_MAGIC)?;
    w.write_u64::<LittleEndian>(pbf_size)?;
    w.write_i64::<LittleEndian>(pbf_mtime)?;
    w.write_u32::<LittleEndian>(max_zoom)?;
    w.write_u64::<LittleEndian>(labels.len() as u64)?;
    for label in labels {
        w.write_f64::<LittleEndian>(label.lon)?;
        w.write_f64::<LittleEndian>(label.lat)?;
        w.write_i16::<LittleEndian>(label.priority)?;
        w.write_u8(label.min_zoom)?;
        w.write_f32::<LittleEndian>(label.font_px)?;
        w.write_u16::<LittleEndian>(label.name.len() as u16)?;
        w.write_all(label.name.as_bytes())?;
    }
    w.flush()?;
    Ok(())
}

pub fn load_place_labels_cached<P: AsRef<Path>>(osm_path: P, max_zoom: u32) -> io::Result<PlaceLabelStore> {
    let osm_path = osm_path.as_ref();
    let cache_path = labels_cache_path(osm_path)?;
    let (pbf_size, pbf_mtime) = pbf_metadata(osm_path)?;

    let labels = if let Some((labels, cached_max_zoom)) =
        read_labels_cache(&cache_path, pbf_size, pbf_mtime)?
    {
        if cached_max_zoom == max_zoom {
            labels
        } else {
            // Keep labels and rebuild in-memory tile map for new zoom span.
            labels
        }
    } else {
        log::info!("Building place labels cache...");
        let reader = ElementReader::from_path(osm_path).map_err(io::Error::other)?;
        let mut labels = Vec::new();
        let mut checked = 0u64;
        reader
            .for_each(|element| match element {
                Element::Node(node) => {
                    checked += 1;
                    let mut name: Option<String> = None;
                    let mut place: Option<String> = None;
                    let mut capital = false;
                    for (k, v) in node.tags() {
                        match k {
                            "name" => name = Some(v.to_string()),
                            "place" => place = Some(v.to_string()),
                            "capital" => {
                                if v == "yes" || v == "2" {
                                    capital = true;
                                }
                            }
                            _ => {}
                        }
                    }
                    if let (Some(name), Some(place)) = (name, place) {
                        if let Some((priority, min_zoom, font_px)) = place_style(&place, capital) {
                            labels.push(PlaceLabel {
                                lon: node.lon(),
                                lat: node.lat(),
                                name,
                                priority,
                                min_zoom,
                                font_px,
                            });
                        }
                    }
                }
                Element::DenseNode(node) => {
                    checked += 1;
                    let mut name: Option<String> = None;
                    let mut place: Option<String> = None;
                    let mut capital = false;
                    for (k, v) in node.tags() {
                        match k {
                            "name" => name = Some(v.to_string()),
                            "place" => place = Some(v.to_string()),
                            "capital" => {
                                if v == "yes" || v == "2" {
                                    capital = true;
                                }
                            }
                            _ => {}
                        }
                    }
                    if let (Some(name), Some(place)) = (name, place) {
                        if let Some((priority, min_zoom, font_px)) = place_style(&place, capital) {
                            labels.push(PlaceLabel {
                                lon: node.lon(),
                                lat: node.lat(),
                                name,
                                priority,
                                min_zoom,
                                font_px,
                            });
                        }
                    }
                }
                _ => {}
            })
            .map_err(io::Error::other)?;

        labels.sort_by(|a, b| b.priority.cmp(&a.priority).then_with(|| a.name.cmp(&b.name)));
        log::info!("Place labels: selected {} names from {} nodes", labels.len(), checked);
        write_labels_cache(&cache_path, &labels, pbf_size, pbf_mtime, max_zoom)?;
        labels
    };

    let by_tile = build_tile_index(&labels, max_zoom);
    let font = load_system_font()?;
    Ok(PlaceLabelStore {
        labels,
        by_tile,
        max_zoom,
        font,
    })
}

#[derive(Clone, Copy)]
struct Rect {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
}

impl Rect {
    fn overlaps(&self, other: &Rect) -> bool {
        self.x0 < other.x1 && self.x1 > other.x0 && self.y0 < other.y1 && self.y1 > other.y0
    }
}

fn label_screen_position(tile: &Tile, tile_size: u32, lon: f64, lat: f64) -> (f32, f32) {
    let z = tile.z as i32;
    let n = (tile_size as f64) * 2.0_f64.powi(z);
    let world_x = ((lon + 180.0) / 360.0) * n;
    let lat_rad = lat.to_radians();
    let world_y =
        ((1.0 - (lat_rad.tan() + (1.0 / lat_rad.cos())).ln() / std::f64::consts::PI) / 2.0) * n;

    let tile_origin_x = tile.x as f64 * tile_size as f64;
    let tile_origin_y = tile.y as f64 * tile_size as f64;
    (
        (world_x - tile_origin_x) as f32,
        (world_y - tile_origin_y) as f32,
    )
}

fn text_bounds(font: &Font, text: &str, px: f32) -> Option<(f32, f32, f32, f32)> {
    let mut layout = Layout::new(CoordinateSystem::PositiveYDown);
    layout.reset(&LayoutSettings {
        x: 0.0,
        y: 0.0,
        ..LayoutSettings::default()
    });
    layout.append(&[font], &TextStyle::new(text, px, 0));

    let glyphs = layout.glyphs();
    if glyphs.is_empty() {
        return None;
    }

    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;
    for g in glyphs {
        if g.width == 0 || g.height == 0 {
            continue;
        }
        min_x = min_x.min(g.x);
        min_y = min_y.min(g.y);
        max_x = max_x.max(g.x + g.width as f32);
        max_y = max_y.max(g.y + g.height as f32);
    }

    if min_x == f32::MAX {
        return None;
    }
    Some((min_x, min_y, max_x, max_y))
}

fn text_metrics(font: &Font, text: &str, px: f32) -> (f32, f32) {
    if let Some((min_x, min_y, max_x, max_y)) = text_bounds(font, text, px) {
        ((max_x - min_x).max(0.0), (max_y - min_y).max(0.0))
    } else {
        (0.0, 0.0)
    }
}

fn blend_px(dst: &mut Rgba<u8>, src: [u8; 4], alpha: f32) {
    if alpha <= 0.0 {
        return;
    }
    let a = (src[3] as f32 / 255.0) * alpha;
    let inv = 1.0 - a;
    dst[0] = ((src[0] as f32 * a) + (dst[0] as f32 * inv)).clamp(0.0, 255.0) as u8;
    dst[1] = ((src[1] as f32 * a) + (dst[1] as f32 * inv)).clamp(0.0, 255.0) as u8;
    dst[2] = ((src[2] as f32 * a) + (dst[2] as f32 * inv)).clamp(0.0, 255.0) as u8;
    dst[3] = 255;
}

fn draw_text(
    font: &Font,
    image: &mut RgbaImage,
    x: f32,
    y: f32,
    text: &str,
    px: f32,
    color: [u8; 4],
) {
    let Some((min_x, min_y, _, _)) = text_bounds(font, text, px) else {
        return;
    };

    let mut layout = Layout::new(CoordinateSystem::PositiveYDown);
    layout.reset(&LayoutSettings {
        x: 0.0,
        y: 0.0,
        ..LayoutSettings::default()
    });
    layout.append(&[font], &TextStyle::new(text, px, 0));

    for glyph in layout.glyphs() {
        let (metrics, bitmap) = font.rasterize_config(glyph.key);
        // Normalize text origin to top-left so metric and draw anchors match exactly.
        let glyph_x = x + (glyph.x - min_x);
        let glyph_y = y + (glyph.y - min_y);
        for gy in 0..metrics.height {
            for gx in 0..metrics.width {
                let cov = bitmap[gy * metrics.width + gx] as f32 / 255.0;
                if cov <= 0.0 {
                    continue;
                }
                let ix = glyph_x as i32 + gx as i32;
                let iy = glyph_y as i32 + gy as i32;
                if ix < 0 || iy < 0 {
                    continue;
                }
                let (ixu, iyu) = (ix as u32, iy as u32);
                if ixu >= image.width() || iyu >= image.height() {
                    continue;
                }
                let dst = image.get_pixel_mut(ixu, iyu);
                blend_px(dst, color, cov);
            }
        }
    }
}

fn draw_text_with_halo(
    font: &Font,
    image: &mut RgbaImage,
    x: f32,
    y: f32,
    text: &str,
    px: f32,
    fill: [u8; 4],
) {
    let halo = [255, 255, 255, 220];
    let offsets = [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)];
    for (dx, dy) in offsets {
        draw_text(font, image, x + dx, y + dy, text, px, halo);
    }
    draw_text(font, image, x, y, text, px, fill);
}

/// Render collision-aware place labels into a tile image.
/// This is structured around point anchors now and can be extended to way-aligned labels later.
pub fn render_place_labels(tile: &Tile, image: &mut RgbaImage, labels: &PlaceLabelStore) {
    let mut candidates = labels.labels_for_tile(tile);
    candidates.sort_by(|a, b| b.priority.cmp(&a.priority).then_with(|| a.name.len().cmp(&b.name.len())));

    let mut placed: Vec<Rect> = Vec::new();
    let tile_size = image.width();
    for label in candidates {
        let (x, y) = label_screen_position(tile, tile_size, label.lon, label.lat);
        let (w, h) = text_metrics(labels.font(), &label.name, label.font_px);
        if w <= 0.0 || h <= 0.0 {
            continue;
        }

        // place centered above point
        let left = x - w / 2.0;
        let top = y - h - 2.0;
        let rect = Rect {
            x0: left - 2.0,
            y0: top - 2.0,
            x1: left + w + 2.0,
            y1: top + h + 2.0,
        };

        if rect.x1 < 0.0
            || rect.y1 < 0.0
            || rect.x0 > tile_size as f32
            || rect.y0 > image.height() as f32
        {
            continue;
        }

        if placed.iter().any(|p| p.overlaps(&rect)) {
            continue;
        }

        draw_text_with_halo(
            labels.font(),
            image,
            left,
            top,
            &label.name,
            label.font_px,
            [32, 32, 32, 255],
        );
        placed.push(rect);
    }
}
