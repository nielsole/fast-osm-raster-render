use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use memmap2::Mmap;

use super::types::{Tile, MapObjectOffset};

/// Tile key is the unique index for a tile
pub type TileKey = u64;

#[derive(Clone, Copy, Debug)]
struct TileSlice {
    offset_start: usize, // byte offset in mmap where offsets begin
    count: usize,
}

struct LazyTileIndex {
    mmap: Mmap,
    slices: HashMap<TileKey, TileSlice>,
    cache: Mutex<HashMap<TileKey, Arc<Vec<MapObjectOffset>>>>,
}

enum TileIndexInner {
    Eager(HashMap<TileKey, Arc<Vec<MapObjectOffset>>>),
    Lazy(LazyTileIndex),
}

/// Spatial index mapping tiles to map objects
pub struct TileIndex {
    inner: TileIndexInner,
    /// Maximum number of points in any single map object
    pub max_points: usize,
}

impl TileIndex {
    pub fn new() -> Self {
        TileIndex {
            inner: TileIndexInner::Eager(HashMap::new()),
            max_points: 0,
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        TileIndex {
            inner: TileIndexInner::Eager(HashMap::with_capacity(capacity)),
            max_points: 0,
        }
    }

    /// Insert a map object offset into a tile
    pub fn insert(&mut self, tile: Tile, offset: MapObjectOffset) {
        let key = tile.index();
        match &mut self.inner {
            TileIndexInner::Eager(tiles) => {
                let entry = tiles.entry(key).or_insert_with(|| Arc::new(Vec::new()));
                let vec_mut = Arc::make_mut(entry);
                vec_mut.push(offset);
            }
            TileIndexInner::Lazy(_) => {
                panic!("insert called on lazy TileIndex");
            }
        }
    }

    /// Insert offsets by tile key (for bulk merge path in loader)
    pub fn insert_tile_key_offsets(&mut self, key: TileKey, offsets: Vec<MapObjectOffset>) {
        match &mut self.inner {
            TileIndexInner::Eager(tiles) => {
                let entry = tiles.entry(key).or_insert_with(|| Arc::new(Vec::new()));
                let vec_mut = Arc::make_mut(entry);
                vec_mut.extend(offsets);
            }
            TileIndexInner::Lazy(_) => {
                panic!("insert_tile_key_offsets called on lazy TileIndex");
            }
        }
    }

    /// Get map object offsets for a tile
    pub fn get(&self, tile: &Tile) -> Option<Arc<Vec<MapObjectOffset>>> {
        let key = tile.index();
        match &self.inner {
            TileIndexInner::Eager(tiles) => tiles.get(&key).cloned(),
            TileIndexInner::Lazy(lazy) => {
                if let Some(v) = lazy.cache.lock().ok()?.get(&key).cloned() {
                    return Some(v);
                }

                let slice = lazy.slices.get(&key)?;
                let mut offsets = Vec::with_capacity(slice.count);
                for i in 0..slice.count {
                    let byte_pos = slice.offset_start + i * 8;
                    if byte_pos + 8 > lazy.mmap.len() {
                        return None;
                    }
                    let val = unsafe {
                        lazy.mmap
                            .as_ptr()
                            .add(byte_pos)
                            .cast::<u64>()
                            .read_unaligned()
                    };
                    offsets.push(val);
                }
                let arc = Arc::new(offsets);
                if let Ok(mut cache) = lazy.cache.lock() {
                    cache.insert(key, arc.clone());
                }
                Some(arc)
            }
        }
    }

    /// Get the number of tiles in the index
    pub fn len(&self) -> usize {
        match &self.inner {
            TileIndexInner::Eager(tiles) => tiles.len(),
            TileIndexInner::Lazy(lazy) => lazy.slices.len(),
        }
    }

    /// Check if the index is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Update max_points if necessary
    pub fn update_max_points(&mut self, num_points: usize) {
        if num_points > self.max_points {
            self.max_points = num_points;
        }
    }

    /// Magic bytes for the index file format
    const MAGIC: &[u8; 8] = b"OSMIDX02";
    const TOC_MAGIC: &[u8; 8] = b"OSMTOC01";

    /// Write the tile index to a writer with source PBF metadata for cache validation.
    pub fn write_to<W: Write>(&self, writer: &mut W, pbf_size: u64, pbf_mtime: i64) -> io::Result<()> {
        writer.write_all(Self::MAGIC)?;
        writer.write_u64::<LittleEndian>(pbf_size)?;
        writer.write_i64::<LittleEndian>(pbf_mtime)?;
        writer.write_u64::<LittleEndian>(self.max_points as u64)?;
        let tiles = match &self.inner {
            TileIndexInner::Eager(tiles) => tiles,
            TileIndexInner::Lazy(_) => {
                return Err(io::Error::other("write_to not supported for lazy TileIndex"));
            }
        };

        writer.write_u64::<LittleEndian>(tiles.len() as u64)?;

        for (&tile_key, offsets) in tiles {
            writer.write_u64::<LittleEndian>(tile_key)?;
            writer.write_u64::<LittleEndian>(offsets.len() as u64)?;
            for &offset in offsets.iter() {
                writer.write_u64::<LittleEndian>(offset)?;
            }
        }

        Ok(())
    }

    /// Write the tile index and an accompanying TOC sidecar for fast lazy loading.
    pub fn write_to_with_toc<W: Write, TW: Write>(
        &self,
        index_writer: &mut W,
        toc_writer: &mut TW,
        pbf_size: u64,
        pbf_mtime: i64,
    ) -> io::Result<()> {
        let tiles = match &self.inner {
            TileIndexInner::Eager(tiles) => tiles,
            TileIndexInner::Lazy(_) => {
                return Err(io::Error::other("write_to_with_toc not supported for lazy TileIndex"));
            }
        };

        // index header
        index_writer.write_all(Self::MAGIC)?;
        index_writer.write_u64::<LittleEndian>(pbf_size)?;
        index_writer.write_i64::<LittleEndian>(pbf_mtime)?;
        index_writer.write_u64::<LittleEndian>(self.max_points as u64)?;
        index_writer.write_u64::<LittleEndian>(tiles.len() as u64)?;

        // toc header
        toc_writer.write_all(Self::TOC_MAGIC)?;
        toc_writer.write_u64::<LittleEndian>(pbf_size)?;
        toc_writer.write_i64::<LittleEndian>(pbf_mtime)?;
        toc_writer.write_u64::<LittleEndian>(self.max_points as u64)?;
        toc_writer.write_u64::<LittleEndian>(tiles.len() as u64)?;

        // byte cursor within index file (after 40-byte header)
        let mut index_cursor: u64 = 8 + 8 + 8 + 8 + 8;
        for (&tile_key, offsets) in tiles {
            index_writer.write_u64::<LittleEndian>(tile_key)?;
            index_writer.write_u64::<LittleEndian>(offsets.len() as u64)?;
            index_cursor += 16;

            toc_writer.write_u64::<LittleEndian>(tile_key)?;
            toc_writer.write_u64::<LittleEndian>(index_cursor)?;
            toc_writer.write_u64::<LittleEndian>(offsets.len() as u64)?;

            for &offset in offsets.iter() {
                index_writer.write_u64::<LittleEndian>(offset)?;
            }
            index_cursor += (offsets.len() as u64) * 8;
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

        let mut tiles: HashMap<TileKey, Arc<Vec<MapObjectOffset>>> = HashMap::with_capacity(num_tiles);
        for _ in 0..num_tiles {
            let tile_key = reader.read_u64::<LittleEndian>()?;
            let num_offsets = reader.read_u64::<LittleEndian>()? as usize;
            let mut offsets = Vec::with_capacity(num_offsets);
            for _ in 0..num_offsets {
                offsets.push(reader.read_u64::<LittleEndian>()?);
            }
            tiles.insert(tile_key, Arc::new(offsets));
        }

        Ok(Some(TileIndex {
            inner: TileIndexInner::Eager(tiles),
            max_points,
        }))
    }

    /// Read a tile index from a file path using mmap-backed lazy loading.
    /// This avoids loading every tile's full offset list into memory at startup.
    pub fn read_from_mmap<P: AsRef<Path>>(
        path: P,
        expected_pbf_size: u64,
        expected_pbf_mtime: i64,
    ) -> io::Result<Option<Self>> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };
        if mmap.len() < 40 {
            return Ok(None);
        }

        let mut cursor = 0usize;
        let read_u64 = |buf: &[u8], cursor: &mut usize| -> Option<u64> {
            if *cursor + 8 > buf.len() {
                return None;
            }
            let v = u64::from_le_bytes(buf[*cursor..*cursor + 8].try_into().ok()?);
            *cursor += 8;
            Some(v)
        };
        let read_i64 = |buf: &[u8], cursor: &mut usize| -> Option<i64> {
            read_u64(buf, cursor).map(|v| v as i64)
        };

        if cursor + 8 > mmap.len() {
            return Ok(None);
        }
        let magic = &mmap[cursor..cursor + 8];
        cursor += 8;
        if magic != Self::MAGIC {
            return Ok(None);
        }

        let pbf_size = match read_u64(&mmap, &mut cursor) {
            Some(v) => v,
            None => return Ok(None),
        };
        let pbf_mtime = match read_i64(&mmap, &mut cursor) {
            Some(v) => v,
            None => return Ok(None),
        };
        if pbf_size != expected_pbf_size || pbf_mtime != expected_pbf_mtime {
            return Ok(None);
        }

        let max_points = match read_u64(&mmap, &mut cursor) {
            Some(v) => v as usize,
            None => return Ok(None),
        };
        let num_tiles = match read_u64(&mmap, &mut cursor) {
            Some(v) => v as usize,
            None => return Ok(None),
        };

        let mut slices = HashMap::with_capacity(num_tiles);
        for _ in 0..num_tiles {
            let tile_key = match read_u64(&mmap, &mut cursor) {
                Some(v) => v,
                None => return Ok(None),
            };
            let count = match read_u64(&mmap, &mut cursor) {
                Some(v) => v as usize,
                None => return Ok(None),
            };
            let offsets_start = cursor;
            let bytes = count.saturating_mul(8);
            if offsets_start + bytes > mmap.len() {
                return Ok(None);
            }
            slices.insert(
                tile_key,
                TileSlice {
                    offset_start: offsets_start,
                    count,
                },
            );
            cursor += bytes;
        }

        Ok(Some(TileIndex {
            inner: TileIndexInner::Lazy(LazyTileIndex {
                mmap,
                slices,
                cache: Mutex::new(HashMap::new()),
            }),
            max_points,
        }))
    }

    /// Read tile index lazily using a TOC sidecar (no full index scan on startup).
    pub fn read_from_mmap_with_toc<P: AsRef<Path>, TP: AsRef<Path>>(
        index_path: P,
        toc_path: TP,
        expected_pbf_size: u64,
        expected_pbf_mtime: i64,
    ) -> io::Result<Option<Self>> {
        let toc_file = match File::open(toc_path) {
            Ok(f) => f,
            Err(_) => return Ok(None),
        };
        let mut toc_reader = io::BufReader::new(toc_file);

        let mut magic = [0u8; 8];
        if toc_reader.read_exact(&mut magic).is_err() || &magic != Self::TOC_MAGIC {
            return Ok(None);
        }

        let pbf_size = toc_reader.read_u64::<LittleEndian>()?;
        let pbf_mtime = toc_reader.read_i64::<LittleEndian>()?;
        if pbf_size != expected_pbf_size || pbf_mtime != expected_pbf_mtime {
            return Ok(None);
        }

        let max_points = toc_reader.read_u64::<LittleEndian>()? as usize;
        let num_tiles = toc_reader.read_u64::<LittleEndian>()? as usize;
        let mut slices = HashMap::with_capacity(num_tiles);
        for _ in 0..num_tiles {
            let tile_key = toc_reader.read_u64::<LittleEndian>()?;
            let offset_start = toc_reader.read_u64::<LittleEndian>()? as usize;
            let count = toc_reader.read_u64::<LittleEndian>()? as usize;
            slices.insert(tile_key, TileSlice { offset_start, count });
        }

        let index_file = File::open(index_path)?;
        let mmap = unsafe { Mmap::map(&index_file)? };

        Ok(Some(TileIndex {
            inner: TileIndexInner::Lazy(LazyTileIndex {
                mmap,
                slices,
                cache: Mutex::new(HashMap::new()),
            }),
            max_points,
        }))
    }

    /// Build TOC sidecar by scanning an existing index file.
    pub fn build_toc_from_index<P: AsRef<Path>, TP: AsRef<Path>>(
        index_path: P,
        toc_path: TP,
        expected_pbf_size: u64,
        expected_pbf_mtime: i64,
    ) -> io::Result<bool> {
        let mut index = io::BufReader::new(File::open(index_path)?);
        let mut magic = [0u8; 8];
        if index.read_exact(&mut magic).is_err() || &magic != Self::MAGIC {
            return Ok(false);
        }

        let pbf_size = index.read_u64::<LittleEndian>()?;
        let pbf_mtime = index.read_i64::<LittleEndian>()?;
        if pbf_size != expected_pbf_size || pbf_mtime != expected_pbf_mtime {
            return Ok(false);
        }

        let max_points = index.read_u64::<LittleEndian>()?;
        let num_tiles = index.read_u64::<LittleEndian>()?;

        let mut toc = io::BufWriter::new(File::create(toc_path)?);
        toc.write_all(Self::TOC_MAGIC)?;
        toc.write_u64::<LittleEndian>(pbf_size)?;
        toc.write_i64::<LittleEndian>(pbf_mtime)?;
        toc.write_u64::<LittleEndian>(max_points)?;
        toc.write_u64::<LittleEndian>(num_tiles)?;

        let mut cursor: u64 = 8 + 8 + 8 + 8 + 8;
        for _ in 0..num_tiles {
            let tile_key = index.read_u64::<LittleEndian>()?;
            let count = index.read_u64::<LittleEndian>()?;
            cursor += 16;

            toc.write_u64::<LittleEndian>(tile_key)?;
            toc.write_u64::<LittleEndian>(cursor)?;
            toc.write_u64::<LittleEndian>(count)?;

            let skip = (count as i64) * 8;
            index.seek(SeekFrom::Current(skip))?;
            cursor += count * 8;
        }
        toc.flush()?;
        Ok(true)
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
        assert_eq!(offsets_b.as_ref(), &vec![300u64]);
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
