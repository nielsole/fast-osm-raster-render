use crate::data::types::{BoundingBox, Point, Tile};
use std::f64::consts::PI;

const MAX_LAT: f64 = 85.0511287798;

/// Convert latitude to Mercator Y coordinate
pub fn lat_to_mercator(lat: f64) -> f64 {
    let lat_clamped = lat.clamp(-MAX_LAT, MAX_LAT);
    let lat_rad = lat_clamped * PI / 180.0;
    (PI / 4.0 + lat_rad / 2.0).tan().ln()
}

/// Get bounding box for a tile
pub fn get_bounding_box(tile: &Tile) -> BoundingBox {
    let n = 2.0_f64.powi(tile.z as i32);
    let lon_min = (tile.x as f64) / n * 360.0 - 180.0;
    let lat_min = ((PI * (1.0 - 2.0 * (tile.y as f64) / n)).sinh()).atan() * 180.0 / PI;
    let lon_max = ((tile.x + 1) as f64) / n * 360.0 - 180.0;
    let lat_max = ((PI * (1.0 - 2.0 * ((tile.y + 1) as f64) / n)).sinh()).atan() * 180.0 / PI;

    BoundingBox {
        min: Point {
            lon: lon_min.min(lon_max),
            lat: lat_min.min(lat_max),
        },
        max: Point {
            lon: lon_min.max(lon_max),
            lat: lat_min.max(lat_max),
        },
    }
}

/// Convert lat/lon to tile coordinates at a given zoom level
pub fn deg2num(lat_deg: f64, lon_deg: f64, zoom: u32) -> (u32, u32) {
    let lat_rad = PI * lat_deg / 180.0;
    let n = 2.0_f64.powi(zoom as i32);
    let x = ((lon_deg + 180.0) / 360.0 * n).floor() as u32;
    let y = ((1.0 - (lat_rad.tan() + (1.0 / lat_rad.cos())).ln() / PI) / 2.0 * n).floor() as u32;
    (x, y)
}

/// Get all tiles that overlap with a bounding box for a range of zoom levels
pub fn get_tiles_for_bounding_box(bbox: &BoundingBox, min_z: u32, max_z: u32) -> Vec<Tile> {
    let mut tiles = Vec::new();

    for z in min_z..=max_z {
        let (min_x, min_y) = deg2num(bbox.max.lat, bbox.min.lon, z);
        let (max_x, max_y) = deg2num(bbox.min.lat, bbox.max.lon, z);

        for x in min_x..=max_x {
            for y in min_y..=max_y {
                tiles.push(Tile { x, y, z });
            }
        }
    }

    tiles
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lat_to_mercator() {
        // Test some known values
        let y = lat_to_mercator(0.0);
        assert!((y - 0.0).abs() < 1e-10);
    }

    #[test]
    fn test_deg2num() {
        // Test tile 0,0,0 contains the whole world
        let (x, y) = deg2num(0.0, 0.0, 0);
        assert_eq!(x, 0);
        assert_eq!(y, 0);
    }
}
