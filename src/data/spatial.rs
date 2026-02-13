use std::collections::HashMap;
use std::io::{self, Read, Write};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
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

    /// Magic bytes for the index file format
    const MAGIC: &[u8; 8] = b"OSMIDX02";

    /// Write the tile index to a writer with source PBF metadata for cache validation.
    pub fn write_to<W: Write>(&self, writer: &mut W, pbf_size: u64, pbf_mtime: i64) -> io::Result<()> {
        writer.write_all(Self::MAGIC)?;
        writer.write_u64::<LittleEndian>(pbf_size)?;
        writer.write_i64::<LittleEndian>(pbf_mtime)?;
        writer.write_u64::<LittleEndian>(self.max_points as u64)?;
        writer.write_u64::<LittleEndian>(self.tiles.len() as u64)?;

        for (&tile_key, offsets) in &self.tiles {
            writer.write_u64::<LittleEndian>(tile_key)?;
            writer.write_u64::<LittleEndian>(offsets.len() as u64)?;
            for &offset in offsets {
                writer.write_u64::<LittleEndian>(offset)?;
            }
        }

        Ok(())
    }

    /// Read a tile index from a reader. Returns None if the magic or PBF metadata don't match.
    pub fn read_from<R: Read>(reader: &mut R, expected_pbf_size: u64, expected_pbf_mtime: i64) -> io::Result<Option<Self>> {
        let mut magic = [0u8; 8];
        if reader.read_exact(&mut magic).is_err() {
            return Ok(None);
        }
        if &magic != Self::MAGIC {
            return Ok(None);
        }

        let pbf_size = reader.read_u64::<LittleEndian>()?;
        let pbf_mtime = reader.read_i64::<LittleEndian>()?;
        if pbf_size != expected_pbf_size || pbf_mtime != expected_pbf_mtime {
            return Ok(None);
        }

        let max_points = reader.read_u64::<LittleEndian>()? as usize;
        let num_tiles = reader.read_u64::<LittleEndian>()? as usize;

        let mut tiles = HashMap::with_capacity(num_tiles);
        for _ in 0..num_tiles {
            let tile_key = reader.read_u64::<LittleEndian>()?;
            let num_offsets = reader.read_u64::<LittleEndian>()? as usize;
            let mut offsets = Vec::with_capacity(num_offsets);
            for _ in 0..num_offsets {
                offsets.push(reader.read_u64::<LittleEndian>()?);
            }
            tiles.insert(tile_key, offsets);
        }

        Ok(Some(TileIndex { tiles, max_points }))
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
    fn test_tile_index_write_read_roundtrip() {
        let mut index = TileIndex::new();
        let tile_a = Tile::new(0, 0, 0);
        let tile_b = Tile::new(1, 0, 1);

        index.insert(tile_a, 100);
        index.insert(tile_a, 200);
        index.insert(tile_b, 300);
        index.update_max_points(42);

        let pbf_size = 12345u64;
        let pbf_mtime = 1700000000i64;

        let mut buf = Vec::new();
        index.write_to(&mut buf, pbf_size, pbf_mtime).unwrap();

        let mut cursor = std::io::Cursor::new(&buf);
        let loaded = TileIndex::read_from(&mut cursor, pbf_size, pbf_mtime)
            .unwrap()
            .expect("should load successfully");

        assert_eq!(loaded.max_points, 42);
        assert_eq!(loaded.len(), 2);

        let offsets_a = loaded.get(&tile_a).unwrap();
        assert_eq!(offsets_a.len(), 2);
        assert!(offsets_a.contains(&100));
        assert!(offsets_a.contains(&200));

        let offsets_b = loaded.get(&tile_b).unwrap();
        assert_eq!(offsets_b, &vec![300u64]);
    }

    #[test]
    fn test_tile_index_read_wrong_metadata() {
        let mut index = TileIndex::new();
        index.insert(Tile::new(0, 0, 0), 100);

        let mut buf = Vec::new();
        index.write_to(&mut buf, 1000, 2000).unwrap();

        // Wrong size
        let mut cursor = std::io::Cursor::new(&buf);
        let result = TileIndex::read_from(&mut cursor, 9999, 2000).unwrap();
        assert!(result.is_none());

        // Wrong mtime
        let mut cursor = std::io::Cursor::new(&buf);
        let result = TileIndex::read_from(&mut cursor, 1000, 9999).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_tile_index_read_bad_magic() {
        let buf = b"BADMAGIC01234567";
        let mut cursor = std::io::Cursor::new(&buf[..]);
        let result = TileIndex::read_from(&mut cursor, 0, 0).unwrap();
        assert!(result.is_none());
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
