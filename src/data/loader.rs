use super::serialization::write_map_object;
use super::spatial::TileIndex;
use super::types::{BoundingBox, MapObject, Point};
use crate::projection::get_tiles_for_bounding_box;
use osmpbf::{Element, ElementReader};
use std::fs::File;
use std::io;
use std::path::Path;

/// Check if a way should be displayed at zoom levels < 11
/// Only major roads are shown at lower zoom levels
fn is_important_way(tags: &[(String, String)]) -> bool {
    for (key, value) in tags {
        if key == "highway" {
            match value.as_str() {
                "motorway" | "trunk" | "primary" | "secondary" | "tertiary"
                | "motorway_link" | "trunk_link" | "primary_link"
                | "secondary_link" | "tertiary_link" => return true,
                _ => {}
            }
        }
    }
    false
}

/// Load OSM data from a PBF file and build spatial index
///
/// # Arguments
/// * `osm_path` - Path to the OSM PBF file
/// * `max_z` - Maximum zoom level to index (typically 15)
/// * `temp_file` - Temporary file to write map objects to
///
/// # Returns
/// A TileIndex containing the spatial index and metadata
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

                // Create map object
                let map_object = MapObject::new(bounding_box, points);

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

                // Get tags for filtering
                let tags: Vec<(String, String)> = way
                    .tags()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect();
                let is_important = is_important_way(&tags);

                // Get all tiles that overlap with this way's bounding box
                let tiles = get_tiles_for_bounding_box(&bounding_box, 0, max_z);

                for tile in tiles {
                    // Skip non-important ways at zoom < 11
                    if !is_important && tile.z < 11 {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_important_way() {
        let motorway = vec![("highway".to_string(), "motorway".to_string())];
        assert!(is_important_way(&motorway));

        let residential = vec![("highway".to_string(), "residential".to_string())];
        assert!(!is_important_way(&residential));

        let primary = vec![("highway".to_string(), "primary".to_string())];
        assert!(is_important_way(&primary));

        let footway = vec![("highway".to_string(), "footway".to_string())];
        assert!(!is_important_way(&footway));
    }
}
