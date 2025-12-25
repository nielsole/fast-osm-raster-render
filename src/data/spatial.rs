use std::collections::HashMap;
use super::types::{Tile, MapObjectOffset};

/// Tile key is the unique index for a tile
pub type TileKey = u64;

/// Spatial index mapping tiles to map objects
pub struct TileIndex {
    /// Map from tile key to list of map object offsets
    pub tiles: HashMap<TileKey, Vec<MapObjectOffset>>,
    /// Maximum number of points in any single map object
    pub max_points: usize,
}

impl TileIndex {
    pub fn new() -> Self {
        TileIndex {
            tiles: HashMap::new(),
            max_points: 0,
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        TileIndex {
            tiles: HashMap::with_capacity(capacity),
            max_points: 0,
        }
    }

    /// Insert a map object offset into a tile
    pub fn insert(&mut self, tile: Tile, offset: MapObjectOffset) {
        let key = tile.index();
        self.tiles.entry(key).or_insert_with(Vec::new).push(offset);
    }

    /// Get map object offsets for a tile
    pub fn get(&self, tile: &Tile) -> Option<&Vec<MapObjectOffset>> {
        self.tiles.get(&tile.index())
    }

    /// Get the number of tiles in the index
    pub fn len(&self) -> usize {
        self.tiles.len()
    }

    /// Check if the index is empty
    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }

    /// Update max_points if necessary
    pub fn update_max_points(&mut self, num_points: usize) {
        if num_points > self.max_points {
            self.max_points = num_points;
        }
    }
}

impl Default for TileIndex {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tile_index_insert_and_get() {
        let mut index = TileIndex::new();
        let tile = Tile::new(0, 0, 0);

        index.insert(tile, 100);
        index.insert(tile, 200);

        let offsets = index.get(&tile).unwrap();
        assert_eq!(offsets.len(), 2);
        assert_eq!(offsets[0], 100);
        assert_eq!(offsets[1], 200);
    }

    #[test]
    fn test_tile_index_max_points() {
        let mut index = TileIndex::new();
        assert_eq!(index.max_points, 0);

        index.update_max_points(100);
        assert_eq!(index.max_points, 100);

        index.update_max_points(50);
        assert_eq!(index.max_points, 100);

        index.update_max_points(200);
        assert_eq!(index.max_points, 200);
    }
}
